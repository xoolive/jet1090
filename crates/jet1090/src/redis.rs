//! Main-loop-owned Redis batches. No output task or additional queue.
use std::sync::Arc;
use std::time::Duration;

use ::redis::{aio::MultiplexedConnection, Client};
use tokio::sync::watch;
use tokio::time::{sleep, sleep_until, timeout, Instant};
use tracing::warn;

pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const FINISH_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_MESSAGES: usize = 16;
const MAX_BYTES: usize = 64 * 1024;
const COLLECT_FOR: Duration = Duration::from_millis(5);

pub(crate) struct Batch {
    client: Client,
    connection: Option<MultiplexedConnection>,
    topic: String,
    retry: Duration,
    messages: Vec<Arc<String>>,
    bytes: usize,
    deadline: Option<Instant>,
}
impl Batch {
    pub(crate) fn new(
        client: Client,
        connection: MultiplexedConnection,
        topic: String,
        retry: Duration,
    ) -> Self {
        Self {
            client,
            connection: Some(connection),
            topic,
            retry,
            messages: Vec::with_capacity(MAX_MESSAGES),
            bytes: 0,
            deadline: None,
        }
    }

    pub(crate) fn needs_flush_before(&self, payload: &str) -> bool {
        !self.messages.is_empty()
            && (self.full()
                || payload.len() > MAX_BYTES - self.bytes
                || self.deadline.is_some_and(|d| d <= Instant::now()))
    }

    pub(crate) fn push(&mut self, payload: Arc<String>) {
        // An oversized payload is accepted whole, as a singleton.
        assert!(
            !self.full()
                && (self.messages.is_empty()
                    || payload.len() <= MAX_BYTES - self.bytes)
        );
        self.deadline
            .get_or_insert_with(|| Instant::now() + COLLECT_FOR);
        self.bytes += payload.len();
        self.messages.push(payload);
    }

    fn full(&self) -> bool {
        self.messages.len() >= MAX_MESSAGES || self.bytes >= MAX_BYTES
    }

    /// Selected before new input so busy and idle feeds both respect the deadline.
    pub(crate) async fn ready(&self) {
        if self.full() {
            return;
        }
        match self.deadline {
            Some(deadline) => sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    }

    pub(crate) async fn flush_or_quit(
        &mut self,
        quit: &mut watch::Receiver<bool>,
    ) -> bool {
        if *quit.borrow() {
            return false;
        }
        tokio::select! {
            biased;
            _ = quit.changed() => {
                warn!("Redis flush cancelled during shutdown; {} pending messages may have been delivered; final flush may repeat a prefix", self.messages.len());
                false
            },
            _ = self.flush() => true,
        }
    }

    async fn publish(&mut self) -> Result<(), (String, bool)> {
        // Taking the connection also invalidates it if this future is cancelled.
        // A cancelled query may already have executed; do not silently reuse it.
        let mut connection = match self.connection.take() {
            Some(connection) => connection,
            None => timeout(
                CONNECT_TIMEOUT,
                self.client.get_multiplexed_async_connection(),
            )
            .await
            .map_err(|_| ("Redis reconnect timed out".to_string(), false))?
            .map_err(|error| (error.to_string(), false))?,
        };
        let mut pipeline = ::redis::pipe();
        for payload in &self.messages {
            pipeline
                .cmd("PUBLISH")
                .arg(&self.topic)
                .arg(payload.as_str());
        }
        let replies = pipeline
            .query_async::<Vec<i64>>(&mut connection)
            .await
            .map_err(|error| (error.to_string(), true))?;
        if replies.len() != self.messages.len() {
            return Err((
                "Unexpected Redis batch reply count".to_string(),
                true,
            ));
        }
        self.connection = Some(connection);
        Ok(())
    }

    async fn flush(&mut self) {
        if self.messages.is_empty() {
            return;
        }
        loop {
            match self.publish().await {
                Ok(()) => break,
                Err((description, uncertain)) => {
                    if self.retry.is_zero() {
                        warn!(error = %description, delivery_uncertain = uncertain,
                            "Redis publication failed; retries disabled; skipping {} unacknowledged messages", self.messages.len());
                        break;
                    }
                    if uncertain {
                        warn!(error = %description,
                            "Redis batch delivery uncertain; retrying {} messages in {}s; an already-delivered prefix may repeat", self.messages.len(), self.retry.as_secs_f64());
                    } else {
                        warn!(error = %description,
                            "Redis connection unavailable; retrying {} messages in {}s", self.messages.len(), self.retry.as_secs_f64());
                    }
                    sleep(self.retry).await;
                }
            }
        }
        self.messages.clear();
        self.bytes = 0;
        self.deadline = None;
    }

    pub(crate) async fn finish(mut self) -> Result<(), String> {
        timeout(FINISH_TIMEOUT, self.flush()).await.map_err(|_| {
            format!("Timed out flushing Redis; {} messages remain unacknowledged and may have been delivered", self.messages.len())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io, sync::Mutex};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::task::JoinHandle;
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::prelude::*;

    fn payload(value: usize) -> Arc<String> {
        Arc::new(value.to_string())
    }
    fn empty() -> Batch {
        Batch {
            client: Client::open("redis://127.0.0.1:1/").unwrap(),
            connection: None,
            topic: "test-only".into(),
            retry: Duration::ZERO,
            messages: Vec::new(),
            bytes: 0,
            deadline: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn first_item_deadline_survives_later_input_and_idle() {
        let mut batch = empty();
        assert!(timeout(COLLECT_FOR, batch.ready()).await.is_err());
        batch.finish().await.unwrap();
        batch = empty();
        batch.push(payload(0));
        let deadline = batch.deadline.unwrap();
        sleep(Duration::from_millis(3)).await;
        batch.push(payload(1));
        assert_eq!(batch.deadline, Some(deadline));
        batch.ready().await;
        assert_eq!(Instant::now(), deadline);
        assert!(batch.needs_flush_before("2"));
    }

    #[tokio::test(start_paused = true)]
    async fn count_and_byte_limits_include_oversize_singletons() {
        let mut batch = empty();
        for i in 0..MAX_MESSAGES {
            batch.push(payload(i));
        }
        assert!(batch.needs_flush_before("next"));
        assert!(timeout(Duration::ZERO, batch.ready()).await.is_ok());
        batch = empty();
        batch.push(Arc::new("x".repeat(MAX_BYTES - 2)));
        assert!(batch.needs_flush_before("xxx"));
        assert!(!batch.needs_flush_before("xx"));
        batch.push(Arc::new("xx".into()));
        assert!(batch.full());
        batch = empty();
        let large = Arc::new("x".repeat(MAX_BYTES + 1));
        batch.push(large.clone());
        assert_eq!(batch.messages, [large]);
        assert!(batch.needs_flush_before(""));
        assert!(timeout(Duration::ZERO, batch.ready()).await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn due_batch_is_selected_before_continuously_ready_input() {
        let mut batch = empty();
        batch.push(payload(0));
        sleep(COLLECT_FOR).await;
        let flush = tokio::select! {
            biased;
            _ = batch.ready() => true,
            _ = std::future::ready(()) => false,
        };
        assert!(flush);
    }

    // Ephemeral loopback RESP fixture, never the user's Redis. One server task
    // handles reconnects serially and is aborted on drop, including stalled I/O.
    struct Server {
        task: JoinHandle<()>,
        client: Client,
        seen: Arc<Mutex<Vec<Arc<String>>>>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Server {
        async fn batch(&self, retry: Duration) -> Batch {
            Batch::new(
                self.client.clone(),
                self.client
                    .get_multiplexed_async_connection()
                    .await
                    .unwrap(),
                "test-only".into(),
                retry,
            )
        }
    }
    async fn command(
        reader: &mut BufReader<TcpStream>,
    ) -> io::Result<Option<Vec<Vec<u8>>>> {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        let count = line
            .strip_prefix('*')
            .and_then(|s| s.trim().parse::<usize>().ok())
            .ok_or_else(|| io::Error::other("invalid mock array"))?;
        let mut args = Vec::with_capacity(count);
        for _ in 0..count {
            line.clear();
            reader.read_line(&mut line).await?;
            let len = line
                .strip_prefix('$')
                .and_then(|s| s.trim().parse::<usize>().ok())
                .ok_or_else(|| io::Error::other("invalid mock bulk"))?;
            let mut data = vec![0; len];
            reader.read_exact(&mut data).await?;
            let mut ending = [0; 2];
            reader.read_exact(&mut ending).await?;
            if ending != *b"\r\n" {
                return Err(io::Error::other("invalid mock ending"));
            }
            args.push(data);
        }
        Ok(Some(args))
    }
    async fn serve(
        stream: TcpStream,
        seen: &Mutex<Vec<Arc<String>>>,
        disconnect: Option<usize>,
        delay: Duration,
    ) -> io::Result<()> {
        stream.set_nodelay(true)?;
        let mut reader = BufReader::new(stream);
        let mut publications = 0;
        while let Some(args) = command(&mut reader).await? {
            if args.first().is_some_and(|arg| arg == b"PUBLISH") {
                assert_eq!(args.len(), 3);
                assert_eq!(args[1], b"test-only");
                seen.lock().unwrap().push(Arc::new(
                    String::from_utf8(args[2].clone()).unwrap(),
                ));
                publications += 1;
                if disconnect == Some(publications) {
                    return Ok(());
                }
                sleep(delay).await;
                reader.get_mut().write_all(b":1\r\n").await?;
            } else {
                reader.get_mut().write_all(b"+OK\r\n").await?;
            }
        }
        Ok(())
    }
    async fn server(
        disconnect_first_after: Option<usize>,
        first_delay: Duration,
    ) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::open(format!(
            "redis://{}/",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let shared = seen.clone();
        let task = tokio::spawn(async move {
            let mut first = true;
            while let Ok((stream, _)) = listener.accept().await {
                let (disconnect, delay) = if first {
                    (disconnect_first_after, first_delay)
                } else {
                    (None, Duration::ZERO)
                };
                first = false;
                let _ = serve(stream, &shared, disconnect, delay).await;
            }
        });
        Server { task, client, seen }
    }

    #[tokio::test]
    async fn actual_pipeline_preserves_fifo_bytes_and_shutdown_tail() {
        let server = server(None, Duration::ZERO).await;
        let mut batch = server.batch(Duration::ZERO).await;
        let expected: Vec<_> = (0..35)
            .map(payload)
            .chain([
                Arc::new("{\"timestamp\":1000.5}".into()),
                Arc::new("中文\r\n\0".into()),
                Arc::new("x".repeat(MAX_BYTES + 1)),
                payload(99),
            ])
            .collect();
        for message in &expected {
            if batch.needs_flush_before(message) {
                batch.flush().await;
            }
            batch.push(message.clone());
            assert!(batch.messages.len() <= MAX_MESSAGES);
            assert!(batch.bytes <= MAX_BYTES || batch.messages.len() == 1);
        }
        batch.finish().await.unwrap();
        assert_eq!(*server.seen.lock().unwrap(), expected);
    }

    #[tokio::test]
    async fn idle_deadline_flushes_low_rate_input() {
        let server = server(None, Duration::ZERO).await;
        let mut batch = server.batch(Duration::ZERO).await;
        batch.push(payload(1));
        let deadline = batch.deadline.unwrap();
        batch.ready().await;
        assert!(Instant::now() >= deadline);
        batch.flush().await;
        assert!(batch.messages.is_empty());
        assert_eq!(*server.seen.lock().unwrap(), [payload(1)]);
    }

    #[tokio::test]
    async fn reconnect_retry_warns_about_replayed_prefix() {
        let server = server(Some(2), Duration::ZERO).await;
        let mut batch = server.batch(Duration::from_millis(1)).await;
        for i in 0..4 {
            batch.push(payload(i));
        }
        let health = crate::health::new_health();
        let subscriber = tracing_subscriber::registry()
            .with(crate::health::UiEventLayer::new(health.clone()));
        batch.finish().with_subscriber(subscriber).await.unwrap();
        assert_eq!(
            *server.seen.lock().unwrap(),
            [0, 1, 0, 1, 2, 3]
                .into_iter()
                .map(payload)
                .collect::<Vec<_>>()
        );
        assert!(health.lock().unwrap().events.iter().any(|e| e
            .message
            .contains("already-delivered prefix may repeat")));
    }

    #[tokio::test]
    async fn retries_disabled_warn_and_skip_pending_batch() {
        let server = server(Some(1), Duration::ZERO).await;
        let mut batch = server.batch(Duration::ZERO).await;
        for i in 0..4 {
            batch.push(payload(i));
        }
        let health = crate::health::new_health();
        let subscriber = tracing_subscriber::registry()
            .with(crate::health::UiEventLayer::new(health.clone()));
        batch.flush().with_subscriber(subscriber).await;
        assert!(batch.messages.is_empty());
        assert!(health
            .lock()
            .unwrap()
            .events
            .iter()
            .any(|e| e.message.contains("skipping 4 unacknowledged")));
        batch.push(payload(99));
        batch.finish().await.unwrap();
        assert_eq!(*server.seen.lock().unwrap(), [payload(0), payload(99)]);
    }

    #[tokio::test]
    async fn connection_failure_is_known_not_sent_and_retains_batch() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut batch = empty();
        batch.client = Client::open(format!(
            "redis://{}/",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        drop(listener);
        batch.push(payload(1));
        let (_, uncertain) = batch.publish().await.unwrap_err();
        assert!(!uncertain);
        assert_eq!(batch.messages, [payload(1)]);
    }

    #[tokio::test]
    async fn quit_interrupts_reconnect_backoff() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut batch = empty();
        batch.client = Client::open(format!(
            "redis://{}/",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        drop(listener);
        batch.retry = Duration::from_secs(3600);
        batch.push(payload(1));
        let (quit, mut receiver) = watch::channel(false);
        tokio::spawn(async move {
            sleep(Duration::from_millis(10)).await;
            quit.send(true).unwrap();
        });
        assert!(!timeout(
            Duration::from_secs(1),
            batch.flush_or_quit(&mut receiver)
        )
        .await
        .unwrap());
        assert_eq!(batch.messages, [payload(1)]);
        batch.retry = Duration::ZERO;
        batch.finish().await.unwrap();
    }

    async fn quit_after_publication(server: &Server) -> watch::Receiver<bool> {
        let (quit, receiver) = watch::channel(false);
        let seen = server.seen.clone();
        tokio::spawn(async move {
            timeout(Duration::from_secs(1), async {
                while seen.lock().unwrap().is_empty() {
                    sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
            quit.send(true).unwrap();
        });
        receiver
    }

    #[tokio::test]
    async fn quit_cancels_ack_wait_and_final_flush_warns_before_replay() {
        let server = server(None, Duration::from_millis(30)).await;
        let mut batch = server.batch(Duration::from_millis(1)).await;
        batch.push(payload(1));
        let mut quit = quit_after_publication(&server).await;
        let health = crate::health::new_health();
        let subscriber = tracing_subscriber::registry()
            .with(crate::health::UiEventLayer::new(health.clone()));
        assert!(
            !batch
                .flush_or_quit(&mut quit)
                .with_subscriber(subscriber)
                .await
        );
        assert!(batch.connection.is_none());
        assert_eq!(batch.messages, [payload(1)]);
        assert!(health
            .lock()
            .unwrap()
            .events
            .iter()
            .any(|e| e.message.contains("final flush may repeat a prefix")));
        batch.finish().await.unwrap();
        assert_eq!(*server.seen.lock().unwrap(), [payload(1), payload(1)]);
    }

    #[tokio::test]
    async fn shutdown_has_a_bounded_flush_and_reports_pending_count() {
        let server = server(None, Duration::from_secs(30)).await;
        let mut batch = server.batch(Duration::from_millis(1)).await;
        batch.push(payload(1));
        let mut quit = quit_after_publication(&server).await;
        assert!(!batch.flush_or_quit(&mut quit).await);
        let start = Instant::now();
        let error = batch.finish().await.unwrap_err();
        assert!(start.elapsed() < FINISH_TIMEOUT + Duration::from_secs(2));
        assert!(error.contains(
            "1 messages remain unacknowledged and may have been delivered"
        ));
    }

    #[tokio::test]
    async fn already_requested_quit_leaves_accepted_tail_for_finish() {
        let server = server(None, Duration::ZERO).await;
        let mut batch = server.batch(Duration::ZERO).await;
        batch.push(payload(1));
        let (_quit, mut receiver) = watch::channel(true);
        assert!(!batch.flush_or_quit(&mut receiver).await);
        assert!(server.seen.lock().unwrap().is_empty());
        batch.finish().await.unwrap();
        assert_eq!(*server.seen.lock().unwrap(), [payload(1)]);
    }
}
