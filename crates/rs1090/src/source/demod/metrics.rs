use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Aggregated demodulation counters for one processed chunk.
#[derive(Debug, Clone, Default)]
pub struct DemodChunkStats {
    pub preambles_detected: u64,
    pub messages_valid: u64,
    pub messages_invalid: u64,
    pub preamble_corr_sum: f64,
    pub preamble_corr_count: u64,
    pub pulse_snr_db_sum: f64,
    pub pulse_snr_count: u64,
}

impl DemodChunkStats {
    pub fn record_preamble(&mut self, corr: f64, pulse_snr_db: Option<f64>) {
        self.preambles_detected += 1;
        self.preamble_corr_sum += corr;
        self.preamble_corr_count += 1;
        if let Some(v) = pulse_snr_db {
            self.pulse_snr_db_sum += v;
            self.pulse_snr_count += 1;
        }
    }

    pub fn record_valid(&mut self) {
        self.messages_valid += 1;
    }

    pub fn record_invalid(&mut self) {
        self.messages_invalid += 1;
    }
}

/// Demodulation quality metrics suitable for telemetry/export.
#[derive(Debug, Clone, Default)]
pub struct DemodMetrics {
    pub pulse_snr_db: f64,
    pub preamble_corr: f64,
    pub crc_rate: f64,
    pub message_rate: f64,
    pub timestamp: f64,
}

/// Streaming calculator producing periodic `DemodMetrics` snapshots.
#[derive(Debug)]
pub struct DemodMetricsTracker {
    start: Instant,
    emit_every: Duration,
    preambles_detected: u64,
    messages_valid: u64,
    preamble_corr_sum: f64,
    preamble_corr_count: u64,
    pulse_snr_db_sum: f64,
    pulse_snr_count: u64,
    last_snapshot: Option<DemodMetrics>,
}

impl Default for DemodMetricsTracker {
    fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }
}

impl DemodMetricsTracker {
    pub fn new(emit_every: Duration) -> Self {
        Self {
            start: Instant::now(),
            emit_every,
            preambles_detected: 0,
            messages_valid: 0,
            preamble_corr_sum: 0.0,
            preamble_corr_count: 0,
            pulse_snr_db_sum: 0.0,
            pulse_snr_count: 0,
            last_snapshot: None,
        }
    }

    pub fn update(&mut self, stats: &DemodChunkStats) -> Option<DemodMetrics> {
        self.preambles_detected += stats.preambles_detected;
        self.messages_valid += stats.messages_valid;
        self.preamble_corr_sum += stats.preamble_corr_sum;
        self.preamble_corr_count += stats.preamble_corr_count;
        self.pulse_snr_db_sum += stats.pulse_snr_db_sum;
        self.pulse_snr_count += stats.pulse_snr_count;

        if self.start.elapsed() < self.emit_every {
            return None;
        }

        let elapsed = self.start.elapsed().as_secs_f64().max(1e-6);
        let crc_rate = if self.preambles_detected > 0 {
            self.messages_valid as f64 / self.preambles_detected as f64
        } else {
            0.0
        };
        let message_rate = self.messages_valid as f64 / elapsed;
        let preamble_corr = if self.preamble_corr_count > 0 {
            self.preamble_corr_sum / self.preamble_corr_count as f64
        } else {
            0.0
        };
        let pulse_snr_db = if self.pulse_snr_count > 0 {
            self.pulse_snr_db_sum / self.pulse_snr_count as f64
        } else {
            0.0
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        let snapshot = DemodMetrics {
            pulse_snr_db,
            preamble_corr,
            crc_rate,
            message_rate,
            timestamp,
        };

        self.last_snapshot = Some(snapshot.clone());
        self.start = Instant::now();
        self.preambles_detected = 0;
        self.messages_valid = 0;
        self.preamble_corr_sum = 0.0;
        self.preamble_corr_count = 0;
        self.pulse_snr_db_sum = 0.0;
        self.pulse_snr_count = 0;

        Some(snapshot)
    }

    pub fn last_snapshot(&self) -> Option<&DemodMetrics> {
        self.last_snapshot.as_ref()
    }
}
