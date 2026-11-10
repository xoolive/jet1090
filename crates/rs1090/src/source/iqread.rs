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

enum ReplayDemodulator {
    At2400(super::demod::demod2400::Demodulator2400),
    At6000(super::demod::demod6000::Demodulator6000),
}

impl ReplayDemodulator {
    fn new(rate: f64) -> Self {
        match rate {
            2.4e6 => Self::At2400(Default::default()),
            6.0e6 => Self::At6000(Default::default()),
            _ => unreachable!("validated sample rate"),
        }
    }

    fn push(
        &mut self,
        samples: &[num_complex::Complex<f32>],
    ) -> Vec<(u64, super::demod::ModeSMessage)> {
        match self {
            Self::At2400(demodulator) => {
                demodulator.push(&magnitude_u16(samples))
            }
            Self::At6000(demodulator) => demodulator
                .push(convert_f32_to_i16_iq(samples).as_chunks::<2>().0),
        }
    }

    fn finish(&mut self) -> Vec<(u64, super::demod::ModeSMessage)> {
        match self {
            Self::At2400(demodulator) => demodulator.finish(),
            Self::At6000(demodulator) => demodulator.finish(),
        }
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
    let mut demodulator = ReplayDemodulator::new(rate);
    let outcome = loop {
        let buf = tokio::select! {
            biased;
            _ = tx.closed() => return Ok(()),
            buf = source.next() => buf,
        };
        let buf = match buf {
            None => break Ok(()),
            Some(Err(error)) => break Err(error),
            Some(Ok(buf)) => buf,
        };
        for (sample_position, data) in demodulator.push(&buf) {
            let tmsg = file_message(
                data,
                serial,
                rate,
                base_timestamp,
                sample_position,
                name.clone(),
            );
            if tx.send(tmsg).await.is_err() {
                return Ok(());
            }
        }
    };
    // A read error also ends the available prefix. Validate complete candidates
    // in that prefix without padding, then return the original read failure.
    for (sample_position, data) in demodulator.finish() {
        let tmsg = file_message(
            data,
            serial,
            rate,
            base_timestamp,
            sample_position,
            name.clone(),
        );
        if tx.send(tmsg).await.is_err() {
            return outcome;
        }
    }
    outcome
}

fn file_message(
    data: super::demod::ModeSMessage,
    serial: u64,
    rate: f64,
    base_timestamp: f64,
    sample_position: u64,
    name: Option<String>,
) -> TimedMessage {
    let timestamp = base_timestamp + sample_position as f64 / rate;
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
    async fn file_replay_keeps_sample_clock_across_tiny_and_short_chunks() {
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
        let expected: Vec<_> = demodulate6000(&convert_f32_to_i16_iq(&samples))
            .into_iter()
            .map(|msg| (msg.sample_position, msg.msg))
            .collect();
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
        for size in [1, 7, 127, 719, 2400, 8136] {
            let source = IqAsyncSource::from_file(
                &path,
                1_090_000_000,
                6_000_000,
                size,
                desperado::IqFormat::Cf32,
            )
            .await
            .unwrap();
            let (tx, mut rx) = mpsc::channel(8);
            // The legacy hint must not determine timestamps. Some of these
            // chunks are shorter than a preamble, let alone a whole packet.
            file_receiver(tx, source, 42, 6e6, 1000.0, 9999, None).await;
            for &(position, frame) in &expected {
                let msg = rx.recv().await.unwrap();
                assert_eq!(msg.frame, frame);
                assert_eq!(msg.timestamp, 1000.0 + position as f64 / 6e6);
                assert_eq!(msg.metadata[0].system_timestamp, msg.timestamp);
            }
            assert!(rx.recv().await.is_none());
        }
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn read_error_flushes_a_complete_short_tail_without_hiding_the_error()
    {
        let frame = hex::decode("5d48ae83eb46b7").unwrap();
        let mut iq = vec![[0_i16; 2]; 420];
        for offset in [0, 6, 21, 27] {
            for sample in &mut iq[20 + offset..20 + offset + 3] {
                sample[0] = 29000;
            }
        }
        for bit in 0..56 {
            let half = if frame[bit / 8] & (1 << (7 - bit % 8)) != 0 {
                0
            } else {
                3
            };
            let offset = 20 + 48 + bit * 6 + half;
            for sample in &mut iq[offset..offset + 3] {
                sample[0] = 29000;
            }
        }
        let mut bytes: Vec<_> =
            iq.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        bytes.push(0); // Incomplete next complex pair, not missing packet bits.
        let path = std::env::temp_dir().join(format!(
            "rs1090-iq-short-error-{}-{}.cs16",
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
            420,
            desperado::IqFormat::Cs16,
        )
        .await
        .unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let result = try_file_receiver(tx, source, 42, 6e6, 1000.0, None).await;
        std::fs::remove_file(path).unwrap();
        assert!(result.is_err());
        let message = rx.recv().await.unwrap();
        assert_eq!(&message.frame[..7], frame);
        assert_eq!(message.timestamp, 1000.0 + 21.0 / 6e6);
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    #[ignore = "requires private captures: set RS1090_IQ_FIXTURES to decode_adsb"]
    async fn private_gqrx_phase_counterexamples_survive_file_replay() {
        let root = std::path::PathBuf::from(
            std::env::var_os("RS1090_IQ_FIXTURES")
                .expect("set RS1090_IQ_FIXTURES"),
        );
        let source = IqAsyncSource::from_file(
            root.join("gqrx_20260127_141546_1090000000_6000000_fc.raw"),
            1_090_000_000,
            6_000_000,
            8136,
            desperado::IqFormat::Cf32,
        )
        .await
        .unwrap();
        let (tx, mut rx) = mpsc::channel(128);
        let task =
            tokio::spawn(try_file_receiver(tx, source, 42, 6e6, 0.0, None));
        let mut seen = std::collections::BTreeSet::new();
        while let Some(message) = rx.recv().await {
            let index = (message.timestamp * 6e6).round() as u64;
            let length = if message.frame[0] & 0x80 != 0 { 14 } else { 7 };
            assert!(
                seen.insert((index, hex::encode(&message.frame[..length]))),
                "same detection emitted twice"
            );
            assert_eq!(message.metadata[0].system_timestamp, message.timestamp);
        }
        task.await.unwrap().unwrap();
        // All13 raw-PPM-confirmed packets lost by the initial continuous-state
        // version are required. These are measured detector indices, not a
        // claim of sub-sample physical TOA or GPS precision.
        for (index, frame) in [
            (19_391_145, "a00017b0bc19f730a0040043e2c6"),
            (23_946_440, "5d0a008be97326"),
            (25_660_163, "28000229c82690"),
            (31_289_915, "a00017b0bc29f730bffc00f3f697"),
            (32_093_060, "8d0a008b58bf01208855886ca867"),
            (33_223_174, "5d0a008be97332"),
            (34_914_707, "8d0a008be1022900000000e46d37"),
            (43_643_096, "8d0a008b99088ab190049310c5be"),
            (44_698_108, "a8000229c84e4270a800003a4654"),
            (53_438_168, "200017b046b9fc"),
            (61_651_802, "200017b046b9fc"),
            (62_255_371, "a00017b0c84e4270a80000394537"),
            (62_321_062, "200017b046b9fc"),
        ] {
            assert!(
                seen.contains(&(index, frame.to_owned())),
                "lost real counterexample {frame} at {index}"
            );
        }
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
    fn file_sample_clock_does_not_wrap_at_32_bit_sample_counts() {
        let position = u32::MAX as u64 + 123;
        let message = file_message(decoded(0), 42, 6e6, 1000.0, position, None);
        assert_eq!(message.timestamp, 1000.0 + position as f64 / 6e6);
    }

    #[test]
    fn file_messages_keep_absolute_sample_positions_and_raw_metadata() {
        for rate in [2.4e6, 6.0e6] {
            let first = file_message(
                decoded(24),
                42,
                rate,
                1000.0,
                124,
                Some("capture".into()),
            );
            let second = file_message(
                decoded(48),
                42,
                rate,
                1000.0,
                148,
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
