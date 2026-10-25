#![allow(clippy::match_result_ok)]
mod api {
    tonic::include_proto!("serosystems.proto.v3.backend.api");
}

use api::{
    se_ro_api_client::SeRoApiClient, ModeSDownlinkFrame,
    ModeSDownlinkFramesRequest, Sensor, SensorInfoRequest, SensorInfoResponse,
};
#[cfg(feature = "ssh")]
use hyper_util::rt::tokio::TokioIo;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tonic::{
    transport::{Certificate, Channel, ClientTlsConfig, Endpoint},
    Streaming,
};
use tracing::info;

use crate::decode::time::{
    now_in_ns, since_gps_week_to_since_today, since_gps_week_to_unix_s,
};
use crate::prelude::*;
#[cfg(feature = "ssh")]
use crate::source::ssh::TunnelledSero;

type StaticBoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T> = std::result::Result<T, StaticBoxError>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeroClient {
    pub token: String,
    pub df_filter: Vec<u32>,
    pub aircraft_filter: Vec<u32>,
    pub sensor_filter: Vec<String>,
    pub jump: Option<String>,
}

async fn download_file(url: &str, destination: &PathBuf) -> Result<()> {
    let response = reqwest::get(url).await?.bytes().await?;
    let mut file = File::create(destination).await?;
    file.write_all(&response).await?;
    Ok(())
}

pub async fn receiver(
    sero: SeroClient,
    tx: mpsc::Sender<TimedMessage>,
) -> Result<()> {
    receiver_with_connected_callback(sero, tx, || {}).await
}

/// Receive SeRo frames and notify the caller once the authenticated stream and
/// sensor metadata are ready, even if no frames have arrived yet.
pub async fn receiver_with_connected_callback<F: FnOnce()>(
    sero: SeroClient,
    tx: mpsc::Sender<TimedMessage>,
    on_connected: F,
) -> Result<()> {
    let stream = sero.rawstream().await?;
    let info = &sero.info().await?;
    let sensor_map: HashMap<u64, String> = info
        .sensor_info
        .iter()
        .map(|elt| (elt.sensor.unwrap().serial, elt.alias.to_string()))
        .collect();
    forward_stream(stream, sensor_map, tx, on_connected).await
}

async fn forward_stream<S, F>(
    mut stream: S,
    sensor_map: HashMap<u64, String>,
    tx: mpsc::Sender<TimedMessage>,
    on_connected: F,
) -> Result<()>
where
    S: futures::Stream<
            Item = std::result::Result<ModeSDownlinkFrame, tonic::Status>,
        > + Unpin,
    F: FnOnce(),
{
    on_connected();
    while let Some(response) = stream.next().await {
        let msg = response?;
        let bytes = msg.reply.as_slice();
        let system_timestamp = now_in_ns() as f64 * 1e-9;
        let metadata = msg
            .receptions
            .into_iter()
            .map(|rm| SensorMetadata {
                system_timestamp,
                gnss_timestamp: Some(since_gps_week_to_unix_s(
                    rm.gnss_timestamp,
                )),
                nanoseconds: Some(since_gps_week_to_since_today(
                    rm.gnss_timestamp,
                )),
                rssi: Some(rm.signal_level),
                serial: rm.sensor.unwrap().serial,
                name: sensor_map.get(&rm.sensor.unwrap().serial).cloned(),
            })
            .collect();

        let tmsg = TimedMessage {
            timestamp: system_timestamp,
            frame: bytes.to_vec(),
            message: None,
            metadata,
            decode_time: None,
        };
        if tx.send(tmsg).await.is_err() {
            return Ok(());
        }
    }
    Ok(())
}

impl SeroClient {
    pub async fn client(&self) -> Result<SeRoApiClient<Channel>> {
        let mut cache_path = dirs::cache_dir().unwrap_or_default();
        cache_path.push("jet1090");
        if !cache_path.exists() {
            let msg =
                format!("failed to create {:?}", cache_path.to_str().unwrap());
            fs::create_dir_all(&cache_path).await.expect(&msg);
        }

        let ca_cert_url = "https://doc.sero-systems.de/api/_downloads/017ce4f89360621e0345c257b6136b21/ca.crt";
        let ca_cert_file = "ca_sero.crt";

        cache_path.push(ca_cert_file);
        if !cache_path.exists() {
            info!("Downloading sero certificate");
            download_file(ca_cert_url, &cache_path).await?;
        }

        // Load the CA certificate
        info!("Loading sero certificate: {:?}", cache_path);
        let ca_cert = fs::read(cache_path).await?;
        let ca_cert = Certificate::from_pem(ca_cert);

        // Configure TLS
        let tls_config = ClientTlsConfig::new().ca_certificate(ca_cert);

        // Build the channel with TLS configuration
        let endpoint = Endpoint::from_static("https://api.secureadsb.com:4201")
            .tls_config(tls_config)?;

        #[cfg(not(feature = "ssh"))]
        let channel = endpoint.connect().await?;

        #[cfg(feature = "ssh")]
        let channel = match &self.jump {
            None => endpoint.connect().await?,
            Some(jump) => {
                let jump = jump.clone();
                let connector = tower::service_fn(move |_uri: http::Uri| {
                    let jump = jump.clone();
                    async move {
                        let tunnel = TunnelledSero { jump };
                        let stream =
                            tunnel.connect().await.map_err(|error| {
                                std::io::Error::other(error.to_string())
                            })?;
                        Ok::<_, std::io::Error>(TokioIo::new(stream))
                    }
                });
                endpoint.connect_with_connector(connector).await?
            }
        };

        Ok(SeRoApiClient::new(channel))
    }

    pub async fn info(&self) -> Result<SensorInfoResponse> {
        let request = tonic::Request::new(SensorInfoRequest {
            token: self.token.clone(),
            sensors: vec![],
        });
        Ok(self
            .client()
            .await?
            .get_sensor_info(request)
            .await?
            .into_inner())
    }

    pub async fn rawstream(&self) -> Result<Streaming<ModeSDownlinkFrame>> {
        let sensor_filter: Vec<Sensor> = self
            .info()
            .await?
            .sensor_info
            .iter()
            .filter(|s| self.sensor_filter.contains(&s.alias))
            .map(|s| s.sensor.unwrap())
            .collect();
        let request = tonic::Request::new(ModeSDownlinkFramesRequest {
            token: self.token.clone(),
            df_filter: self.df_filter.clone(),
            aircraft_filter: self.aircraft_filter.clone(),
            sensor_filter,
        });
        Ok(self
            .client()
            .await?
            .get_mode_s_downlink_frames(request)
            .await?
            .into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn connection_is_reported_before_any_frame_arrives() {
        let (tx, _rx) = mpsc::channel(1);
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(forward_stream(
            futures::stream::pending(),
            HashMap::new(),
            tx,
            move || {
                let _ = ready_tx.send(());
            },
        ));
        tokio::time::timeout(Duration::from_secs(1), ready_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(!task.is_finished());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn stream_errors_are_propagated_instead_of_silently_ignored() {
        let (tx, _rx) = mpsc::channel(1);
        let error = forward_stream(
            futures::stream::iter([Err(tonic::Status::unavailable(
                "connection lost",
            ))]),
            HashMap::new(),
            tx,
            || {},
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("connection lost"));
    }

    #[tokio::test]
    async fn forwards_frames_after_reporting_connection() {
        let (tx, mut rx) = mpsc::channel(1);
        let frame = ModeSDownlinkFrame {
            reply: vec![1, 2, 3],
            ..Default::default()
        };
        let mut connected = false;
        forward_stream(
            futures::stream::iter([Ok(frame)]),
            HashMap::new(),
            tx,
            || {
                connected = true;
            },
        )
        .await
        .unwrap();
        assert!(connected);
        assert_eq!(rx.recv().await.unwrap().frame, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn closed_consumer_stops_an_otherwise_live_stream() {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let stream = futures::stream::iter([Ok(ModeSDownlinkFrame::default())])
            .chain(futures::stream::pending());
        tokio::time::timeout(
            Duration::from_secs(1),
            forward_stream(stream, HashMap::new(), tx, || {}),
        )
        .await
        .unwrap()
        .unwrap();
    }
}
