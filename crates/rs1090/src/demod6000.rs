//! 6 MS/s ADS-B Demodulator
//!
//! Native 6.0 MS/s energy/correlation-based demodulator designed for Airspy Mini.
//! Provides superior weak-signal recovery and collision tolerance compared to 2.4 MS/s.
//!
//! # Design Principles
//!
//! - **No decimation**: Process at native 6 MS/s for maximum collision tolerance
//! - **Energy-based detection**: More robust than threshold-based approaches
//! - **Integer math**: Use u32/u64 for magnitude², avoid float in hot path
//! - **Single-pass pipeline**: No reprocessing or backtracking
//!
//! # Performance Targets
//!
//! - CPU: <1 core @ 6 MS/s
//! - Latency: <2 ms
//! - Throughput: ≥1000 msg/s
//! - Message gain vs 2.4 MS/s: +15-30%
//!
//! # Signal Processing Pipeline
//!
//! ```text
//! IQ (i16) → Magnitude² (u32) → DC removal → Energy stream (u64)
//!          → Preamble correlation → Bit decoding → CRC validation
//! ```
//!
//! # References
//!
//! - Design spec: `analysis/demod6000.md`
//! - RadarCape-inspired approach
//! - ICAO Annex 10 Vol IV (Mode S specifications)

use crate::decode::crc::modes_checksum;
use crate::source::iqread::{getbits, icao_filter_add, icao_filter_test};

/// ICAO filter flag for non-transponder ADS-B
const ICAO_FILTER_ADSB_NT: u32 = 1 << 25;

/// Mode S message lengths
const MODES_SHORT_MSG_BITS: usize = 56; // 7 bytes
const MODES_LONG_MSG_BITS: usize = 112; // 14 bytes

/// Demodulator configuration
const SAMPLE_RATE: u32 = 6_000_000; // 6 MS/s
const SYMBOL_RATE: u32 = 1_000_000; // 1 Mbps (Mode S)
const SAMPLES_PER_BIT: usize = (SAMPLE_RATE / SYMBOL_RATE) as usize; // 6 samples/bit

/// Preamble timing (in microseconds) per DO-260B
/// Mode S preamble has 4 pulses at 0.0, 1.0, 3.5, 4.5 µs (each 0.5 µs wide)
/// Data bits start at 8.0 µs
/// Preamble pulse positions in samples @ 6 MS/s
/// 0.0µs=0, 1.0µs=6, 3.5µs=21, 4.5µs=27
/// Each pulse is 0.5µs = 3 samples wide
const PREAMBLE_TAPS_POSITIVE: [usize; 4] = [0, 6, 21, 27];
/// Gaps (noise suppression) at 0.5µs=3, 1.5µs=9, 4.0µs=24
const PREAMBLE_TAPS_NEGATIVE: [usize; 3] = [3, 9, 24];

/// Preamble length in samples (from start of preamble to start of data)
/// Data starts at 8.0 µs = 48 samples
const PREAMBLE_LENGTH_SAMPLES: usize = 48;

/// Energy window size (samples to sum for energy calculation)
const ENERGY_WINDOW_SIZE: usize = 3;

/// DC blocker coefficient (α ≈ 0.995)
const DC_ALPHA: f32 = 0.995;

/// Minimum energy delta for bit decision confidence (increased for better noise rejection)
const MIN_BIT_ENERGY_DELTA: u64 = 10000;

// Re-export ModeSMessage from iqread module for compatibility
pub use crate::source::iqread::ModeSMessage;

/// Magnitude buffer with DC removal and energy calculation
struct MagnitudeProcessor {
    /// Previous sample for DC blocker
    prev_input: f32,
    /// Previous output for DC blocker
    prev_output: f32,
    /// Sliding window for energy calculation
    energy_window: [u64; ENERGY_WINDOW_SIZE],
    /// Current position in energy window
    window_pos: usize,
}

impl MagnitudeProcessor {
    fn new() -> Self {
        Self {
            prev_input: 0.0,
            prev_output: 0.0,
            energy_window: [0; ENERGY_WINDOW_SIZE],
            window_pos: 0,
        }
    }

    /// Process IQ sample: calculate magnitude², apply DC removal, return energy
    ///
    /// # Pipeline
    /// 1. Magnitude² = I² + Q²
    /// 2. DC removal: y[n] = x[n] - x[n-1] + α*y[n-1]
    /// 3. Energy window: E[n] = sum of last 3 magnitude values
    fn process_sample(&mut self, i: i16, q: i16) -> u64 {
        // 1. Calculate magnitude squared (avoid sqrt for performance)
        let i_f32 = i as f32;
        let q_f32 = q as f32;
        let mag_sqr = i_f32 * i_f32 + q_f32 * q_f32;

        // 2. DC removal (IIR high-pass filter)
        let dc_removed =
            mag_sqr - self.prev_input + DC_ALPHA * self.prev_output;
        self.prev_input = mag_sqr;
        self.prev_output = dc_removed;

        // Ensure non-negative (DC removal can temporarily go negative)
        let mag_value = dc_removed.max(0.0) as u64;

        // 3. Update sliding energy window
        self.energy_window[self.window_pos] = mag_value;
        self.window_pos = (self.window_pos + 1) % ENERGY_WINDOW_SIZE;

        // 4. Return sum of energy window (E[n] = mag[n] + mag[n-1] + mag[n-2])
        self.energy_window.iter().sum()
    }
}

/// Preamble detector using correlation
struct PreambleDetector {
    /// Adaptive threshold for preamble detection
    threshold: u64,
    /// Running noise floor estimate
    noise_floor: u64,
    /// Sample count for noise floor update
    sample_count: usize,
}

impl PreambleDetector {
    fn new() -> Self {
        Self {
            threshold: 10_000_000, // Initial threshold (will adapt) - higher for real SDR
            noise_floor: 100_000,  // Higher initial noise floor estimate
            sample_count: 0,
        }
    }

    /// Check for preamble at current position
    ///
    /// Returns correlation score if preamble detected, None otherwise
    fn detect(&mut self, energy_buffer: &[u64], pos: usize) -> Option<u64> {
        // Need at least preamble length available
        if pos + PREAMBLE_LENGTH_SAMPLES > energy_buffer.len() {
            return None;
        }

        // Calculate correlation score
        // score = sum(positive_taps) - sum(negative_taps)
        let mut score: i64 = 0;

        for &tap in &PREAMBLE_TAPS_POSITIVE {
            score += energy_buffer[pos + tap] as i64;
        }

        for &tap in &PREAMBLE_TAPS_NEGATIVE {
            score -= energy_buffer[pos + tap] as i64;
        }

        // Update noise floor estimate (simple running average)
        self.sample_count += 1;
        if self.sample_count.is_multiple_of(1000) {
            let current_energy = energy_buffer[pos];
            self.noise_floor = (self.noise_floor * 99 + current_energy) / 100;
            // Use 50x multiplier instead of 10x for better noise rejection
            self.threshold = self.noise_floor * 50;
        }

        // Check if score exceeds threshold
        if score > 0 && score as u64 > self.threshold {
            // Require ALL 4 positive taps to be strong (stricter validation)
            let strong_taps = PREAMBLE_TAPS_POSITIVE
                .iter()
                .filter(|&&tap| energy_buffer[pos + tap] > self.noise_floor * 2)
                .count();

            if strong_taps >= 4 {
                Some(score as u64)
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Bit decoder for 6 samples per bit
struct BitDecoder;

impl BitDecoder {
    /// Decode a single bit from energy samples
    ///
    /// # Algorithm
    /// - Split 6-sample bit window into two halves: [0,1,2] and [3,4,5]
    /// - E0 = sum(samples[0..3])
    /// - E1 = sum(samples[3..6])
    /// - If E0 > E1: bit = 1 (PPM first half)
    /// - If E1 > E0: bit = 0 (PPM second half)
    ///
    /// Returns (bit_value, confidence)
    fn decode_bit(energy_samples: &[u64]) -> (u8, bool) {
        debug_assert!(energy_samples.len() >= SAMPLES_PER_BIT);

        // Energy in first half (samples 0, 1, 2)
        let e0: u64 = energy_samples[0] + energy_samples[1] + energy_samples[2];

        // Energy in second half (samples 3, 4, 5)
        let e1: u64 = energy_samples[3] + energy_samples[4] + energy_samples[5];

        // Bit decision (PPM: pulse in first half = 1, second half = 0)
        let bit = if e0 > e1 { 1 } else { 0 };

        // Confidence based on energy difference
        let delta = e0.abs_diff(e1);
        let confident = delta > MIN_BIT_ENERGY_DELTA;

        (bit, confident)
    }

    /// Decode message bits starting from position
    ///
    /// Returns (message_bytes, total_signal_power, weak_bit_count)
    fn decode_message(
        energy_buffer: &[u64],
        start_pos: usize,
        num_bits: usize,
    ) -> Option<(Vec<u8>, u64, usize)> {
        let num_bytes = num_bits / 8;
        let mut message = vec![0u8; num_bytes];
        let mut total_signal_power: u64 = 0;
        let mut weak_bit_count = 0;

        for bit_idx in 0..num_bits {
            let sample_start = start_pos + bit_idx * SAMPLES_PER_BIT;

            // Check buffer bounds
            if sample_start + SAMPLES_PER_BIT > energy_buffer.len() {
                return None;
            }

            let bit_samples =
                &energy_buffer[sample_start..sample_start + SAMPLES_PER_BIT];
            let (bit_value, confident) = Self::decode_bit(bit_samples);

            if !confident {
                weak_bit_count += 1;
            }

            // Accumulate signal power
            for &sample in bit_samples {
                total_signal_power += sample;
            }

            // Store bit in message
            let byte_idx = bit_idx / 8;
            let bit_pos = 7 - (bit_idx % 8); // MSB first
            message[byte_idx] |= bit_value << bit_pos;
        }

        Some((message, total_signal_power, weak_bit_count))
    }
}

/// Demodulate IQ samples at 6 MS/s and extract Mode S messages
///
/// # Arguments
/// * `iq_samples` - Interleaved I/Q samples as i16 pairs [I0, Q0, I1, Q1, ...]
///
/// # Returns
/// Vector of successfully decoded Mode S messages with CRC validation
pub fn demodulate6000(iq_samples: &[i16]) -> Vec<ModeSMessage> {
    let mut results = Vec::new();

    // Convert IQ to energy stream
    let mut processor = MagnitudeProcessor::new();
    let mut energy_buffer = Vec::with_capacity(iq_samples.len() / 2);

    // Process IQ pairs → energy
    for chunk in iq_samples.chunks_exact(2) {
        let i = chunk[0];
        let q = chunk[1];
        let energy = processor.process_sample(i, q);
        energy_buffer.push(energy);
    }

    // Diagnostic logging every 100 buffers
    static CALL_COUNTER: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);
    let call_count =
        CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if call_count % 100 == 1 {
        let avg_energy = energy_buffer.iter().sum::<u64>() as f64
            / energy_buffer.len() as f64;
        let max_energy = *energy_buffer.iter().max().unwrap_or(&0);
        let min_energy = *energy_buffer.iter().min().unwrap_or(&0);

        // Calculate percentiles for better understanding of distribution
        let mut sorted = energy_buffer.clone();
        sorted.sort_unstable();
        let p50 = sorted[sorted.len() / 2];
        let p95 = sorted[sorted.len() * 95 / 100];
        let p99 = sorted[sorted.len() * 99 / 100];

        tracing::info!(
            "Demod6000 call #{}: {} energy samples, avg={:.1}, min={}, p50={}, p95={}, p99={}, max={}",
            call_count,
            energy_buffer.len(),
            avg_energy,
            min_energy,
            p50,
            p95,
            p99,
            max_energy
        );
    }

    // Search for preambles and decode messages
    let mut detector = PreambleDetector::new();
    let mut pos = 0;
    let mut preambles_found = 0;

    while pos < energy_buffer.len() {
        if let Some(correlation_score) = detector.detect(&energy_buffer, pos) {
            preambles_found += 1;
            // Found preamble! Try to decode message
            let msg_start = pos + PREAMBLE_LENGTH_SAMPLES;

            // Try short message (56 bits = 7 bytes)
            if let Some((msg, signal_power, _weak_bits)) =
                BitDecoder::decode_message(
                    &energy_buffer,
                    msg_start,
                    MODES_SHORT_MSG_BITS,
                )
            {
                // Reject all-zero messages (noise)
                if msg.iter().all(|&b| b == 0x00) {
                    pos += SAMPLES_PER_BIT;
                    continue;
                }

                // Validate with CRC
                if let Ok(crc) = modes_checksum(&msg, MODES_SHORT_MSG_BITS) {
                    if is_valid_short_message(&msg, crc) {
                        let signal_level = signal_power as f64
                            / (MODES_SHORT_MSG_BITS * SAMPLES_PER_BIT) as f64
                            / 65535.0
                            / 65535.0;

                        // Convert Vec<u8> to [u8; 14] (pad with zeros for short messages)
                        let mut msg_array = [0u8; 14];
                        msg_array[..msg.len()].copy_from_slice(&msg);

                        results.push(ModeSMessage {
                            msg: msg_array,
                            signal_level,
                            score: correlation_score as i32,
                            sample_position: pos,
                        });

                        // Skip ahead to avoid re-detecting same message
                        pos += PREAMBLE_LENGTH_SAMPLES
                            + MODES_SHORT_MSG_BITS * SAMPLES_PER_BIT;
                        continue;
                    }
                }
            }

            // Try long message (112 bits = 14 bytes)
            if let Some((msg, signal_power, _weak_bits)) =
                BitDecoder::decode_message(
                    &energy_buffer,
                    msg_start,
                    MODES_LONG_MSG_BITS,
                )
            {
                // Reject all-zero messages (noise)
                if msg.iter().all(|&b| b == 0x00) {
                    pos += SAMPLES_PER_BIT;
                    continue;
                }

                // Validate with CRC
                if let Ok(crc) = modes_checksum(&msg, MODES_LONG_MSG_BITS) {
                    if is_valid_long_message(&msg, crc) {
                        let signal_level = signal_power as f64
                            / (MODES_LONG_MSG_BITS * SAMPLES_PER_BIT) as f64
                            / 65535.0
                            / 65535.0;

                        // Convert Vec<u8> to [u8; 14]
                        let mut msg_array = [0u8; 14];
                        msg_array.copy_from_slice(&msg);

                        results.push(ModeSMessage {
                            msg: msg_array,
                            signal_level,
                            score: correlation_score as i32,
                            sample_position: pos,
                        });

                        // Skip ahead to avoid re-detecting same message
                        pos += PREAMBLE_LENGTH_SAMPLES
                            + MODES_LONG_MSG_BITS * SAMPLES_PER_BIT;
                        continue;
                    }
                }
            }

            // Preamble detected but no valid message, skip ahead a bit
            pos += SAMPLES_PER_BIT;
        } else {
            pos += 1;
        }
    }

    if call_count % 100 == 1 {
        tracing::info!(
            "Demod6000 call #{}: found {} preambles, {} valid messages (threshold={}, noise_floor={})",
            call_count,
            preambles_found,
            results.len(),
            detector.threshold,
            detector.noise_floor
        );
    }

    results
}

/// Check if ICAO address is in a plausible allocated range
/// Rejects obvious noise like 000000, FFFFFF, and unallocated high ranges
fn is_plausible_icao(addr: u32) -> bool {
    // Reject all zeros
    if addr == 0x000000 {
        return false;
    }

    // Reject all ones
    if addr == 0xffffff {
        return false;
    }

    // Reject high unallocated ranges (> 0xd00000 is mostly unallocated)
    // Most allocated ranges are below 0xc00000
    if addr > 0xd00000 {
        return false;
    }

    true
}

/// Validate short Mode S message (DF 0, 4, 5, 11)
fn is_valid_short_message(msg: &[u8], crc: u32) -> bool {
    if msg.is_empty() {
        return false;
    }

    let df = msg[0] >> 3; // Downlink Format in bits 1-5

    match df {
        0 | 4 | 5 => {
            // Short air-air surveillance / altitude reply
            // CRC encodes ICAO address - must be in filter and plausible
            is_plausible_icao(crc) && icao_filter_test(crc)
        }
        11 => {
            // All-call reply
            let iid = crc & 0x7f;
            let crc = crc & 0x00ff_ff80;
            let addr = getbits(msg, 9, 32) as u32;

            // Check if ICAO is plausible first
            if !is_plausible_icao(addr) {
                return false;
            }

            match (crc, iid, icao_filter_test(addr)) {
                (0, 0, true) => true, // Known ICAO, perfect match
                (0, 0, false) => {
                    // New ICAO, add to filter
                    icao_filter_add(addr);
                    true
                }
                (0, _, true) => true, // Known ICAO with IID
                _ => false,
            }
        }
        _ => false,
    }
}

/// Validate long Mode S message (DF 16, 17, 18, 20, 21, 24-31)
fn is_valid_long_message(msg: &[u8], crc: u32) -> bool {
    if msg.is_empty() {
        return false;
    }

    let df = msg[0] >> 3; // Downlink Format

    match df {
        17 | 18 => {
            // Extended squitter (ADS-B)
            if crc != 0 {
                return false; // Must have perfect CRC for ADS-B
            }

            let addr = getbits(msg, 9, 32) as u32;

            // Check if ICAO is plausible
            if !is_plausible_icao(addr) {
                return false;
            }

            if icao_filter_test(addr) {
                true // Known ICAO
            } else {
                // New ICAO address, add to filter
                if df == 17 {
                    icao_filter_add(addr);
                } else {
                    // DF 18: Non-transponder (mark with flag)
                    icao_filter_add(addr | ICAO_FILTER_ADSB_NT);
                }
                true
            }
        }
        16 | 20 | 21 => {
            // Comm-B messages: CRC encodes ICAO address
            is_plausible_icao(crc) && icao_filter_test(crc)
        }
        24..=31 => {
            // Comm-D messages: CRC encodes ICAO address
            is_plausible_icao(crc) && icao_filter_test(crc)
        }
        _ => false,
    }
}
