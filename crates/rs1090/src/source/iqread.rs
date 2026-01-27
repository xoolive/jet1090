use desperado::IqAsyncSource;
use tokio::sync::mpsc;

use crate::decode::time::now_in_ns;
use crate::prelude::*;
use crate::source::demod::demod2400::demodulate2400;
use crate::source::demod::demod6000::demodulate6000;
use crate::source::demod::{convert_f32_to_i16_iq, magnitude_u16};

pub async fn receiver(
    tx: mpsc::Sender<TimedMessage>,
    mut source: IqAsyncSource,
    serial: u64,
    rate: f64,
    name: Option<String>,
) {
    while let Some(buf) = source.next().await {
        if let Ok(buf) = buf {
            // Timestamp buffer arrival to calculate per-message reception times.
            // Ideally timestamping should occur at hardware read (in desperado), but this
            // approach adds only ~20-100μs delay vs 54ms error from timestamping after demod.
            let buffer_arrival_ns = now_in_ns();
            let buffer_arrival_time = buffer_arrival_ns as f64 * 1e-9;

            let resulting_data = match rate {
                2.4e6 => {
                    let mag_u16 = magnitude_u16(&buf);
                    demodulate2400(&mag_u16)
                }
                6.0e6 => {
                    let iq_i16 = convert_f32_to_i16_iq(&buf);
                    demodulate6000(&iq_i16)
                }
                _ => {
                    panic!(
                        "Unsupported sample rate: {} (supported: 2.4e6, 6.0e6)",
                        rate
                    );
                }
            };

            for data in resulting_data {
                // Calculate when this message was received based on its sample position.
                // Earlier samples (position 0) are older, so subtract offset from buffer arrival.
                // Note: Uses nominal sample rate; SDR clock drift (±1-2ppm) accumulates over time.
                let sample_offset_seconds = data.sample_position as f64 / rate;
                let system_timestamp =
                    buffer_arrival_time - sample_offset_seconds;

                let metadata = SensorMetadata {
                    system_timestamp,
                    gnss_timestamp: None,
                    nanoseconds: None,
                    rssi: Some(10. * data.signal_level.log10() as f32),
                    serial,
                    name: name.clone(),
                };
                let tmsg = TimedMessage {
                    timestamp: system_timestamp,
                    frame: data.msg.to_vec(),
                    message: None,
                    metadata: vec![metadata],
                    decode_time: None,
                };
                if tx.send(tmsg).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Receiver for IQ file sources that calculates timestamps based on sample position
/// and a base timestamp (typically file modification time), rather than using current system time.
///
/// This allows replaying IQ files with timestamps that reflect when the data was originally recorded,
/// preserving temporal relationships between messages for proper CPR decoding and analysis.
pub async fn file_receiver(
    tx: mpsc::Sender<TimedMessage>,
    mut source: IqAsyncSource,
    serial: u64,
    rate: f64,
    base_timestamp: f64,
    chunk_size: u64,
    name: Option<String>,
) {
    let mut sample_count: u64 = 0;

    while let Some(buf) = source.next().await {
        if let Ok(buf) = buf {
            let resulting_data = match rate {
                2.4e6 => {
                    let mag_u16 = magnitude_u16(&buf);
                    demodulate2400(&mag_u16)
                }
                6.0e6 => {
                    let iq_i16 = convert_f32_to_i16_iq(&buf);
                    demodulate6000(&iq_i16)
                }
                _ => {
                    panic!(
                        "Unsupported sample rate: {} (supported: 2.4e6, 6.0e6)",
                        rate
                    );
                }
            };
            for data in resulting_data {
                // Calculate timestamp based on sample position and file base time
                let sample_timestamp =
                    base_timestamp + (sample_count as f64 / rate);

                let metadata = SensorMetadata {
                    system_timestamp: sample_timestamp,
                    gnss_timestamp: None,
                    nanoseconds: None,
                    rssi: Some(10. * data.signal_level.log10() as f32),
                    serial,
                    name: name.clone(),
                };
                let tmsg = TimedMessage {
                    timestamp: sample_timestamp,
                    frame: data.msg.to_vec(),
                    message: None,
                    metadata: vec![metadata],
                    decode_time: None,
                };
                if tx.send(tmsg).await.is_err() {
                    break;
                }
            }

            // Increment sample count by the chunk size
            sample_count += chunk_size;
        }
    }
}
