use desperado::IqAsyncSource;
use tokio::sync::mpsc;

use crate::decode::time::now_in_ns;
use crate::prelude::*;
use crate::source::demod::demod2400::demodulate2400;
use crate::source::demod::demod6000::demodulate6000;
use crate::source::demod::{convert_f32_to_i16_iq, magnitude_u16};

pub async fn receiver(
    tx: mpsc::Sender<TimedMessage>,
    source: &mut IqAsyncSource,
    serial: u64,
    rate: f64,
    name: Option<String>,
) -> Result<(), desperado::Error> {
    while let Some(buf) = source.next().await {
        let buf = buf?;
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
                return Err(desperado::Error::other(format!(
                    "Unsupported sample rate: {rate} (supported: 2.4e6, 6.0e6)"
                )));
            }
        };

        for data in resulting_data {
            // Calculate when this message was received based on its sample position.
            // Earlier samples (position 0) are older, so subtract offset from buffer arrival.
            // Note: Uses nominal sample rate; SDR clock drift (±1-2ppm) accumulates over time.
            let sample_offset_seconds = data.sample_position as f64 / rate;
            let system_timestamp = buffer_arrival_time - sample_offset_seconds;

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
                return Ok(());
            }
        }
    }

    Err(desperado::Error::stream_terminated(
        "SDR",
        "reader ended unexpectedly",
    ))
}

/// Replay IQ samples relative to a capture start (or an approximate file-time
/// fallback), including each decoded preamble's position within its buffer.
/// Actual buffer lengths advance the sample counter. `chunk_size` is retained
/// for API compatibility but is not a reliable count for short buffers.
/// This does not change the timestamp interpretation of live SDR input.
/// Errors are logged for compatibility; callers needing an outcome should use
/// [`try_file_receiver`].
pub async fn file_receiver(
    tx: mpsc::Sender<TimedMessage>,
    source: IqAsyncSource,
    serial: u64,
    rate: f64,
    base_timestamp: f64,
    _chunk_size: u64,
    name: Option<String>,
) {
    if let Err(error) =
        try_file_receiver(tx, source, serial, rate, base_timestamp, name).await
    {
        tracing::error!(%error, "IQ file replay failed; stopping rather than skipping samples");
    }
}

/// Replay a finite IQ input, returning parameter/read errors to the caller.
/// Normal EOF and a closed downstream channel are successful completion.
/// Already sent messages remain valid if a subsequent read fails.
/// Unlike [`file_receiver`], this does not log and discard read errors.
pub async fn try_file_receiver(
    tx: mpsc::Sender<TimedMessage>,
    mut source: IqAsyncSource,
    serial: u64,
    rate: f64,
    base_timestamp: f64,
    name: Option<String>,
) -> Result<(), desperado::Error> {
    if !matches!(rate, 2.4e6 | 6.0e6) || !base_timestamp.is_finite() {
        return Err(desperado::Error::other(format!(
            "Invalid IQ file replay clock: rate={rate}, base_timestamp={base_timestamp}"
        )));
    }
    let mut sample_count: u64 = 0;
    loop {
        let buf = tokio::select! {
            biased;
            _ = tx.closed() => return Ok(()),
            buf = source.next() => buf,
        };
        let Some(buf) = buf else { return Ok(()) };
        let buf = buf?;
        let resulting_data = match rate {
            2.4e6 => demodulate2400(&magnitude_u16(&buf)),
            6.0e6 => demodulate6000(&convert_f32_to_i16_iq(&buf)),
            _ => unreachable!("validated sample rate"),
        };
        for data in resulting_data {
            let tmsg = file_message(
                data,
                serial,
                rate,
                base_timestamp,
                sample_count,
                name.clone(),
            );
            if tx.send(tmsg).await.is_err() {
                return Ok(());
            }
        }
        sample_count += buf.len() as u64;
    }
}

fn file_message(
    data: super::demod::ModeSMessage,
    serial: u64,
    rate: f64,
    base_timestamp: f64,
    sample_count: u64,
    name: Option<String>,
) -> TimedMessage {
    let timestamp = base_timestamp
        + (sample_count as f64 + data.sample_position as f64) / rate;
    TimedMessage {
        timestamp,
        frame: data.msg.to_vec(),
        message: None,
        metadata: vec![SensorMetadata {
            system_timestamp: timestamp,
            gnss_timestamp: None,
            nanoseconds: None,
            rssi: Some(10. * data.signal_level.log10() as f32),
            serial,
            name,
        }],
        decode_time: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::demod::ModeSMessage;

    fn decoded(position: usize) -> ModeSMessage {
        ModeSMessage {
            msg: [0x8d; 14],
            signal_level: 0.5,
            score: 0,
            sample_position: position,
        }
    }

    #[tokio::test]
    async fn file_replay_uses_actual_buffers_and_decodes_a_short_final_chunk() {
        use num_complex::Complex;
        let frame = [
            0x8d, 0x40, 0x62, 0x1d, 0x58, 0xc3, 0x82, 0xd6, 0x90, 0xc8, 0xac,
            0x28, 0x63, 0xa7,
        ];
        let mut samples = vec![Complex::new(0.0_f32, 0.0); 3400];
        for start in [200, 1200, 2500] {
            for offset in [0, 6, 21, 27] {
                for s in &mut samples[start + offset..start + offset + 3] {
                    s.re = 0.9;
                }
            }
            for bit in 0..112 {
                let half = if frame[bit / 8] & (1 << (7 - bit % 8)) != 0 {
                    0
                } else {
                    3
                };
                let offset = start + 48 + bit * 6 + half;
                for s in &mut samples[offset..offset + 3] {
                    s.re = 0.9;
                }
            }
        }
        let mut expected = Vec::new();
        for (start, buffer) in [(0, &samples[..2400]), (2400, &samples[2400..])]
        {
            for msg in demodulate6000(&convert_f32_to_i16_iq(buffer)) {
                expected.push((start + msg.sample_position, msg.msg));
            }
        }
        assert_eq!(
            expected.len(),
            3,
            "synthetic packets must be decoded by the unchanged demodulator"
        );
        let path = std::env::temp_dir().join(format!(
            "rs1090-iq-clock-{}-{}.cf32",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|s| {
                s.re.to_le_bytes().into_iter().chain(s.im.to_le_bytes())
            })
            .collect();
        std::fs::write(&path, bytes).unwrap();
        let source = IqAsyncSource::from_file(
            &path,
            1_090_000_000,
            6_000_000,
            2400,
            desperado::IqFormat::Cf32,
        )
        .await
        .unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        // Deliberately different from actual reader size: the legacy hint must
        // not determine timestamps, especially around short buffers.
        file_receiver(tx, source, 42, 6e6, 1000.0, 9999, None).await;
        std::fs::remove_file(path).unwrap();
        for (position, frame) in expected {
            let msg = rx.recv().await.unwrap();
            assert_eq!(msg.frame, frame);
            assert_eq!(msg.timestamp, 1000.0 + position as f64 / 6e6);
            assert_eq!(msg.metadata[0].system_timestamp, msg.timestamp);
        }
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn fallible_replay_distinguishes_eof_errors_and_closed_output() {
        for (index, (bytes, rate, base, closed, success)) in [
            (&[][..], 6e6, 1000.0, false, true),
            (&[0][..], 6e6, 1000.0, false, false),
            (&[0][..], 6e6, 1000.0, true, true),
            (&[][..], 1e6, 1000.0, false, false),
            (&[][..], f64::NAN, 1000.0, false, false),
            (&[][..], 6e6, f64::INFINITY, false, false),
        ]
        .into_iter()
        .enumerate()
        {
            let path = std::env::temp_dir().join(format!(
                "rs1090-iq-outcome-{}-{index}-{}.cf32",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::write(&path, bytes).unwrap();
            let source = IqAsyncSource::from_file(
                &path,
                1_090_000_000,
                6_000_000,
                2400,
                desperado::IqFormat::Cf32,
            )
            .await
            .unwrap();
            let (tx, rx) = mpsc::channel(1);
            let rx = (!closed).then_some(rx);
            let result =
                try_file_receiver(tx, source, 42, rate, base, None).await;
            std::fs::remove_file(path).unwrap();
            assert_eq!(result.is_ok(), success, "case {index}: {result:?}");
            drop(rx);
        }
    }

    #[tokio::test]
    async fn closed_output_cancels_an_idle_replay_read() {
        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source = IqAsyncSource::from_tcp(
            "127.0.0.1",
            listener.local_addr().unwrap().port(),
            1_090_000_000,
            6_000_000,
            2400,
            desperado::IqFormat::Cf32,
        )
        .await
        .unwrap();
        let (_socket, _) = listener.accept().await.unwrap();
        let (tx, rx) = mpsc::channel(1);
        let task =
            tokio::spawn(try_file_receiver(tx, source, 42, 6e6, 1000.0, None));
        tokio::task::yield_now().await;
        drop(rx);
        tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[test]
    fn file_messages_keep_within_buffer_offsets_and_raw_metadata() {
        for rate in [2.4e6, 6.0e6] {
            let first = file_message(
                decoded(24),
                42,
                rate,
                1000.0,
                100,
                Some("capture".into()),
            );
            let second = file_message(
                decoded(48),
                42,
                rate,
                1000.0,
                100,
                Some("capture".into()),
            );
            assert_eq!(first.timestamp, 1000.0 + 124.0 / rate);
            assert_eq!(second.timestamp, 1000.0 + 148.0 / rate);
            assert!(second.timestamp > first.timestamp);
            assert_eq!(first.metadata[0].system_timestamp, first.timestamp);
            assert_eq!(first.metadata[0].serial, 42);
            assert_eq!(first.metadata[0].name.as_deref(), Some("capture"));
            assert_eq!(first.frame, vec![0x8d; 14]);
            assert!(first.metadata[0].gnss_timestamp.is_none());
            assert!(first.metadata[0].nanoseconds.is_none());
        }
    }
}
