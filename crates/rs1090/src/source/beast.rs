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
use tracing::{debug, error, info, trace, warn};

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

// Evidence only: a grid is not proof of GPS validity or rounding semantics.
const TIMESTAMP_GRID_NS: u64 = 1 << 27;
#[derive(Default)]
struct TimestampGrid {
    first_second: Option<u64>,
    tags: std::collections::HashSet<u64>,
    bins: u8,
    span_seconds: u64,
    confirmed: bool,
}
impl TimestampGrid {
    fn observe(&mut self, raw: u64, valid_gps: bool) -> Option<bool> {
        let seconds = raw >> 30;
        let fraction = raw & ((1 << 30) - 1);
        if !valid_gps
            || seconds >= 86400
            || fraction >= 1_000_000_000
            || !fraction.is_multiple_of(TIMESTAMP_GRID_NS)
        {
            let was_confirmed = self.confirmed;
            *self = Self::default();
            return was_confirmed.then_some(false);
        }
        // A replayed/nonconsecutive tag is not independent evidence. Retain
        // only the first 256 distinct tags: the observer has a fixed memory cap.
        if self.tags.len() < 256 {
            self.tags.insert(raw);
        }
        self.bins |= 1 << (fraction / TIMESTAMP_GRID_NS);
        let first = *self.first_second.get_or_insert(seconds);
        let span = (seconds + 86400 - first) % 86400;
        if span < 43200 {
            self.span_seconds = self.span_seconds.max(span);
        }
        if !self.confirmed
            && self.tags.len() >= 256
            && self.bins == u8::MAX
            && self.span_seconds >= 5
        {
            self.confirmed = true;
            return Some(true);
        }
        None
    }
}

// Leave partial escaped frames intact until complete. Preserve the previous
// reader's acceptance of literal marker bytes; discard only unframed noise.
fn extract_beast(data: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        let Some(start) = data.iter().position(|byte| *byte == 0x1a) else {
            data.clear();
            return None;
        };
        data.drain(..start);
        if data.len() < 2 {
            return None;
        }
        let size = match data[1] {
            0x31 => 11,
            0x32 => 16,
            0x33 | 0x34 => 23,
            _ => {
                data.remove(0);
                continue;
            }
        };
        let mut frame = Vec::with_capacity(size);
        frame.extend_from_slice(&data[..2]);
        let mut cursor = 2;
        while frame.len() < size {
            let byte = *data.get(cursor)?;
            if byte == 0x1a {
                let next = *data.get(cursor + 1)?;
                if next == 0x1a {
                    cursor += 1;
                }
            }
            frame.push(byte);
            cursor += 1;
        }
        data.drain(..cursor);
        return Some(frame);
    }
}

pub async fn next_msg(mut stream: DataSource) -> impl Stream<Item = Vec<u8>> {
    let mut data = Vec::new();
    // A UDP read consumes the datagram; a small buffer permanently loses its tail.
    let mut buffer = vec![0u8; u16::MAX as usize];
    stream! {
    loop {
        let bytes_read = match &mut stream {
            DataSource::Tcp(tcp_stream) => {
                match timeout(READ_TIMEOUT, tcp_stream.read(&mut buffer[..1024])).await {
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
                    Ok(Some(Ok(Message::Binary(chunk)))) => {
                        debug!("Received {:?}", chunk);
                        data.extend_from_slice(&chunk);
                        0
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
                    Ok(Ok(Some(makiko::TunnelEvent::Data(chunk)))) => {
                        debug!("Received {:?}", chunk);
                        data.extend_from_slice(&chunk);
                        0
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
                    Ok(Some(Ok(Message::Binary(chunk)))) => {
                        debug!("Received {:?}", chunk);
                        data.extend_from_slice(&chunk);
                        0
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

        while let Some(msg) = extract_beast(&mut data) {
            if msg[1] != 0x34 { yield msg; }
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

/// Connection lifecycle events emitted by [`receiver`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionStatus {
    Connecting,
    Healthy,
    Reconnecting,
}

/// Receive Beast messages from `address` and forward them to `tx`.
///
/// Whenever the connection cannot be opened, gets closed by the peer, or
/// stays silent for five minutes, it is opened again after an exponential
/// backoff capped at 30 seconds. The function only returns once the
/// receiving end of `tx` has been dropped.
pub async fn receiver<F>(
    address: BeastSource,
    tx: mpsc::Sender<TimedMessage>,
    serial: u64,
    name: Option<String>,
    on_status: F,
) -> io::Result<()>
where
    F: Fn(ConnectionStatus),
{
    receiver_with_timing_callback(address, tx, serial, name, on_status, |_| {})
        .await
}

/// Receive Beast data with the same per-connection raw-grid evidence used by
/// diagnostics. `on_grid(false)` resets evidence on connection changes; true
/// describes resolution only, never truncation, accuracy or GPS eligibility.
pub async fn receiver_with_timing_callback<F, G>(
    address: BeastSource,
    tx: mpsc::Sender<TimedMessage>,
    serial: u64,
    name: Option<String>,
    on_status: F,
    on_grid: G,
) -> io::Result<()>
where
    F: Fn(ConnectionStatus),
    G: Fn(bool),
{
    let mut backoff = INITIAL_BACKOFF;

    on_status(ConnectionStatus::Connecting);
    loop {
        on_grid(false);
        let source = match connect(&address).await {
            Ok(source) => {
                on_status(ConnectionStatus::Healthy);
                source
            }
            Err(error) => {
                on_status(ConnectionStatus::Reconnecting);
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
        let mut timestamp_grid = TimestampGrid::default();
        while let Some(msg) = msg_stream.next().await {
            if !received_any {
                // Only reset the backoff once the new connection has
                // actually delivered something
                received_any = true;
                backoff = INITIAL_BACKOFF;
            }
            let tmsg = process_radarcape(&msg, serial, name.clone());
            if let Some(meta) = tmsg.metadata.first() {
                if let Some(raw) = meta.nanoseconds {
                    if let Some(confirmed) = timestamp_grid
                        .observe(raw, meta.gnss_timestamp.is_some())
                    {
                        on_grid(confirmed);
                        info!(target: "rs1090::source::beast::timing", source = serial,
                            confirmed, step_ns = TIMESTAMP_GRID_NS,
                            samples = timestamp_grid.tags.len(), span_seconds = timestamp_grid.span_seconds,
                            "Raw GPS timestamp grid evidence changed; rounding and clock accuracy unverified");
                    }
                }
            }
            trace!("Received {}", tmsg);
            if tx.send(tmsg).await.is_err() {
                // The consumer is gone, so there is no point reconnecting
                return Ok(());
            }
        }

        on_grid(false);
        on_status(ConnectionStatus::Reconnecting);
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

    fn tag(second: u64, bin: u64) -> u64 {
        (second << 30) | (bin * TIMESTAMP_GRID_NS)
    }

    #[test]
    fn grid_requires_samples_bins_and_span_then_revokes() {
        let mut grid = TimestampGrid::default();
        for _ in 0..1000 {
            assert_eq!(grid.observe(tag(0, 0), true), None);
        }
        assert_eq!(grid.tags.len(), 1);
        for i in 1..256 {
            grid.observe(tag(i / 8, i % 8), true);
        }
        assert!(grid.confirmed);
        assert_eq!(grid.observe(tag(33, 0) + 1, true), Some(false));
        assert!(!grid.confirmed);
        for i in 0..255 {
            assert_eq!(grid.observe(tag(i / 8, i % 8), true), None);
        }
        assert_eq!(grid.observe(tag(31, 7), true), Some(true));
        assert_eq!(grid.observe(tag(32, 0), false), Some(false));
    }

    #[test]
    fn grid_does_not_confirm_fine_tags_or_single_bin_or_short_span() {
        let mut fine = TimestampGrid::default();
        let mut one_bin = TimestampGrid::default();
        let mut short = TimestampGrid::default();
        for i in 0..1000 {
            fine.observe(tag(i / 8, i % 8) + i % 17, true);
            one_bin.observe(tag(i, 0), true);
            short.observe(tag(0, i % 8), true);
        }
        assert!(!fine.confirmed && !one_bin.confirmed && !short.confirmed);
        let mut replay = TimestampGrid::default();
        for _ in 0..1000 {
            for bin in 0..8 {
                replay.observe(tag(0, bin), true);
            }
            replay.observe(tag(5, 0), true);
        }
        assert_eq!(replay.tags.len(), 9);
        assert!(!replay.confirmed);
    }

    #[test]
    fn grid_handles_midnight_without_counting_backwards_as_a_day() {
        let mut grid = TimestampGrid::default();
        grid.observe(tag(100, 0), true);
        grid.observe(tag(99, 1), true);
        assert_eq!(grid.span_seconds, 0);
        let mut grid = TimestampGrid::default();
        for i in 0..256 {
            grid.observe(tag((86398 + i / 8) % 86400, i % 8), true);
        }
        assert!(grid.confirmed);
        assert!(grid.span_seconds < 60);
        assert!(!TimestampGrid::default().confirmed); // reconnect starts fresh
    }

    #[test]
    fn framing_accepts_short_frames_and_fragmented_escapes() {
        let mut short = vec![0x1a, 0x32];
        short.extend_from_slice(&[0; 14]);
        assert_eq!(extract_beast(&mut short.clone()), Some(short));
        let mut ac = vec![0x1a, 0x31];
        ac.extend_from_slice(&[0; 9]);
        assert_eq!(extract_beast(&mut ac.clone()), Some(ac));
        let mut expected = long_frame(1);
        expected[7] = 0x1a;
        expected[15] = 0x1a;
        let mut escaped = expected[..2].to_vec();
        for byte in &expected[2..] {
            escaped.push(*byte);
            if *byte == 0x1a {
                escaped.push(*byte);
            }
        }
        for split in 1..escaped.len() {
            let mut buffer = escaped[..split].to_vec();
            assert!(extract_beast(&mut buffer).is_none());
            assert_eq!(buffer, escaped[..split]);
            buffer.extend_from_slice(&escaped[split..]);
            assert_eq!(extract_beast(&mut buffer), Some(expected.clone()));
            assert!(buffer.is_empty());
        }
    }

    #[test]
    fn framing_skips_invalid_prefixes_and_does_not_keep_unframed_noise() {
        let mut noise = vec![0; 100_000];
        assert!(extract_beast(&mut noise).is_none());
        assert!(noise.is_empty());
        let expected = long_frame(1);
        let mut data = vec![0x1a, 0x99, 0, 0];
        data.extend_from_slice(&expected);
        assert_eq!(extract_beast(&mut data), Some(expected));
        assert!(data.is_empty());
    }

    #[test]
    fn framing_keeps_legacy_single_marker_bytes_in_clock_and_payload() {
        let mut expected = long_frame(1);
        expected[4] = 0x1a;
        expected[5] = 0x33;
        expected[12] = 0x1a;
        let mut buffer = expected.clone();
        assert_eq!(extract_beast(&mut buffer), Some(expected));
        assert!(buffer.is_empty());
    }

    #[tokio::test]
    async fn websocket_preserves_binary_messages_larger_than_scratch_buffer() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expected = long_frame(1);
        let payload = expected.repeat(96);
        let server = tokio::spawn(async move {
            use futures_util::SinkExt;
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            ws.send(Message::Binary(payload.into())).await.unwrap();
            ws.close(None).await.unwrap();
        });
        let (ws, _) = connect_async(format!("ws://{address}")).await.unwrap();
        let (_, read) = ws.split();
        let stream = next_msg(DataSource::Websocket(read)).await;
        pin_mut!(stream);
        let frames: Vec<_> = stream.take(96).collect().await;
        assert_eq!(frames, vec![expected; 96]);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn udp_preserves_datagrams_larger_than_scratch_buffer() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let expected = long_frame(1);
        sender
            .send_to(&expected.repeat(96), socket.local_addr().unwrap())
            .await
            .unwrap();
        let stream = next_msg(DataSource::Udp(socket)).await;
        pin_mut!(stream);
        let frames: Vec<_> =
            timeout(Duration::from_secs(1), stream.take(96).collect())
                .await
                .expect("complete datagram must be parsed");
        assert_eq!(frames, vec![expected; 96]);
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
        let client = tokio::spawn(receiver(
            BeastSource::Tcp(address),
            tx,
            0,
            None,
            |_| {},
        ));

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
