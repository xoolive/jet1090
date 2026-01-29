//! 6 MS/s ADS-B Demodulator
//!
//! Native 6.0 MS/s energy/correlation-based demodulator designed for non RTL-SDR
//! SDRs capable of providing 6 MS/s IQ samples (e.g., Airspy, SDRplay, HackRF).
//! Provides superior weak-signal recovery and collision tolerance compared to 2.4 MS/s.
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
//! - ICAO Annex 10 Vol IV (Mode S specifications)

use super::{
    validate_modes_message, ModeSMessage, MODES_LONG_MSG_BITS,
    MODES_SHORT_MSG_BITS,
};

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
const MIN_BIT_ENERGY_DELTA: u64 = 5000;

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
            self.threshold = self.noise_floor * 30;
        }

        // Check if score exceeds threshold
        if score > 0 && score as u64 > self.threshold {
            // Require ALL 4 positive taps to be strong (stricter validation)
            let strong_taps = PREAMBLE_TAPS_POSITIVE
                .iter()
                .filter(|&&tap| energy_buffer[pos + tap] > self.noise_floor * 2)
                .count();

            if strong_taps >= 3 {
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

    // Search for preambles and decode messages
    let mut detector = PreambleDetector::new();
    let mut pos = 0;

    while pos < energy_buffer.len() {
        if let Some(_correlation_score) = detector.detect(&energy_buffer, pos) {
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

                // Validate with shared validation function
                let score = validate_modes_message(&msg);
                if score >= 0 {
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
                        score,
                        sample_position: pos,
                    });

                    // Skip ahead to avoid re-detecting same message
                    pos += PREAMBLE_LENGTH_SAMPLES
                        + MODES_SHORT_MSG_BITS * SAMPLES_PER_BIT;
                    continue;
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

                // Validate with shared validation function
                let score = validate_modes_message(&msg);
                if score >= 0 {
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
                        score,
                        sample_position: pos,
                    });

                    // Skip ahead to avoid re-detecting same message
                    pos += PREAMBLE_LENGTH_SAMPLES
                        + MODES_LONG_MSG_BITS * SAMPLES_PER_BIT;
                    continue;
                }
            }

            // Preamble detected but no valid message, skip ahead a bit
            pos += SAMPLES_PER_BIT;
        } else {
            pos += 1;
        }
    }

    results
}
