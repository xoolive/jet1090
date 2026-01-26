use async_stream::stream;
use futures::stream::SplitStream;
use futures_util::pin_mut;
use futures_util::stream::{Stream, StreamExt};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc;
#[cfg(feature = "ssh")]
use tokio_tungstenite::client_async;
use tokio_tungstenite::connect_async;
#[cfg(feature = "ssh")]
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{
    tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info};

use std::collections::HashSet;
use std::io;

use crate::decode::time::{now_in_ns, since_today_to_nanos};
use crate::prelude::*;

/// Iterate a Beast binary feed.
///
///  - esc "1" : 6 byte MLAT timestamp, 1 byte signal level, 2 byte Mode-AC
///  - esc "2" : 6 byte MLAT timestamp, 1 byte signal level, 7 byte Mode-S short frame
///  - esc "3" : 6 byte MLAT timestamp, 1 byte signal level, 14 byte Mode-S long frame
///  - esc "4" : 6 byte MLAT timestamp, status data, DIP switch configuration settings (not on Mode-S Beast classic)
///
/// esc esc: true 0x1a
/// esc is 0x1a, and "1", "2" and "3" are 0x31, 0x32 and 0x33
///
/// Decoding the timestamp:
/// <https://wiki.modesbeast.com/Radarcape:Firmware_Versions#The_GPS_timestamp>
pub type WsStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;
#[cfg(feature = "ssh")]
pub type TunnelledWsStream = SplitStream<WebSocketStream<makiko::TunnelStream>>;

pub enum DataSource {
    Tcp(TcpStream),
    Udp(UdpSocket),
    Websocket(WsStream),
    #[cfg(feature = "ssh")]
    TunnelledTcp(makiko::TunnelReceiver),
    #[cfg(feature = "ssh")]
    TunnelledWebSocket(TunnelledWsStream),
}

pub enum BeastSource {
    Tcp(String),
    Udp(String),
    Websocket(String),
    #[cfg(feature = "ssh")]
    TunnelledTcp(super::ssh::TunnelledTcp),
    #[cfg(feature = "ssh")]
    TunnelledWebsocket(super::ssh::TunnelledWebsocket),
}

pub async fn next_msg(mut stream: DataSource) -> impl Stream<Item = Vec<u8>> {
    // Initialize a HashSet to check for valid message types
    let valid_msg_types: HashSet<u8> =
        vec![0x31, 0x32, 0x33, 0x34].into_iter().collect();

    let mut data = Vec::new();
    stream! {
    loop {
        // Read from the stream into the buffer
        let mut buffer = [0u8; 1024];
        let bytes_read = match &mut stream {
            DataSource::Tcp(tcp_stream) => {
                match tcp_stream.read(&mut buffer).await {
                    Ok(0) => break, // Connection closed by peer
                    Ok(n) => n,
                    Err(e) => {
                        error!("Error reading from socket: {}", e);
                        break;
                    }
                }
            }
            DataSource::Udp(udp_socket) => {
                match udp_socket.recv_from(&mut buffer).await {
                    Ok((n, _)) => n,
                    Err(e) => {
                        error!("Error reading from socket: {}", e);
                        break;
                    }
                }
            }
            DataSource::Websocket(ws_receive) => {
                match ws_receive.next().await {
                    Some(Ok(Message::Binary(data))) => {
                        debug!("Received {:?}", data);
                        let len = data.len().min(buffer.len());
                        buffer[..len].copy_from_slice(&data[..len]);
                        len
                    }
                    _ => {
                        error!("Error reading from websocket");
                        break;
                    }
                }
            }
            #[cfg(feature = "ssh")]
            DataSource::TunnelledTcp(tunnel_rx) => {
                match tunnel_rx.recv().await {
                    Ok(Some(makiko::TunnelEvent::Data(data))) => {
                        debug!("Received {:?}", data);
                        let len = data.len().min(buffer.len());
                        buffer[..len].copy_from_slice(&data[..len]);
                        len
                    }
                    _ => {
                        error!("Error reading from tunnel");
                        break;
                    }
                }
            }
            #[cfg(feature = "ssh")]
            DataSource::TunnelledWebSocket(tunnel_rx) => {
                match tunnel_rx.next().await {
                    Some(Ok(Message::Binary(data))) => {
                        debug!("Received {:?}", data);
                        let len = data.len().min(buffer.len());
                        buffer[..len].copy_from_slice(&data[..len]);
                        len
                    }
                    _ => {
                        error!("Error reading from tunnel");
                        break;
                    }
                }
            }
        };

        // Extend the data vector with the read bytes
        data.extend_from_slice(&buffer[..bytes_read]);

        while data.len() >= 23 {
            if let Some(it) = data.iter().position(|&x| x == 0x1A) {
                data = data.split_off(it);

                if data.len() < 23 {
                    break;
                }

                let msg_type = data[1];
                if valid_msg_types.contains(&msg_type) {
                    // Collapse consecutive 0x1A into a single 0x1A
                    let mut ref_idx = 1;
                    let mut idx;
                    let msg_size = match msg_type {
                        0x31 => 11,
                        0x32 => 16,
                        0x33 => 23,
                        0x34 => 23, // Adjust the message size accordingly
                        _ => 0,
                    };

                    loop {
                        idx = data[ref_idx..msg_size.min(data.len())]
                            .iter()
                            .position(|&x| x == 0x1A);
                        if let Some(start) = idx.map(|idx| ref_idx + idx) {
                            ref_idx = start + 1;
                            if data.get(ref_idx) == Some(&0x1A) {
                                data.splice(start..=start, std::iter::empty());
                            }
                        } else {
                            break;
                        }
                    }

                    if idx.is_some() || data.len() < msg_size {
                        // Move to the next buffer
                        break;
                    }

                    let msg = data.drain(..msg_size).collect::<Vec<u8>>();
                    if msg_type != 0x34 {
                        yield msg
                    }
                } else {
                    // Probably corrupted message
                    data = data.split_off(1);
                }
            } else {
                break;
            }
        }
    }
    }
}

pub async fn receiver(
    address: BeastSource,
    tx: mpsc::Sender<TimedMessage>,
    serial: u64,
    name: Option<String>,
) -> io::Result<()> {
    let msg_stream = match address {
        BeastSource::Tcp(address) => match TcpStream::connect(&address).await {
            Ok(stream) => {
                info!("Connected to TCP stream: {}", address);
                DataSource::Tcp(stream)
            }
            Err(error) => {
                info!(
                    "Failed to connect to TCP {} ({}), trying in UDP",
                    address,
                    error.to_string()
                );
                DataSource::Udp(UdpSocket::bind(&address).await?)
            }
        },
        BeastSource::Udp(address) => {
            DataSource::Udp(UdpSocket::bind(&address).await?)
        }
        BeastSource::Websocket(address) => {
            info!("Connecting to websocket: {}", address);
            let (stream, _) =
                connect_async(&address).await.map_err(io::Error::other)?;
            info!("Connected to websocket: {}", address);
            let (_, rx) = stream.split();
            DataSource::Websocket(rx)
        }
        #[cfg(feature = "ssh")]
        BeastSource::TunnelledTcp(tunnel) => {
            let tunnel_rx = tunnel.connect().await.expect("Failed to connect");
            DataSource::TunnelledTcp(tunnel_rx)
        }
        #[cfg(feature = "ssh")]
        BeastSource::TunnelledWebsocket(tunnel) => {
            let stream = tunnel.connect().await.expect("Failed to connect");
            let url =
                tunnel.url.into_client_request().expect("Invalid request");

            // Now perform the handshake using client_async, which accepts an already connected stream.
            let (ws_stream, _) =
                client_async(url, stream).await.expect("Failed to connect");

            let (_, rx) = ws_stream.split();
            DataSource::TunnelledWebSocket(rx)
        }
    };

    let msg_stream = beast::next_msg(msg_stream).await;
    pin_mut!(msg_stream); // needed for iteration
    'receive: loop {
        while let Some(msg) = msg_stream.next().await {
            let tmsg = process_radarcape(&msg, serial, name.clone());
            info!("Received {}", tmsg);
            if tx.send(tmsg).await.is_err() {
                break 'receive;
            }
        }
    }
    Ok(())
}

fn process_radarcape(
    msg: &[u8],
    serial: u64,
    name: Option<String>,
) -> TimedMessage {
    // Copy the bytes from the slice into the array starting from index 2
    let mut array = [0u8; 8];
    array[2..8].copy_from_slice(&msg[2..8]);

    let ts_u64 = u64::from_be_bytes(array);
    let seconds = ts_u64 as u128 >> 30;
    let nanos = ts_u64 & 0x00003FFFFFFF;
    let timestamp_in_s =
        since_today_to_nanos(seconds * 1_000_000_000 + nanos as u128) as f64
            * 1e-9;

    let system_timestamp = now_in_ns() as f64 * 1e-9;

    // Validate Beast GNSS timestamp: if it differs from system time by >1 hour, it's invalid
    let gnss_timestamp_valid =
        (system_timestamp - timestamp_in_s).abs() < 3600.;
    let gnss_timestamp = if gnss_timestamp_valid {
        Some(timestamp_in_s)
    } else {
        None
    };

    let rssi = if msg[8] == 0xff { None } else { Some(msg[8]) };
    let rssi = rssi.map(|v| v as f64 / 255.);
    let rssi = rssi.map(|v| 10. * (v * v).log10() as f32);

    // In some cases, the timestamp is just the one of dump1090, so forget it!
    let metadata = SensorMetadata {
        system_timestamp,
        gnss_timestamp,
        nanoseconds: Some(ts_u64),
        rssi,
        serial,
        name,
    };

    TimedMessage {
        // Use Beast GNSS timestamp if valid, fall back to system timestamp
        timestamp: if gnss_timestamp_valid {
            timestamp_in_s
        } else {
            system_timestamp
        },
        frame: msg[9..].to_vec(),
        message: None,
        metadata: vec![metadata],
        decode_time: None,
    }
}
