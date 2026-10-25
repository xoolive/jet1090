#![allow(clippy::match_result_ok)]
mod api {
    tonic::include_proto!("serosystems.proto.v3.backend.api");
}

use api::{
    se_ro_api_client::SeRoApiClient, ModeSDownlinkFrame,
    ModeSDownlinkFramesRequest, Sensor, SensorInfoRequest, SensorInfoResponse,
    TimingBase, TimingSyncSource,
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

use crate::decode::time::{checked_gps_week_to_unix_ns, now_in_ns};
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
        let observation_ns = now_in_ns();
        let system_timestamp = observation_ns as f64 * 1e-9;
        let metadata: Vec<SensorMetadata> = msg
            .receptions
            .into_iter()
            .filter_map(|rm| {
                let sensor = rm.sensor?;
                // Trust SeRo's declared clock, not the numeric field's name:
                // SYSTEM_TIME is uptime and cannot be converted as GPS ToW.
                let synchronized = matches!(
                    TimingSyncSource::try_from(rm.timing_sync_source),
                    Ok(TimingSyncSource::Gnss | TimingSyncSource::AtomicClock)
                );
                let capture_ns = if rm.timing_base == TimingBase::GpsTow as i32
                    && synchronized
                {
                    checked_gps_week_to_unix_ns(
                        rm.gnss_timestamp,
                        observation_ns,
                    )
                } else {
                    None
                };
                Some(SensorMetadata {
                    system_timestamp,
                    gnss_timestamp: capture_ns.map(|ns| ns as f64 * 1e-9),
                    nanoseconds: capture_ns
                        .map(|ns| (ns % 86_400_000_000_000) as u64),
                    rssi: Some(rm.signal_level),
                    serial: sensor.serial,
                    name: sensor_map.get(&sensor.serial).cloned(),
                })
            })
            .collect();
        let timestamp = metadata
            .iter()
            .filter_map(|meta| meta.gnss_timestamp)
            .min_by(f64::total_cmp)
            .unwrap_or(system_timestamp);

        let tmsg = TimedMessage {
            timestamp,
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

    fn reception_at(
        unix_ns: u128,
        serial: u64,
        base: i32,
        sync: i32,
    ) -> api::ReceptionMetadata {
        let week_start = crate::decode::time::gps_week_in_s(
            (unix_ns / 1_000_000_000) as u64,
        );
        api::ReceptionMetadata {
            sensor: Some(Sensor {
                serial,
                ..Default::default()
            }),
            gnss_timestamp: (unix_ns - u128::from(week_start) * 1_000_000_000)
                as u64,
            timing_base: base,
            timing_sync_source: sync,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn capture_clock_preserves_spacing_despite_bursty_delivery() {
        let start = now_in_ns() - 2_000_000_000;
        let frames = [start, start + 440_000_000].map(|time| {
            Ok(ModeSDownlinkFrame {
                reply: vec![1, 2, 3],
                receptions: vec![reception_at(
                    time,
                    42,
                    api::TimingBase::GpsTow as i32,
                    api::TimingSyncSource::Gnss as i32,
                )],
                ..Default::default()
            })
        });
        let (tx, mut rx) = mpsc::channel(2);
        forward_stream(
            futures::stream::iter(frames),
            HashMap::new(),
            tx,
            || {},
        )
        .await
        .unwrap();
        let first = rx.recv().await.unwrap();
        let second = rx.recv().await.unwrap();
        assert!((first.timestamp - start as f64 * 1e-9).abs() < 1e-6);
        assert!((second.timestamp - first.timestamp - 0.44).abs() < 1e-6);
        assert!(first.metadata[0].system_timestamp > first.timestamp + 1.0);
        assert_eq!(first.metadata[0].serial, 42);
        assert_eq!(first.frame, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn only_declared_synchronized_gps_clocks_are_selected() {
        use api::{TimingBase, TimingSyncSource};
        let capture_ns = now_in_ns() - 2_000_000_000;
        for (base, sync, usable) in [
            (
                TimingBase::GpsTow as i32,
                TimingSyncSource::Gnss as i32,
                true,
            ),
            (
                TimingBase::GpsTow as i32,
                TimingSyncSource::AtomicClock as i32,
                true,
            ),
            (
                TimingBase::GpsTow as i32,
                TimingSyncSource::NoSource as i32,
                false,
            ),
            (
                TimingBase::SystemTime as i32,
                TimingSyncSource::Gnss as i32,
                false,
            ),
            (
                TimingBase::NoBase as i32,
                TimingSyncSource::Gnss as i32,
                false,
            ),
            (99, TimingSyncSource::Gnss as i32, false),
            (TimingBase::GpsTow as i32, 99, false),
        ] {
            let frame = ModeSDownlinkFrame {
                receptions: vec![reception_at(capture_ns, 42, base, sync)],
                ..Default::default()
            };
            let (tx, mut rx) = mpsc::channel(1);
            forward_stream(
                futures::stream::iter([Ok(frame)]),
                HashMap::new(),
                tx,
                || {},
            )
            .await
            .unwrap();
            let message = rx.recv().await.unwrap();
            let metadata = &message.metadata[0];
            assert_eq!(
                metadata.gnss_timestamp.is_some(),
                usable,
                "base={base}, sync={sync}"
            );
            assert_eq!(metadata.nanoseconds.is_some(), usable);
            if usable {
                assert_eq!(message.timestamp, metadata.gnss_timestamp.unwrap());
            } else {
                assert_eq!(message.timestamp, metadata.system_timestamp);
            }
        }
    }

    #[tokio::test]
    async fn earliest_usable_reception_wins_without_rewriting_sensor_metadata()
    {
        use api::{TimingBase, TimingSyncSource};
        let capture_ns = now_in_ns() - 2_000_000_000;
        let mut malformed = reception_at(
            capture_ns,
            4,
            TimingBase::GpsTow as i32,
            TimingSyncSource::Gnss as i32,
        );
        malformed.gnss_timestamp = u64::MAX;
        let frame = ModeSDownlinkFrame {
            receptions: vec![
                reception_at(
                    capture_ns + 100_000_000,
                    1,
                    TimingBase::GpsTow as i32,
                    TimingSyncSource::Gnss as i32,
                ),
                reception_at(
                    capture_ns,
                    2,
                    TimingBase::GpsTow as i32,
                    TimingSyncSource::Gnss as i32,
                ),
                reception_at(
                    capture_ns - 100_000_000,
                    3,
                    TimingBase::SystemTime as i32,
                    TimingSyncSource::Gnss as i32,
                ),
                malformed,
            ],
            ..Default::default()
        };
        let (tx, mut rx) = mpsc::channel(1);
        forward_stream(
            futures::stream::iter([Ok(frame)]),
            HashMap::from([(2, "sensor-two".to_string())]),
            tx,
            || {},
        )
        .await
        .unwrap();
        let message = rx.recv().await.unwrap();
        assert!((message.timestamp - capture_ns as f64 * 1e-9).abs() < 1e-6);
        assert_eq!(message.metadata.len(), 4);
        assert_eq!(message.metadata[1].name.as_deref(), Some("sensor-two"));
        assert_eq!(
            message
                .metadata
                .iter()
                .map(|m| m.serial)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert!(message.metadata[2].gnss_timestamp.is_none());
        assert!(message.metadata[3].gnss_timestamp.is_none());
    }

    #[tokio::test]
    async fn absent_reception_identity_does_not_panic() {
        let frame = ModeSDownlinkFrame {
            receptions: vec![api::ReceptionMetadata::default()],
            ..Default::default()
        };
        let (tx, mut rx) = mpsc::channel(1);
        forward_stream(
            futures::stream::iter([Ok(frame)]),
            HashMap::new(),
            tx,
            || {},
        )
        .await
        .unwrap();
        assert!(rx.recv().await.unwrap().metadata.is_empty());
    }

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
