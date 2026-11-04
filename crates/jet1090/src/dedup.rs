use rs1090::decode::DF;
use rs1090::prelude::*;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tokio::time::Instant;

const TIMING_TARGET: &str = "jet1090::dedup::timing";

/// Packed-Beast source IDs and current coarse-grid evidence, owned by ingress.
/// Absence denotes other clock formats, not a receiver-quality assessment.
pub type ClockGrids = Arc<Mutex<HashMap<u64, bool>>>;

const MAX_PENDING_GROUPS: usize = 4096;
const MAX_PENDING_RECEPTIONS: usize = 16384;
const MAX_GROUP_RECEPTIONS: usize = 64;
type Deadline = (Instant, u64);
struct Group {
    entries: Vec<TimedMessage>,
    earliest: f64,
    latest: f64,
}
impl Group {
    fn compatible(&self, msg: &TimedMessage, threshold: f64) -> bool {
        let span =
            self.latest.max(msg.timestamp) - self.earliest.min(msg.timestamp);
        span.is_finite()
            && msg.timestamp.is_finite()
            && span * 1000. < threshold
            && self.entries.len() < MAX_GROUP_RECEPTIONS
            && !msg.metadata.is_empty()
            && !self.entries.iter().any(|entry| {
                entry.metadata.is_empty()
                    || entry.metadata.iter().any(|old| {
                        msg.metadata.iter().any(|new| old.serial == new.serial)
                    })
            })
    }
}
#[derive(Default)]
struct Pending {
    groups: BTreeMap<Deadline, Group>,
    by_frame: HashMap<Vec<u8>, Vec<Deadline>>,
    next_id: u64,
    receptions: usize,
}
impl Pending {
    fn insert(&mut self, msg: TimedMessage, threshold: u32, collect: Duration) {
        // More than one compatible transmission is ambiguous: retain separately,
        // never force nearest-time pairing of repeated/static payloads.
        let candidates: Vec<_> = self
            .by_frame
            .get(&msg.frame)
            .into_iter()
            .flatten()
            .filter(|key| {
                self.groups[*key].compatible(&msg, f64::from(threshold))
            })
            .copied()
            .take(2)
            .collect();
        self.receptions += 1;
        if candidates.len() == 1 {
            let group =
                self.groups.get_mut(&candidates[0]).expect("indexed group");
            group.earliest = group.earliest.min(msg.timestamp);
            group.latest = group.latest.max(msg.timestamp);
            group.entries.push(msg);
        } else {
            let key = (Instant::now() + collect, self.next_id);
            self.next_id = self.next_id.wrapping_add(1);
            self.by_frame
                .entry(msg.frame.clone())
                .or_default()
                .push(key);
            self.groups.insert(
                key,
                Group {
                    earliest: msg.timestamp,
                    latest: msg.timestamp,
                    entries: vec![msg],
                },
            );
        }
    }
    fn pop(&mut self) -> Option<Vec<TimedMessage>> {
        let (key, group) = self.groups.pop_first()?;
        let frame = &group.entries[0].frame;
        let keys = self.by_frame.get_mut(frame).expect("indexed frame");
        keys.retain(|k| *k != key);
        if keys.is_empty() {
            self.by_frame.remove(frame);
        }
        self.receptions -= group.entries.len();
        Some(group.entries)
    }
}

/// Associate exact payloads only with a unique compatible active transmission.
/// Capture span must be strictly below `dedup_threshold`, on both sides.
/// Collection is monotonic and fixed at threshold + lookahead (zero is immediate).
/// A further bounded lookahead orders ready groups by their selected real times.
/// Idle/EOF flush and bounded-pressure flush preserve original receptions.
/// Stale feeds can still produce late output; no timestamps are clamped.
pub async fn deduplicate_messages(
    mut rx: mpsc::Receiver<TimedMessage>,
    tx: mpsc::Sender<TimedMessage>,
    dedup_threshold: u32,
    reorder_window: u32,
    clock_grids: ClockGrids,
) {
    let mut cache = Pending::default();
    let collect = if dedup_threshold == 0 {
        Duration::ZERO
    } else {
        Duration::from_millis(
            u64::from(dedup_threshold) + u64::from(reorder_window),
        )
    };
    let mut diagnostics =
        tracing::enabled!(target: TIMING_TARGET, tracing::Level::DEBUG)
            .then(Associations::new);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut eof = false;
    let reorder = if dedup_threshold == 0 {
        Duration::ZERO
    } else {
        Duration::from_millis(u64::from(reorder_window))
    };
    let mut emission: VecDeque<(Instant, TimedMessage)> = VecDeque::new();
    let mut last_pressure_warning = None;
    loop {
        let now = Instant::now();
        while let Some((key, _)) = cache.groups.first_key_value() {
            let pressure = cache.groups.len() >= MAX_PENDING_GROUPS
                || cache.receptions >= MAX_PENDING_RECEPTIONS;
            if !eof && key.0 > now && !pressure {
                break;
            }
            if pressure
                && last_pressure_warning.is_none_or(|last: Instant| {
                    now.duration_since(last) >= Duration::from_secs(1)
                })
            {
                tracing::warn!(groups = cache.groups.len(), receptions = cache.receptions, "Dedup capacity reached; flushing oldest group early without dropping receptions");
                last_pressure_warning = Some(now);
            }
            let entries = cache.pop().expect("pending group");
            let grids = clock_grids.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(msg) = decode_group_with_clocks(entries, &grids) {
                emission.push_back((now + reorder, msg));
            }
        }
        let force = eof || emission.len() >= MAX_PENDING_GROUPS;
        let cutoff = emission
            .iter()
            .take_while(|(deadline, _)| force || *deadline <= now)
            .map(|(_, msg)| msg.timestamp)
            .max_by(f64::total_cmp);
        let mut ready = Vec::new();
        if let Some(cutoff) = cutoff {
            let mut kept = VecDeque::new();
            for (deadline, msg) in emission.drain(..) {
                // Include younger ready groups whose capture points precede a due
                // group. Never emit the due point ahead of already-known older data.
                if force || msg.timestamp.total_cmp(&cutoff).is_le() {
                    ready.push(msg);
                } else {
                    kept.push_back((deadline, msg));
                }
            }
            emission = kept;
        }
        if !eof && force {
            tracing::warn!("Dedup reorder capacity reached; releasing buffered receptions early");
        }
        ready.sort_by(|a, b| {
            a.timestamp
                .total_cmp(&b.timestamp)
                .then_with(|| a.frame.cmp(&b.frame))
        });
        for msg in ready {
            if let Some(d) = &mut diagnostics {
                d.observe(&msg, Instant::now());
            }
            if tx.send(msg).await.is_err() {
                return;
            }
        }
        if eof {
            break;
        }
        let deadline = cache
            .groups
            .first_key_value()
            .map(|(key, _)| key.0)
            .into_iter()
            .chain(emission.front().map(|(deadline, _)| *deadline))
            .min();
        tokio::select! {
            biased;
            _ = tx.closed() => break,
            _ = tokio::time::sleep_until(deadline.unwrap_or(now + Duration::from_secs(86400))), if deadline.is_some() => {},
            _ = tick.tick(), if diagnostics.is_some() => {
                let d = diagnostics.as_mut().expect("diagnostics enabled");
                d.expire(Instant::now()); d.report(false);
            },
            input = rx.recv() => match input {
                Some(msg) => cache.insert(msg, dedup_threshold, collect),
                None => eof = true,
            },
        }
    }
    if let Some(d) = &mut diagnostics {
        d.finish(Instant::now());
    }
}

#[cfg(test)]
fn decode_group(entries: Vec<TimedMessage>) -> Option<TimedMessage> {
    decode_group_with_clocks(entries, &HashMap::new())
}

fn decode_group_with_clocks(
    mut entries: Vec<TimedMessage>,
    grids: &HashMap<u64, bool>,
) -> Option<TimedMessage> {
    entries.sort_by(|a, b| a.timestamp.total_cmp(&b.timestamp));
    // Pick an actual finer reported GNSS reception, not a synthetic midpoint or
    // a receiver score. This applies only when a matched clock is confirmed coarse.
    let has_coarse = entries
        .iter()
        .flat_map(|m| &m.metadata)
        .any(|m| grids.get(&m.serial) == Some(&true));
    let finer = has_coarse
        .then(|| {
            entries
                .iter()
                .flat_map(|m| &m.metadata)
                .filter_map(|m| {
                    let timestamp =
                        m.gnss_timestamp.filter(|t| t.is_finite())?;
                    let raw = m.nanoseconds?;
                    // Ingress registers packed Beast sources; other GPS raw tags here
                    // are SeRo UTC-day nanoseconds. Native SDR without GPS is ineligible.
                    let fraction = if grids.contains_key(&m.serial) {
                        raw & ((1 << 30) - 1)
                    } else {
                        raw % 1_000_000_000
                    };
                    (fraction < 1_000_000_000
                        && !fraction.is_multiple_of(1 << 27))
                    .then_some(timestamp)
                })
                .min_by(f64::total_cmp)
        })
        .flatten();
    let metadata = entries
        .iter()
        .flat_map(|entry| entry.metadata.clone())
        .collect();
    let mut tmsg = entries.remove(0);
    tmsg.metadata = metadata;
    if let Some(timestamp) = finer {
        tmsg.timestamp = timestamp;
    }
    let start = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("SystemTime before unix epoch")
        .as_secs_f64();
    let (_, message) = Message::from_bytes((&tmsg.frame, 0)).ok()?;
    tmsg.decode_time = Some(
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("SystemTime before unix epoch")
            .as_secs_f64()
            - start,
    );
    tmsg.message = Some(message);
    Some(tmsg)
}

// Opt-in observations of DECODED output groups, not a clock correction or a new
// matching rule. Exact BDS05 bytes still do not prove a physical transmission ID.
const ASSOCIATION_WINDOW: Duration = Duration::from_secs(2);
const MAX_ASSOCIATIONS: usize = 2048;
const MAX_RECEIVERS: usize = 32;
const MAX_PAIRS: usize = 256;
struct Reception {
    capture: Option<f64>,
    observed: f64,
    group: u64,
    repeated: bool,
}
struct Association {
    started: Instant,
    receivers: HashMap<u64, Reception>,
    censored: bool,
}
#[derive(Default)]
struct PairStats {
    count: u64,
    split: u64,
    capture_sum_ms: f64,
    observed_sum_ms: f64,
    capture_min_ms: f64,
    capture_max_ms: f64,
}
struct Associations {
    pending: HashMap<Vec<u8>, Association>,
    pairs: HashMap<(u64, u64), PairStats>,
    groups: u64,
    ambiguous: u64,
    unpaired: u64,
    censored: u64,
    last_report: Instant,
}
impl Associations {
    fn new() -> Self {
        Self {
            pending: HashMap::new(),
            pairs: HashMap::new(),
            groups: 0,
            ambiguous: 0,
            unpaired: 0,
            censored: 0,
            last_report: Instant::now(),
        }
    }
    fn observe(&mut self, msg: &TimedMessage, now: Instant) {
        if msg.frame.len() != 14 {
            return;
        }
        let Some(Message {
            df: DF::ExtendedSquitterADSB(adsb),
            ..
        }) = &msg.message
        else {
            return;
        };
        if !matches!(adsb.message, ME::BDS05 { tc: 9..=18 | 20..=22, .. }) {
            return;
        }
        self.groups += 1;
        if self.pending.get(&msg.frame).is_some_and(|e| {
            now.duration_since(e.started) >= ASSOCIATION_WINDOW
        }) {
            let entry = self.pending.remove(&msg.frame).unwrap();
            self.collect(entry);
        }
        if !self.pending.contains_key(&msg.frame)
            && self.pending.len() >= MAX_ASSOCIATIONS
        {
            self.censored += 1;
            return;
        }
        let entry =
            self.pending.entry(msg.frame.clone()).or_insert_with(|| {
                Association {
                    started: now,
                    receivers: HashMap::new(),
                    censored: false,
                }
            });
        for meta in &msg.metadata {
            if let Some(previous) = entry.receivers.get_mut(&meta.serial) {
                if !previous.repeated {
                    self.ambiguous += 1;
                }
                previous.repeated = true;
            } else if entry.receivers.len() < MAX_RECEIVERS {
                entry.receivers.insert(
                    meta.serial,
                    Reception {
                        capture: meta.gnss_timestamp.filter(|t| t.is_finite()),
                        observed: meta.system_timestamp,
                        group: self.groups,
                        repeated: false,
                    },
                );
            } else {
                entry.censored = true;
            }
        }
    }
    fn collect(&mut self, entry: Association) {
        if entry.censored {
            self.censored += 1;
            return;
        }
        let mut receivers: Vec<_> = entry
            .receivers
            .into_iter()
            .filter(|(_, r)| {
                !r.repeated && r.capture.is_some() && r.observed.is_finite()
            })
            .collect();
        receivers.sort_by_key(|(id, _)| *id);
        if receivers.len() < 2 {
            self.unpaired += 1;
        }
        for (i, (a, x)) in receivers.iter().enumerate() {
            for (b, y) in &receivers[i + 1..] {
                if !self.pairs.contains_key(&(*a, *b))
                    && self.pairs.len() >= MAX_PAIRS
                {
                    self.censored += 1;
                    continue;
                }
                let stats = self.pairs.entry((*a, *b)).or_default();
                let delta = (x.capture.unwrap() - y.capture.unwrap()) * 1000.;
                if stats.count == 0 {
                    stats.capture_min_ms = delta;
                    stats.capture_max_ms = delta;
                }
                stats.count += 1;
                stats.split += u64::from(x.group != y.group);
                stats.capture_sum_ms += delta;
                stats.observed_sum_ms += (x.observed - y.observed) * 1000.;
                stats.capture_min_ms = stats.capture_min_ms.min(delta);
                stats.capture_max_ms = stats.capture_max_ms.max(delta);
            }
        }
    }
    fn expire(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, e)| {
                now.duration_since(e.started) >= ASSOCIATION_WINDOW
            })
            .map(|(frame, _)| frame.clone())
            .collect();
        for frame in expired {
            let entry = self.pending.remove(&frame).unwrap();
            self.collect(entry);
        }
    }
    fn report(&mut self, force: bool) {
        if !force && self.last_report.elapsed() < Duration::from_secs(30) {
            return;
        }
        self.last_report = Instant::now();
        tracing::debug!(target: TIMING_TARGET, groups = self.groups, ambiguous_receiver_entries = self.ambiguous,
            unpaired_windows = self.unpaired, censored = self.censored, pending = self.pending.len(), "BDS05 association evidence; report only, cumulative candidate counts");
        for ((a, b), stats) in &self.pairs {
            tracing::debug!(target: TIMING_TARGET, receiver_a = a, receiver_b = b, candidates = stats.count,
                same_group = stats.count - stats.split, split_groups = stats.split,
                mean_capture_a_minus_b_ms = stats.capture_sum_ms / stats.count as f64,
                mean_observation_a_minus_b_ms = stats.observed_sum_ms / stats.count as f64,
                min_capture_ms = stats.capture_min_ms, max_capture_ms = stats.capture_max_ms,
                "Unique-receiver BDS05 candidates; payload identity is not proof of the same transmission");
        }
    }
    fn finish(&mut self, now: Instant) {
        self.expire(now);
        // An incomplete diagnostic window cannot establish uniqueness.
        self.censored += self.pending.len() as u64;
        self.pending.clear();
        self.report(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reception(timestamp: f64, serial: u64) -> TimedMessage {
        TimedMessage {
            timestamp,
            frame: hex::decode("8d40621d58c382d690c8ac2863a7").unwrap(),
            message: None,
            metadata: vec![SensorMetadata {
                system_timestamp: timestamp + 0.01,
                gnss_timestamp: Some(timestamp),
                nanoseconds: None,
                rssi: None,
                serial,
                name: None,
            }],
            decode_time: None,
        }
    }
    #[test]
    fn late_repeated_and_ambiguous_transmissions_stay_separate() {
        let mut pending = Pending::default();
        for (time, serial) in [(1000., 1), (999., 2), (1000.1, 3)] {
            pending.insert(
                reception(time, serial),
                450,
                Duration::from_millis(650),
            );
        }
        assert_eq!(pending.groups.len(), 2);
        assert_eq!(
            pending
                .groups
                .values()
                .map(|g| g.entries.len())
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        let mut repeated = Pending::default();
        for (time, serial) in [(1000., 1), (1000.1, 1), (1000.05, 2)] {
            repeated.insert(
                reception(time, serial),
                450,
                Duration::from_millis(650),
            );
        }
        assert_eq!(repeated.groups.len(), 3); // two matches => ambiguous
        while repeated.pop().is_some() {}
        assert!(repeated.by_frame.is_empty());
        assert_eq!(repeated.receptions, 0);
    }
    #[test]
    fn selected_time_is_an_actual_finer_reception_not_an_offset() {
        let originals = || {
            let mut coarse = reception(1000., 1);
            coarse.metadata[0].nanoseconds = Some(1000 << 30);
            let mut precise = reception(1_000.131_388, 2);
            precise.metadata[0].nanoseconds = Some(1_000_131_388_000);
            vec![coarse, precise]
        };
        let chosen =
            decode_group_with_clocks(originals(), &HashMap::from([(1, true)]))
                .unwrap();
        assert_eq!(chosen.timestamp, 1_000.131_388);
        assert_eq!(chosen.metadata[0].gnss_timestamp, Some(1000.));
        assert_eq!(chosen.metadata[1].gnss_timestamp, Some(1_000.131_388));
        assert_eq!(
            decode_group_with_clocks(originals(), &HashMap::new())
                .unwrap()
                .timestamp,
            1000.
        );
    }
    #[tokio::test(start_paused = true)]
    async fn quiet_input_flushes_after_fixed_monotonic_collection() {
        let (sender, input) = mpsc::channel(4);
        let (sink, mut output) = mpsc::channel(4);
        let task = tokio::spawn(deduplicate_messages(
            input,
            sink,
            450,
            200,
            Default::default(),
        ));
        sender.send(reception(1000., 1)).await.unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(500)).await;
        sender.send(reception(1000.1, 2)).await.unwrap();
        tokio::task::yield_now().await;
        assert!(output.try_recv().is_err());
        tokio::time::advance(Duration::from_millis(150)).await;
        tokio::task::yield_now().await;
        assert!(output.try_recv().is_err());
        tokio::time::advance(Duration::from_millis(200)).await;
        let msg = output.recv().await.unwrap();
        assert_eq!(msg.metadata.len(), 2);
        assert_eq!(msg.timestamp, 1000.);
        // Joining did not renew the first-arrival deadline.
        drop(sender);
        task.await.unwrap();
    }
    #[tokio::test(start_paused = true)]
    async fn ordering_waits_for_known_older_capture_points() {
        let (sender, input) = mpsc::channel(4);
        let (sink, mut output) = mpsc::channel(4);
        let task = tokio::spawn(deduplicate_messages(
            input,
            sink,
            450,
            200,
            Default::default(),
        ));
        sender.send(reception(1000.13, 1)).await.unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(70)).await;
        let mut second = reception(1000., 2);
        second.frame = hex::decode("8D4840D6202CC371C32CE0576098").unwrap();
        sender.send(second).await.unwrap();
        tokio::task::yield_now().await;
        for millis in [580, 70] {
            tokio::time::advance(Duration::from_millis(millis)).await;
            tokio::task::yield_now().await;
            assert!(output.try_recv().is_err());
        }
        tokio::time::advance(Duration::from_millis(130)).await;
        assert_eq!(output.recv().await.unwrap().timestamp, 1000.);
        assert_eq!(output.recv().await.unwrap().timestamp, 1000.13);
        drop(sender);
        task.await.unwrap();
    }
    #[tokio::test]
    async fn capacity_flush_preserves_every_reception() {
        let (sender, input) = mpsc::channel(64);
        let (sink, mut output) = mpsc::channel(64);
        let task = tokio::spawn(deduplicate_messages(
            input,
            sink,
            450,
            200000,
            Default::default(),
        ));
        let producer = tokio::spawn(async move {
            for i in 0..MAX_PENDING_GROUPS + 3 {
                sender.send(reception(1000. + i as f64, 1)).await.unwrap();
            }
        });
        let mut count = 0;
        while output.recv().await.is_some() {
            count += 1;
        }
        producer.await.unwrap();
        task.await.unwrap();
        assert_eq!(count, MAX_PENDING_GROUPS + 3);
    }
    #[tokio::test]
    async fn eof_drains_pending_receptions_without_changing_timestamps() {
        for reorder in [0, 200] {
            let (tx_in, rx_in) = mpsc::channel(2);
            let (tx_out, mut rx_out) = mpsc::channel(2);
            let task = tokio::spawn(deduplicate_messages(
                rx_in,
                tx_out,
                450,
                reorder,
                Default::default(),
            ));
            tx_in.send(reception(1000., 1)).await.unwrap();
            tx_in.send(reception(1000.1, 2)).await.unwrap();
            drop(tx_in);
            task.await.unwrap();
            let msg = rx_out
                .recv()
                .await
                .expect("pending group must drain at EOF");
            assert_eq!(msg.timestamp, 1000.);
            assert_eq!(msg.metadata.len(), 2);
            assert!(msg.message.is_some());
            assert!(rx_out.recv().await.is_none());
        }
    }
    #[tokio::test]
    async fn downstream_closure_stops_even_when_input_is_idle() {
        let (_tx_in, rx_in) = mpsc::channel(1);
        let (tx_out, rx_out) = mpsc::channel(1);
        let task = tokio::spawn(deduplicate_messages(
            rx_in,
            tx_out,
            450,
            200,
            Default::default(),
        ));
        drop(rx_out);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("closed output must stop the task")
            .unwrap();
    }
    #[tokio::test]
    async fn identical_payloads_respect_dedup_expiry_boundary() {
        for (gap_ms, expected_groups) in [(250, 1), (450, 2), (1000, 2)] {
            let (tx_in, rx_in) = mpsc::channel(3);
            let (tx_out, mut rx_out) = mpsc::channel(3);
            let task = tokio::spawn(deduplicate_messages(
                rx_in,
                tx_out,
                450,
                0,
                Default::default(),
            ));
            let second_timestamp = (1_000_000 + gap_ms) as f64 / 1000.;
            tx_in.send(reception(1000., 1)).await.unwrap();
            tx_in.send(reception(second_timestamp, 2)).await.unwrap();
            let mut sentinel = reception(1002., 3);
            sentinel.frame = vec![0xff; 14]; // invalid DF, not a hidden pending output
            tx_in.send(sentinel).await.unwrap();
            drop(tx_in);
            task.await.unwrap();
            let mut groups = Vec::new();
            while let Some(msg) = rx_out.recv().await {
                groups.push(msg);
            }
            assert_eq!(groups.len(), expected_groups, "gap={gap_ms}ms");
            assert_eq!(groups[0].timestamp, 1000.);
            if expected_groups == 1 {
                assert_eq!(groups[0].metadata.len(), 2);
            } else {
                assert_eq!(groups[1].timestamp, second_timestamp);
                assert_eq!(groups[0].metadata[0].serial, 1);
                assert_eq!(groups[1].metadata[0].serial, 2);
            }
        }
    }
    #[tokio::test]
    async fn zero_dedup_threshold_emits_without_another_packet() {
        let (tx_in, rx_in) = mpsc::channel(1);
        let (tx_out, mut rx_out) = mpsc::channel(1);
        let task = tokio::spawn(deduplicate_messages(
            rx_in,
            tx_out,
            0,
            0,
            Default::default(),
        ));
        tx_in.send(reception(1000., 1)).await.unwrap();
        let message =
            tokio::time::timeout(Duration::from_secs(1), rx_out.recv())
                .await
                .expect("zero threshold must not wait for another input")
                .unwrap();
        assert_eq!(message.timestamp, 1000.);
        drop(tx_in);
        task.await.unwrap();
    }
    #[tokio::test]
    async fn late_packet_does_not_underflow_reordering_age() {
        let (tx_in, rx_in) = mpsc::channel(3);
        let (tx_out, mut rx_out) = mpsc::channel(3);
        let task = tokio::spawn(deduplicate_messages(
            rx_in,
            tx_out,
            450,
            2000,
            Default::default(),
        ));
        for (timestamp, frame) in [
            (1000., reception(1000., 1).frame),
            (1000.5, vec![0xff; 14]),
            (999., vec![0xfe; 14]),
        ] {
            let mut msg = reception(timestamp, 1);
            msg.frame = frame;
            msg.metadata.clear();
            tx_in.send(msg).await.unwrap();
        }
        drop(tx_in);
        task.await.unwrap();
        assert_eq!(rx_out.recv().await.unwrap().timestamp, 1000.);
    }
    #[tokio::test]
    async fn enabling_diagnostics_does_not_change_output() {
        use tracing::instrument::WithSubscriber;
        async fn output(enabled: bool) -> Vec<(f64, Vec<u64>)> {
            let (sender, input) = mpsc::channel(4);
            let (sink, mut output) = mpsc::channel(4);
            let subscriber = tracing_subscriber::fmt()
                .with_max_level(if enabled {
                    tracing::Level::DEBUG
                } else {
                    tracing::Level::WARN
                })
                .with_writer(std::io::sink)
                .finish();
            let task = tokio::spawn(
                deduplicate_messages(input, sink, 450, 200, Default::default())
                    .with_subscriber(subscriber),
            );
            for (time, source) in [(1000., 1), (1000.1, 2), (1001., 1)] {
                sender.send(reception(time, source)).await.unwrap();
            }
            drop(sender);
            task.await.unwrap();
            let mut result = Vec::new();
            while let Some(msg) = output.recv().await {
                result.push((
                    msg.timestamp,
                    msg.metadata.iter().map(|m| m.serial).collect(),
                ));
            }
            result
        }
        let baseline = output(false).await;
        assert_eq!(output(true).await, baseline);
        assert_eq!(baseline, vec![(1000., vec![1, 2]), (1001., vec![1])]);
    }

    #[tokio::test]
    async fn eof_sorts_all_pending_groups_with_the_existing_buffer() {
        let (sender, input) = mpsc::channel(3);
        let (sink, mut output) = mpsc::channel(3);
        let task = tokio::spawn(deduplicate_messages(
            input,
            sink,
            450,
            2000,
            Default::default(),
        ));
        for (time, serial) in [(1000., 1), (1000.5, 2), (999., 3)] {
            let mut msg = reception(time, serial);
            if serial == 2 {
                msg.frame =
                    hex::decode("8D4840D6202CC371C32CE0576098").unwrap();
            }
            sender.send(msg).await.unwrap();
        }
        drop(sender);
        task.await.unwrap();
        let mut times = Vec::new();
        while let Some(msg) = output.recv().await {
            times.push(msg.timestamp);
        }
        assert_eq!(times, vec![999., 1000., 1000.5]);
    }

    #[tokio::test(start_paused = true)]
    async fn associations_distinguish_merged_split_and_ambiguous_candidates() {
        let start = Instant::now();
        let mut d = Associations::new();
        let merged =
            decode_group(vec![reception(1000., 1), reception(1000.05, 2)])
                .unwrap();
        d.observe(&merged, start);
        d.expire(start + ASSOCIATION_WINDOW);
        assert_eq!(d.pairs[&(1, 2)].count, 1);
        assert_eq!(d.pairs[&(1, 2)].split, 0);
        d.observe(
            &decode_group(vec![reception(1001., 1)]).unwrap(),
            start + ASSOCIATION_WINDOW,
        );
        d.observe(
            &decode_group(vec![reception(1001.1, 2)]).unwrap(),
            start + ASSOCIATION_WINDOW + Duration::from_millis(500),
        );
        d.expire(start + ASSOCIATION_WINDOW * 2);
        assert_eq!(d.pairs[&(1, 2)].count, 2);
        assert_eq!(d.pairs[&(1, 2)].split, 1);
        d.observe(
            &decode_group(vec![
                reception(1002., 1),
                reception(1002.1, 1),
                reception(1002.05, 2),
            ])
            .unwrap(),
            start + ASSOCIATION_WINDOW * 2,
        );
        d.expire(start + ASSOCIATION_WINDOW * 3);
        assert_eq!(d.ambiguous, 1);
        assert_eq!(d.pairs[&(1, 2)].count, 2);
    }
    #[tokio::test(start_paused = true)]
    async fn associations_do_not_use_callsigns_or_incomplete_windows() {
        let start = Instant::now();
        let mut d = Associations::new();
        let mut callsign = reception(1000., 1);
        callsign.frame = hex::decode("8D4840D6202CC371C32CE0576098").unwrap();
        d.observe(&decode_group(vec![callsign]).unwrap(), start);
        assert!(d.pending.is_empty());
        d.observe(
            &decode_group(vec![reception(1000., 1), reception(1000.05, 2)])
                .unwrap(),
            start,
        );
        d.finish(start + Duration::from_millis(1));
        assert!(d.pairs.is_empty());
        assert_eq!(d.censored, 1);
    }
    #[tokio::test(start_paused = true)]
    async fn associations_bound_payload_receiver_and_pair_storage() {
        let start = Instant::now();
        let mut d = Associations::new();
        let mut msg = decode_group(vec![reception(1000., 1)]).unwrap();
        for i in 0..MAX_ASSOCIATIONS + 1 {
            msg.frame[..8].copy_from_slice(&(i as u64).to_be_bytes());
            d.observe(&msg, start);
        }
        assert_eq!(d.pending.len(), MAX_ASSOCIATIONS);
        assert_eq!(d.censored, 1);
        let mut d = Associations::new();
        msg.metadata = (0..MAX_RECEIVERS + 1)
            .map(|id| reception(1000., id as u64).metadata.remove(0))
            .collect();
        d.observe(&msg, start);
        d.expire(start + ASSOCIATION_WINDOW);
        assert!(d.pairs.is_empty());
        assert_eq!(d.censored, 1);
        let mut d = Associations::new();
        msg.metadata.truncate(MAX_RECEIVERS);
        d.observe(&msg, start);
        d.expire(start + ASSOCIATION_WINDOW);
        assert_eq!(d.pairs.len(), MAX_PAIRS);
        assert!(d.censored > 0);
    }
}
