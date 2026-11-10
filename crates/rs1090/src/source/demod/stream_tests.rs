use super::{
    demod2400::Demodulator2400, demod6000::Demodulator6000, ModeSMessage,
};

// Payload independently recovered from a real GQRX waveform and checked with
// pyModeS CRC (no correction). IQ below is synthetic, not a private recording.
const LONG: &str = "8d3964eaea07e7adf33c0815caf7";
const SHORT: &str = "5d48ae83eb46b7";

type Signature = Vec<(u64, [u8; 14], u64)>;

fn signature(messages: Vec<(u64, ModeSMessage)>) -> Signature {
    // Score includes the shared ICAO-learning state and can change after an
    // earlier test/run learns the address. Frame/index/RSSI must not change.
    messages
        .into_iter()
        .map(|(index, message)| {
            (index, message.msg, message.signal_level.to_bits())
        })
        .collect()
}

fn iq6000(
    frame: &str,
    starts: &[usize],
    length: usize,
    amplitude: i16,
) -> Vec<[i16; 2]> {
    let frame = hex::decode(frame).unwrap();
    let mut iq = vec![[0, 0]; length];
    for &start in starts {
        for offset in [0, 6, 21, 27] {
            for sample in &mut iq[start + offset..start + offset + 3] {
                sample[0] = amplitude;
            }
        }
        for bit in 0..frame.len() * 8 {
            let half = if frame[bit / 8] & (1 << (7 - bit % 8)) != 0 {
                0
            } else {
                3
            };
            let offset = start + 48 + bit * 6 + half;
            for sample in &mut iq[offset..offset + 3] {
                sample[0] = amplitude;
            }
        }
    }
    iq
}

fn run6000(iq: &[[i16; 2]], sizes: &[usize]) -> Signature {
    let mut demodulator = Demodulator6000::default();
    let mut messages = Vec::new();
    let mut start = 0;
    for &size in sizes.iter().cycle() {
        let end = (start + size).min(iq.len());
        messages.extend(demodulator.push(&iq[start..end]));
        start = end;
        if start == iq.len() {
            break;
        }
    }
    messages.extend(demodulator.finish());
    assert!(demodulator.finish().is_empty());
    signature(messages)
}

#[test]
fn every_preamble_and_payload_split_preserves_the_frame() {
    let iq = iq6000(LONG, &[200], 1100, 29000);
    let flat: Vec<_> = iq.iter().flatten().copied().collect();
    let expected = signature(
        super::demod6000::demodulate6000(&flat)
            .into_iter()
            .map(|m| (m.sample_position as u64, m))
            .collect(),
    );
    assert_eq!(expected.len(), 1);
    for split in 199..=921 {
        assert_eq!(run6000(&iq, &[split, iq.len()]), expected, "split {split}");
    }
}

#[test]
fn tiny_and_irregular_chunks_preserve_repeated_transmissions() {
    let iq = iq6000(LONG, &[200, 1200, 2500], 3400, 29000);
    let expected = run6000(&iq, &[iq.len()]);
    assert_eq!(expected.len(), 3);
    assert!(expected.windows(2).all(|w| w[0].0 < w[1].0));
    for sizes in [&[1][..], &[2, 3, 7, 127, 719], &[720], &[8136]] {
        assert_eq!(run6000(&iq, sizes), expected, "chunks {sizes:?}");
    }
}

#[test]
fn adaptive_state_is_not_reset_by_short_chunks() {
    // A weak burst after a quiet prefix: the frozen detector's existing noise
    // adaptation can acquire it, but resetting the detector each block cannot.
    let iq = iq6000(LONG, &[200_000], 201_000, 512);
    let expected = run6000(&iq, &[iq.len()]);
    assert_eq!(expected.len(), 1);
    for size in [127, 1024, 8136, 65536] {
        assert_eq!(run6000(&iq, &[size]), expected, "chunk {size}");
    }
}

#[test]
fn failed_early_preamble_does_not_skip_the_valid_adjacent_phase() {
    // Real GQRX regressions had a low-energy first tap four samples before
    // the true onset. Together with two real pulses it passes the unchanged
    // three-of-four gate, but fails CRC. A six-sample jump then skips the valid
    // phase. Reproduce that mechanism without embedding a private IQ excerpt.
    let start = 200_000;
    let mut iq = iq6000(LONG, &[start], start + 1000, 512);
    for sample in &mut iq[start - 6..start - 3] {
        sample[0] = 100;
    }
    // Front-loaded pulse apertures make the late phase genuinely fail CRC;
    // ideal rectangular pulses alone also decode at that late phase and would
    // fail to reproduce the real-recording loss.
    let frame = hex::decode(LONG).unwrap();
    for bit in 0..112 {
        let half = if frame[bit / 8] & (1 << (7 - bit % 8)) != 0 {
            0
        } else {
            3
        };
        let offset = start + 48 + bit * 6 + half;
        for (sample, amplitude) in
            iq[offset..offset + 3].iter_mut().zip([512, 256, 128])
        {
            sample[0] = amplitude;
        }
    }
    let messages = run6000(&iq, &[8136]);
    assert_eq!(
        messages.len(),
        1,
        "a failed false cue must not hide the real frame"
    );
    assert_eq!(messages[0].1.as_slice(), hex::decode(LONG).unwrap());
}

#[test]
fn complete_short_eof_tail_is_decoded_without_padding() {
    let iq = iq6000(SHORT, &[20], 420, 29000);
    let mut demodulator = Demodulator6000::default();
    assert!(demodulator.push(&iq).is_empty()); // Not enough long-frame lookahead.
    let messages = demodulator.finish();
    assert_eq!(messages.len(), 1);
    assert_eq!(&messages[0].1.msg[..7], &hex::decode(SHORT).unwrap());
    let mut demodulator = Demodulator6000::default();
    assert!(demodulator.push(&iq[..300]).is_empty());
    assert!(demodulator.finish().is_empty()); // Incomplete short payload.
    assert!(demodulator.finish().is_empty());
}

fn magnitude2400(frame: &str, starts: &[usize], length: usize) -> Vec<u16> {
    let frame = hex::decode(frame).unwrap();
    let mut samples = vec![100.0_f64; length];
    for &start in starts {
        let mut pulses = vec![0.0, 1.0, 3.5, 4.5];
        for bit in 0..frame.len() * 8 {
            let half = if frame[bit / 8] & (1 << (7 - bit % 8)) != 0 {
                0.0
            } else {
                0.5
            };
            pulses.push(8.0 + bit as f64 + half);
        }
        for pulse in pulses {
            // Fractional aperture overlap produces the normal2.4MS/s phases.
            let left = start as f64 + 0.25 + pulse * 2.4;
            let right = left + 1.2;
            for (i, sample) in samples
                .iter_mut()
                .enumerate()
                .take(right.ceil() as usize)
                .skip(left.floor() as usize)
            {
                *sample += 50_000.0
                    * (right.min(i as f64 + 1.0) - left.max(i as f64)).max(0.0);
            }
        }
    }
    samples.into_iter().map(|s| s as u16).collect()
}

fn run2400(samples: &[u16], size: usize) -> Signature {
    let mut demodulator = Demodulator2400::default();
    let mut messages = Vec::new();
    for part in samples.chunks(size) {
        messages.extend(demodulator.push(part));
    }
    messages.extend(demodulator.finish());
    assert!(demodulator.finish().is_empty());
    signature(messages)
}

#[test]
fn magnitude_lookahead_is_carried_once_with_absolute_positions() {
    let samples = magnitude2400(LONG, &[200, 1200], 2000);
    let expected = signature(
        super::demod2400::demodulate2400(&samples)
            .into_iter()
            .map(|m| (m.sample_position as u64, m))
            .collect(),
    );
    assert_eq!(expected.len(), 2);
    for size in [1, 7, 127, 325, 326, 327, 1024, 8136] {
        assert_eq!(run2400(&samples, size), expected, "chunk {size}");
    }
}

#[tokio::test]
#[ignore = "requires private captures: set RS1090_IQ_FIXTURES to decode_adsb"]
async fn private_recordings_preserve_independently_verified_anchors() {
    use crate::prelude::StreamExt;
    use desperado::{IqAsyncSource, IqFormat};
    let root = std::path::PathBuf::from(
        std::env::var_os("RS1090_IQ_FIXTURES")
            .expect("set RS1090_IQ_FIXTURES; recordings are never downloaded"),
    );
    // Receiver-prefixed UTC names were normalized from preserved filesystem
    // timestamps; the times are not GPS ground truth. Rate/format stay explicit
    // here: support for parsing these names belongs upstream in Desperado.
    type Case<'a> = (&'a str, IqFormat, u32, usize, &'a [(u64, &'a str)]);
    let cases: &[Case<'_>] = &[
        // The two default-chunk losses are also present in correction-disabled
        // readsb output. Positions are rs1090's measured detector indices.
        (
            "rtlsdr_20251105_071950_1090000000_2400000_cu8.raw",
            IqFormat::Cu8,
            2_400_000,
            17_432_576,
            &[
                (10_137_261, "8c39d2b239eac2b0f03d16a9cf7d"),
                (13_302_077, "90392afb2810047f7a2c5985c08d"),
            ],
        ),
        // These payloads were independently recovered from raw PPM half-bit
        // energies/preamble contrast and checked with pyModeS CRC, not merely
        // accepted because the existing decoder emitted them.
        (
            "gqrx_20260127_141546_1090000000_6000000_fc.raw",
            IqFormat::Cf32,
            6_000_000,
            6_000_000,
            &[
                (644_569, "8d3964eaea07e7adf33c0815caf7"),
                (1_789_902, "8d3964ea5833f124fc5fb6ae86d3"),
            ],
        ),
        (
            "airspy_20260127_141241_1090000000_6000000_cs16_v8_l8_m0.raw",
            IqFormat::Cs16,
            6_000_000,
            6_000_000,
            &[
                (288_133, "8d34569158bf04b9e8448897e11f"),
                (892_111, "8d407af3250815f8c60820790c3b"),
            ],
        ),
        // No independently verified frame in this prefix: control only, not
        // evidence of format correctness or an exhaustive no-signal verdict.
        (
            "airspy_20260112_230839_1090000000_6000000_cs16.raw",
            IqFormat::Cs16,
            6_000_000,
            6_000_000,
            &[],
        ),
    ];
    for &(name, format, rate, limit, anchors) in cases {
        let mut source = IqAsyncSource::from_file(
            root.join(name),
            1_090_000_000,
            rate,
            limit,
            format,
        )
        .await
        .unwrap();
        let samples = source.next().await.unwrap().unwrap();
        drop(source);
        assert_eq!(samples.len(), limit, "unexpected capture prefix: {name}");
        let magnitudes =
            (rate == 2_400_000).then(|| super::magnitude_u16(&samples));
        let iq =
            (rate == 6_000_000).then(|| super::convert_f32_to_i16_iq(&samples));
        drop(samples);
        let one_shot = || {
            let messages = if let Some(magnitudes) = &magnitudes {
                super::demod2400::demodulate2400(magnitudes)
            } else {
                super::demod6000::demodulate6000(iq.as_ref().unwrap())
            };
            signature(
                messages
                    .into_iter()
                    .map(|m| (m.sample_position as u64, m))
                    .collect(),
            )
        };
        // Fix learned-address initial conditions across runs. This comparison
        // is the chunk contract, not a claim that one-shot output is complete
        // ground truth; independent anchors above are asserted separately.
        let _ = one_shot();
        let expected = one_shot();
        for &(index, frame) in anchors {
            let frame = hex::decode(frame).unwrap();
            assert!(
                expected
                    .iter()
                    .any(|(i, msg, _)| *i == index && msg.as_slice() == frame),
                "missing independent anchor {name} at {index}"
            );
        }
        for size in [127, 1024, 8136, 65536] {
            let result = if let Some(magnitudes) = &magnitudes {
                run2400(magnitudes, size)
            } else {
                run6000(iq.as_ref().unwrap().as_chunks::<2>().0, &[size])
            };
            assert_eq!(result, expected, "{name}, chunk {size}");
        }
    }
}
