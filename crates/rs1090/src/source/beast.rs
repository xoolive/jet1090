use async_stream::stream;
use futures::stream::SplitStream;
use futures_util::pin_mut;
use futures_util::stream::{Stream, StreamExt};
use socket2::{SockRef, TcpKeepalive};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
#[cfg(feature = "ssh")]
use tokio_tungstenite::client_async;
use tokio_tungstenite::connect_async;
#[cfg(feature = "ssh")]
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{
    tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, warn};

use std::collections::HashSet;
use std::fmt;
use std::io;
use std::time::Duration;

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

impl fmt::Display for BeastSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BeastSource::Tcp(address) => write!(f, "tcp://{address}"),
            BeastSource::Udp(address) => write!(f, "udp://{address}"),
            BeastSource::Websocket(address) => write!(f, "{address}"),
            #[cfg(feature = "ssh")]
            BeastSource::TunnelledTcp(tunnel) => write!(
                f,
                "tcp://{}:{} (via {})",
                tunnel.address, tunnel.port, tunnel.jump
            ),
            #[cfg(feature = "ssh")]
            BeastSource::TunnelledWebsocket(tunnel) => {
                write!(f, "{} (via {})", tunnel.url, tunnel.jump)
            }
        }
    }
}

/// First reconnection delay, doubled after each failed attempt.
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Give up on a connection that stays silent this long. A peer can vanish
/// without a TCP reset (a Tailscale node going to sleep, for instance) and
/// a plain read would then block forever. Keep this generous, a quiet
/// receiver in the middle of the night may legitimately see no traffic for
/// a while.
const READ_TIMEOUT: Duration = Duration::from_secs(300);
/// TCP keepalive settings for plain TCP feeds. The kernel starts probing
/// after [`KEEPALIVE_TIME`] of silence and gives up after a handful of
/// unanswered probes, which detects a dead peer much faster than
/// [`READ_TIMEOUT`].
const KEEPALIVE_TIME: Duration = Duration::from_secs(15);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);

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
                match timeout(READ_TIMEOUT, tcp_stream.read(&mut buffer)).await {
                    Ok(Ok(0)) => {
                        info!("Connection closed by peer");
                        break;
                    }
                    Ok(Ok(n)) => n,
                    Ok(Err(e)) => {
                        error!("Error reading from socket: {}", e);
                        break;
                    }
                    Err(_) => {
                        warn!("No data received for {READ_TIMEOUT:?}, assuming the connection is dead");
                        break;
                    }
                }
            }
            DataSource::Udp(udp_socket) => {
                match timeout(READ_TIMEOUT, udp_socket.recv_from(&mut buffer))
                    .await
                {
                    Ok(Ok((n, _))) => n,
                    Ok(Err(e)) => {
                        error!("Error reading from socket: {}", e);
                        break;
                    }
                    // A UDP socket never reports a dead sender. Dropping it
                    // after a long silence sends us back through `connect`,
                    // which also gives TCP another chance when the UDP socket
                    // was only a fallback.
                    Err(_) => {
                        warn!("No data received for {READ_TIMEOUT:?}, re-opening the socket");
                        break;
                    }
                }
            }
            DataSource::Websocket(ws_receive) => {
                match timeout(READ_TIMEOUT, ws_receive.next()).await {
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        debug!("Received {:?}", data);
                        let len = data.len().min(buffer.len());
                        buffer[..len].copy_from_slice(&data[..len]);
                        len
                    }
                    // pings, pongs and text frames carry no Beast data
                    Ok(Some(Ok(_))) => 0,
                    Ok(Some(Err(e))) => {
                        error!("Error reading from websocket: {}", e);
                        break;
                    }
                    Ok(None) => {
                        info!("Websocket closed by peer");
                        break;
                    }
                    Err(_) => {
                        warn!("No data received for {READ_TIMEOUT:?}, assuming the connection is dead");
                        break;
                    }
                }
            }
            #[cfg(feature = "ssh")]
            DataSource::TunnelledTcp(tunnel_rx) => {
                match timeout(READ_TIMEOUT, tunnel_rx.recv()).await {
                    Ok(Ok(Some(makiko::TunnelEvent::Data(data)))) => {
                        debug!("Received {:?}", data);
                        let len = data.len().min(buffer.len());
                        buffer[..len].copy_from_slice(&data[..len]);
                        len
                    }
                    Ok(_) => {
                        error!("Error reading from tunnel");
                        break;
                    }
                    Err(_) => {
                        warn!("No data received for {READ_TIMEOUT:?}, assuming the tunnel is dead");
                        break;
                    }
                }
            }
            #[cfg(feature = "ssh")]
            DataSource::TunnelledWebSocket(tunnel_rx) => {
                match timeout(READ_TIMEOUT, tunnel_rx.next()).await {
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        debug!("Received {:?}", data);
                        let len = data.len().min(buffer.len());
                        buffer[..len].copy_from_slice(&data[..len]);
                        len
                    }
                    // pings, pongs and text frames carry no Beast data
                    Ok(Some(Ok(_))) => 0,
                    Ok(_) => {
                        error!("Error reading from tunnel");
                        break;
                    }
                    Err(_) => {
                        warn!("No data received for {READ_TIMEOUT:?}, assuming the tunnel is dead");
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

/// Open one connection, without retrying.
async fn connect(address: &BeastSource) -> io::Result<DataSource> {
    let source = match address {
        BeastSource::Tcp(address) => match TcpStream::connect(address).await {
            Ok(stream) => {
                info!("Connected to TCP stream: {}", address);
                let keepalive = TcpKeepalive::new()
                    .with_time(KEEPALIVE_TIME)
                    .with_interval(KEEPALIVE_INTERVAL);
                if let Err(e) =
                    SockRef::from(&stream).set_tcp_keepalive(&keepalive)
                {
                    warn!("Failed to enable TCP keepalive: {}", e);
                }
                DataSource::Tcp(stream)
            }
            Err(error) => {
                info!(
                    "Failed to connect to TCP {} ({}), trying in UDP",
                    address,
                    error.to_string()
                );
                DataSource::Udp(UdpSocket::bind(address).await?)
            }
        },
        BeastSource::Udp(address) => {
            DataSource::Udp(UdpSocket::bind(address).await?)
        }
        BeastSource::Websocket(address) => {
            info!("Connecting to websocket: {}", address);
            let (stream, _) =
                connect_async(address).await.map_err(io::Error::other)?;
            info!("Connected to websocket: {}", address);
            let (_, rx) = stream.split();
            DataSource::Websocket(rx)
        }
        #[cfg(feature = "ssh")]
        BeastSource::TunnelledTcp(tunnel) => {
            let tunnel_rx = tunnel.connect().await.map_err(io::Error::other)?;
            DataSource::TunnelledTcp(tunnel_rx)
        }
        #[cfg(feature = "ssh")]
        BeastSource::TunnelledWebsocket(tunnel) => {
            let stream = tunnel.connect().await.map_err(io::Error::other)?;
            let url = tunnel
                .url
                .clone()
                .into_client_request()
                .map_err(io::Error::other)?;

            // Now perform the handshake using client_async, which accepts an already connected stream.
            let (ws_stream, _) =
                client_async(url, stream).await.map_err(io::Error::other)?;

            let (_, rx) = ws_stream.split();
            DataSource::TunnelledWebSocket(rx)
        }
    };
    Ok(source)
}

/// Receive Beast messages from `address` and forward them to `tx`.
///
/// Whenever the connection cannot be opened, gets closed by the peer, or
/// stays silent for five minutes, it is opened again after an exponential
/// backoff capped at 30 seconds. The function only returns once the
/// receiving end of `tx` has been dropped.
pub async fn receiver(
    address: BeastSource,
    tx: mpsc::Sender<TimedMessage>,
    serial: u64,
    name: Option<String>,
) -> io::Result<()> {
    let mut backoff = INITIAL_BACKOFF;

    loop {
        let source = match connect(&address).await {
            Ok(source) => source,
            Err(error) => {
                warn!(
                    "Failed to connect to {address} ({error}), retrying in {backoff:?}"
                );
                sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                continue;
            }
        };

        let msg_stream = next_msg(source).await;
        pin_mut!(msg_stream);
        let mut received_any = false;
        while let Some(msg) = msg_stream.next().await {
            if !received_any {
                // Only reset the backoff once the new connection has
                // actually delivered something
                received_any = true;
                backoff = INITIAL_BACKOFF;
            }
            let tmsg = process_radarcape(&msg, serial, name.clone());
            info!("Received {}", tmsg);
            if tx.send(tmsg).await.is_err() {
                // The consumer is gone, so there is no point reconnecting
                return Ok(());
            }
        }

        warn!("Connection to {address} lost, reconnecting in {backoff:?}");
        sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    /// A 23-byte Mode-S long frame whose last byte is `marker`, so the test
    /// can tell which connection a message came from.
    fn long_frame(marker: u8) -> Vec<u8> {
        let mut msg = vec![0x1a, 0x33];
        msg.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // MLAT timestamp
        msg.push(0x80); // signal level
        msg.extend_from_slice(&[0x8d; 13]); // 14-byte frame
        msg.push(marker);
        msg
    }

    #[tokio::test]
    async fn reconnects_after_peer_closes_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();

        // Serve two connections in a row, one frame each, then hang up
        let server = tokio::spawn(async move {
            for marker in [1u8, 2u8] {
                let (mut socket, _) = listener.accept().await.unwrap();
                socket.write_all(&long_frame(marker)).await.unwrap();
                socket.shutdown().await.unwrap();
                drop(socket);
            }
        });

        let (tx, mut rx) = mpsc::channel(16);
        let client =
            tokio::spawn(receiver(BeastSource::Tcp(address), tx, 0, None));

        let first = timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("first message before timeout")
            .expect("first message");
        assert_eq!(first.frame.last(), Some(&1));

        // The server only sends one frame per connection, so a second
        // message proves the client reconnected
        let second = timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("second message before timeout")
            .expect("second message");
        assert_eq!(second.frame.last(), Some(&2));

        server.await.unwrap();
        client.abort();
    }
}
