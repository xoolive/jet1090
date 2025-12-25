use std::sync::Mutex;

use desperado::IqAsyncSource;
use num_complex::Complex;
use tokio::sync::mpsc;

use crate::decode::crc::modes_checksum;
use crate::decode::time::now_in_ns;
use crate::prelude::*;
use tracing::error;

const MODES_LONG_MSG_BYTES: usize = 14;
const MODES_SHORT_MSG_BYTES: usize = 7;
const MODES_MAG_BUF_SAMPLES: usize = 131_072;
const TRAILING_SAMPLES: usize = 326;

pub async fn receiver(
    tx: mpsc::Sender<TimedMessage>,
    mut source: IqAsyncSource,
    serial: u64,
    rate: f64,
    name: Option<String>,
) {
    while let Some(buf) = source.next().await {
        if let Ok(buf) = buf {
            let outbuf = magnitude(&buf);
            let resulting_data = match rate {
                2.4e6 => demodulate2400(&outbuf).unwrap(),
                _ => {
                    panic!("Unsupported sample rate: {}", rate);
                }
            };
            for data in resulting_data {
                let system_timestamp = now_in_ns() as f64 * 1e-9;
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

pub fn magnitude(data: &[Complex<f32>]) -> MagnitudeBuffer {
    let mut outbuf = MagnitudeBuffer::default();
    for b in data {
        let fq = b.re;
        let fi = b.im;

        let mag_sqr = fi.mul_add(fi, fq * fq);
        let mag = f32::sqrt(mag_sqr);
        outbuf.push(mag.mul_add(f32::from(u16::MAX), 0.5) as u16);
    }
    outbuf
}

// dump1090.h:252
#[derive(Copy, Clone, Debug)]
pub struct MagnitudeBuffer {
    pub data: [u16; TRAILING_SAMPLES + MODES_MAG_BUF_SAMPLES],
    pub length: usize,
    pub first_sample_timestamp_12mhz: usize,
}

impl Default for MagnitudeBuffer {
    fn default() -> Self {
        Self {
            data: [0_u16; TRAILING_SAMPLES + MODES_MAG_BUF_SAMPLES],
            length: 0,
            first_sample_timestamp_12mhz: 0,
        }
    }
}

impl MagnitudeBuffer {
    pub fn push(&mut self, x: u16) {
        self.data[TRAILING_SAMPLES + self.length] = x;
        self.length += 1;
    }
}

// mode_s.c
pub fn getbits(
    data: &[u8],
    firstbit_1idx: usize,
    lastbit_1idx: usize,
) -> usize {
    let mut ans: usize = 0;

    // The original code uses indices that start at 1 and we need 0-indexed values
    let (firstbit, lastbit) = (firstbit_1idx - 1, lastbit_1idx - 1);

    for bit_idx in firstbit..=lastbit {
        ans *= 2;
        let byte_idx: usize = bit_idx / 8;
        let mask = 2_u8.pow(7_u32 - (bit_idx as u32) % 8);
        if (data[byte_idx] & mask) != 0_u8 {
            ans += 1;
        }
    }

    ans
}

// mode_s.c
pub fn score_modes_message(msg: &[u8]) -> i32 {
    let validbits = msg.len() * 8;

    if validbits < 56 {
        return -2;
    }

    // Downlink format
    let df = getbits(msg, 1, 5);
    let msgbits = if (df & 0x10) != 0 {
        MODES_LONG_MSG_BYTES * 8
    } else {
        MODES_SHORT_MSG_BYTES * 8
    };

    if validbits < msgbits {
        return -2;
    }
    if msg.iter().all(|b| *b == 0x00) {
        return -2;
    }

    match df {
        0 | 4 | 5 => {
            // 0:  short air-air surveillance
            // 4:  surveillance, altitude reply
            // 5:  surveillance, altitude reply
            let crc = modes_checksum(msg, MODES_SHORT_MSG_BYTES * 8).unwrap();

            if icao_filter_test(crc) {
                1000
            } else {
                -1
            }
        }
        11 => {
            let crc = modes_checksum(msg, MODES_SHORT_MSG_BYTES * 8).unwrap();

            // 11: All-call reply
            let iid = crc & 0x7f;
            let crc = crc & 0x00ff_ff80;
            let addr = getbits(msg, 9, 32) as u32;

            match (crc, iid, icao_filter_test(addr)) {
                (0, 0, true) => 1600,
                (0, 0, false) => {
                    icao_filter_add(addr);
                    750
                }
                (0, _, true) => 1000,
                (0, _, false) => -1,
                (_, _, _) => -2,
            }
        }
        17 | 18 => {
            // 17: Extended squitter
            // 18: Extended squitter/non-transponder
            let crc = modes_checksum(msg, MODES_LONG_MSG_BYTES * 8).unwrap();
            let addr = getbits(msg, 9, 32) as u32;

            match (crc, icao_filter_test(addr)) {
                (0, true) => 1800,
                (0, false) => {
                    if df == 17 {
                        icao_filter_add(addr);
                    } else {
                        icao_filter_add(addr | ICAO_FILTER_ADSB_NT);
                    }
                    1400
                }
                (_, _) => -2,
            }
        }
        16 | 20 | 21 => {
            // 16: long air-air surveillance
            // 20: Comm-B, altitude reply
            // 21: Comm-B, identity reply
            let crc = modes_checksum(msg, MODES_LONG_MSG_BYTES * 8).unwrap();
            match icao_filter_test(crc) {
                true => 1000,
                false => -2,
            }
        }
        24..=31 => {
            // 24: Comm-D (ELM)
            // 25: Comm-D (ELM)
            // 26: Comm-D (ELM)
            // 27: Comm-D (ELM)
            // 28: Comm-D (ELM)
            // 29: Comm-D (ELM)
            // 30: Comm-D (ELM)
            // 31: Comm-D (ELM)
            let crc = modes_checksum(msg, MODES_LONG_MSG_BYTES * 8).unwrap();
            match icao_filter_test(crc) {
                true => 1000,
                false => -2,
            }
        }
        _ => -2,
    }
}

// icao_filter.c
// The idea is to store plausible icao24 address and avoid returning implausible
// messages.

const ICAO_FILTER_SIZE: u32 = 4096;
const ICAO_FILTER_ADSB_NT: u32 = 1 << 25;

static ICAO_FILTER_A: Mutex<[u32; 4096]> = Mutex::new([0; 4096]);
static ICAO_FILTER_B: Mutex<[u32; 4096]> = Mutex::new([0; 4096]);

pub fn icao_hash(a32: u32) -> u32 // icao_filter.c:38
{
    let a: u64 = u64::from(a32);

    // Jenkins one-at-a-time hash, unrolled for 3 bytes
    let mut hash: u64 = 0;

    hash += a & 0xff;
    hash += hash << 10;
    hash ^= hash >> 6;

    hash += (a >> 8) & 0xff;
    hash += hash << 10;
    hash ^= hash >> 6;

    hash += (a >> 16) & 0xff;
    hash += hash << 10;
    hash ^= hash >> 6;

    hash += hash << 3;
    hash ^= hash >> 11;
    hash += hash << 15;

    (hash as u32) & (ICAO_FILTER_SIZE - 1)
}

// The original function uses a integer return value, but it's used as a boolean
pub fn icao_filter_add(addr: u32) {
    let mut h: u32 = icao_hash(addr);
    let h0: u32 = h;
    if let Ok(mut icao_filter_a) = ICAO_FILTER_A.lock() {
        while (icao_filter_a[h as usize] != 0)
            && (icao_filter_a[h as usize] != addr)
        {
            h = (h + 1) & (ICAO_FILTER_SIZE - 1);
            if h == h0 {
                error!("icao24 hash table full");
                return;
            }
        }

        if icao_filter_a[h as usize] == 0 {
            icao_filter_a[h as usize] = addr;
        }
    }
}
pub fn icao_filter_test(addr: u32) -> bool // icao_filter.c:96
{
    let mut h: u32 = icao_hash(addr);
    let h0: u32 = h;

    if let (Ok(icao_filter_a), Ok(icao_filter_b)) =
        (ICAO_FILTER_A.lock(), ICAO_FILTER_B.lock())
    {
        'loop_a: while (icao_filter_a[h as usize] != 0)
            && (icao_filter_a[h as usize] != addr)
        {
            h = (h + 1) & (ICAO_FILTER_SIZE - 1);
            if h == h0 {
                break 'loop_a;
            }
        }

        if icao_filter_a[h as usize] == addr {
            return true;
        }

        h = h0;

        'loop_b: while (icao_filter_b[h as usize] != 0)
            && (icao_filter_b[h as usize] != addr)
        {
            h = (h + 1) & (ICAO_FILTER_SIZE - 1);
            if h == h0 {
                break 'loop_b;
            }
        }

        if icao_filter_b[h as usize] == addr {
            return true;
        }
    }

    false
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    /// 0|2|4|1|3|0|2|4 -> One
    Zero,
    /// 1|3|0|2|4|1|3|0 -> Two
    One,
    /// 2|4|1|3|0|2|4|1 -> Three
    Two,
    /// 3|0|2|4|1|3|0|2 -> Four
    Three,
    /// 4|1|3|0|2|4|1|3 -> Zero
    Four,
}

impl From<usize> for Phase {
    fn from(num: usize) -> Self {
        match num % 5 {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            4 => Self::Four,
            _ => unimplemented!(),
        }
    }
}

impl Phase {
    /// Increment from 0..4 for incrementing the starting phase
    fn next_start(self) -> Self {
        match self {
            Self::Zero => Self::One,
            Self::One => Self::Two,
            Self::Two => Self::Three,
            Self::Three => Self::Four,
            Self::Four => Self::Zero,
        }
    }

    /// Increment by expected next phase transition for bit denoting
    fn next(self) -> Self {
        match self {
            Self::Zero => Self::Two,
            Self::Two => Self::Four,
            Self::Four => Self::One,
            Self::One => Self::Three,
            Self::Three => Self::Zero,
        }
    }

    /// Amount of mag indexs used, for adding to the next start index
    fn increment_index(self, index: usize) -> usize {
        index
            + match self {
                Self::Zero | Self::Two | Self::One => 2,
                Self::Four | Self::Three => 3,
            }
    }

    /// Calculate the PPM bit
    #[inline(always)]
    fn calculate_bit(self, m: &[u16]) -> i32 {
        let m0 = i32::from(m[0]);
        let m1 = i32::from(m[1]);
        let m2 = i32::from(m[2]);
        match self {
            Self::Zero => 5 * m0 - 3 * m1 - 2 * m2,
            Self::One => 4 * m0 - m1 - 3 * m2,
            Self::Two => 3 * m0 + m1 - 4 * m2,
            Self::Three => 2 * m0 + 3 * m1 - 5 * m2,
            Self::Four => m0 + 5 * m1 - 5 * m2 - i32::from(m[3]),
        }
    }
}

pub struct ModeSMessage {
    /// Binary message
    msg: [u8; 14],
    ///  RSSI, in the range [0..1], as a fraction of full-scale power
    signal_level: f64,
    /// Scoring from scoreModesMessage, if used
    score: i32,
}

pub fn demodulate2400(
    mag: &MagnitudeBuffer,
) -> Result<Vec<ModeSMessage>, &'static str> {
    let mut results = vec![];

    let data = &mag.data;

    let mut skip_count: usize = 0;
    'jloop: for j in 0..mag.length {
        if skip_count > 0 {
            skip_count -= 1;
            continue 'jloop;
        }

        if let Some((high, base_signal, base_noise)) =
            check_preamble(&data[j..j + 14])
        {
            // Check for enough signal
            if base_signal * 2 < 3 * base_noise {
                // about 3.5dB SNR
                continue 'jloop;
            }

            // Check that the "quiet" bits 6,7,15,16,17 are actually quiet
            if i32::from(data[j + 5]) >= high
                || i32::from(data[j + 6]) >= high
                || i32::from(data[j + 7]) >= high
                || i32::from(data[j + 8]) >= high
                || i32::from(data[j + 14]) >= high
                || i32::from(data[j + 15]) >= high
                || i32::from(data[j + 16]) >= high
                || i32::from(data[j + 17]) >= high
                || i32::from(data[j + 18]) >= high
            {
                continue 'jloop;
            }

            // Try all phases
            let mut bestmsg = ModeSMessage {
                msg: [0_u8; MODES_LONG_MSG_BYTES],
                signal_level: 0.,
                score: -2,
            };

            let mut msg: [u8; MODES_LONG_MSG_BYTES] =
                [0_u8; MODES_LONG_MSG_BYTES];

            for try_phase in 4..9 {
                let mut slice_loc: usize = j + 19 + (try_phase / 5);
                let mut phase = Phase::from(try_phase);

                for msg in msg.iter_mut().take(MODES_LONG_MSG_BYTES) {
                    let slice_this_byte: &[u16] = &data[slice_loc..];

                    let starting_phase = phase;
                    let mut the_byte = 0x00;
                    let mut index = 0;
                    // for each phase-bit
                    for i in 0..8 {
                        // find if phase distance denotes a high bit
                        if phase
                            .calculate_bit(&slice_this_byte[index..index + 4])
                            > 0
                        {
                            the_byte |= 1 << (7 - i);
                        }
                        // increment to next phase, increase index
                        index = phase.increment_index(index);
                        phase = phase.next();
                    }
                    // save bytes and move the next starting phase
                    *msg = the_byte;
                    slice_loc += index;
                    phase = starting_phase.next_start();
                }

                let score = score_modes_message(&msg);

                if score > bestmsg.score {
                    bestmsg.msg.clone_from_slice(&msg);
                    bestmsg.score = score;

                    let mut scaled_signal_power = 0_u64;
                    let signal_len = msg.len() * 12 / 5;
                    for k in 0..signal_len {
                        let mag = data[j + 19 + k] as u64;
                        scaled_signal_power += mag * mag;
                    }
                    let signal_power =
                        scaled_signal_power as f64 / 65535.0 / 65535.0;
                    bestmsg.signal_level = signal_power / signal_len as f64;
                }
            }

            // Do we have a candidate?
            if bestmsg.score < 0 {
                continue 'jloop;
            }

            results.push(bestmsg);
        }
    }

    Ok(results)
}

fn check_preamble(preamble: &[u16]) -> Option<(i32, u32, u32)> {
    // This gets rid of the 3 core::panicking::panic_bounds_check calls,
    // but doesn't look to improve performance
    assert!(preamble.len() == 14);

    // quick check: we must have a rising edge 0->1 and a falling edge 12->13
    if !(preamble[0] < preamble[1] && preamble[12] > preamble[13]) {
        return None;
    }

    // check the rising and falling edges of signal
    if preamble[1] > preamble[2] &&                                       // 1
       preamble[2] < preamble[3] && preamble[3] > preamble[4] &&          // 3
       preamble[8] < preamble[9] && preamble[9] > preamble[10] &&         // 9
       preamble[10] < preamble[11]
    {
        // 11-12
        // peaks at 1,3,9,11-12: phase 3
        let high = (i32::from(preamble[1])
            + i32::from(preamble[3])
            + i32::from(preamble[9])
            + i32::from(preamble[11])
            + i32::from(preamble[12]))
            / 4;
        let base_signal = u32::from(preamble[1])
            + u32::from(preamble[3])
            + u32::from(preamble[9]);
        let base_noise = u32::from(preamble[5])
            + u32::from(preamble[6])
            + u32::from(preamble[7]);
        Some((high, base_signal, base_noise))
    } else if preamble[1] > preamble[2] &&                                // 1
              preamble[2] < preamble[3] && preamble[3] > preamble[4] &&   // 3
              preamble[8] < preamble[9] && preamble[9] > preamble[10] &&  // 9
              preamble[11] < preamble[12]
    {
        // 12
        // peaks at 1,3,9,12: phase 4
        let high = (i32::from(preamble[1])
            + i32::from(preamble[3])
            + i32::from(preamble[9])
            + i32::from(preamble[12]))
            / 4;
        let base_signal = u32::from(preamble[1])
            + u32::from(preamble[3])
            + u32::from(preamble[9])
            + u32::from(preamble[12]);
        let base_noise = u32::from(preamble[5])
            + u32::from(preamble[6])
            + u32::from(preamble[7])
            + u32::from(preamble[8]);
        Some((high, base_signal, base_noise))
    } else if preamble[1] > preamble[2] &&                                // 1
              preamble[2] < preamble[3] && preamble[4] > preamble[5] &&   // 3-4
              preamble[8] < preamble[9] && preamble[10] > preamble[11] && // 9-10
              preamble[11] < preamble[12]
    {
        // 12
        // peaks at 1,3-4,9-10,12: phase 5
        let high = (i32::from(preamble[1])
            + i32::from(preamble[3])
            + i32::from(preamble[4])
            + i32::from(preamble[9])
            + i32::from(preamble[10])
            + i32::from(preamble[12]))
            / 4;
        let base_signal = u32::from(preamble[1]) + u32::from(preamble[12]);
        let base_noise = u32::from(preamble[6]) + u32::from(preamble[7]);
        Some((high, base_signal, base_noise))
    } else if preamble[1] > preamble[2] &&                                 // 1
              preamble[3] < preamble[4] && preamble[4] > preamble[5] &&    // 4
              preamble[9] < preamble[10] && preamble[10] > preamble[11] && // 10
              preamble[11] < preamble[12]
    {
        // 12
        // peaks at 1,4,10,12: phase 6
        let high = (i32::from(preamble[1])
            + i32::from(preamble[4])
            + i32::from(preamble[10])
            + i32::from(preamble[12]))
            / 4;
        let base_signal = u32::from(preamble[1])
            + u32::from(preamble[4])
            + u32::from(preamble[10])
            + u32::from(preamble[12]);
        let base_noise = u32::from(preamble[5])
            + u32::from(preamble[6])
            + u32::from(preamble[7])
            + u32::from(preamble[8]);
        Some((high, base_signal, base_noise))
    } else if preamble[2] > preamble[3] &&                                 // 1-2
              preamble[3] < preamble[4] && preamble[4] > preamble[5] &&    // 4
              preamble[9] < preamble[10] && preamble[10] > preamble[11] && // 10
              preamble[11] < preamble[12]
    {
        // 12
        // peaks at 1-2,4,10,12: phase 7
        let high = (i32::from(preamble[1])
            + i32::from(preamble[2])
            + i32::from(preamble[4])
            + i32::from(preamble[10])
            + i32::from(preamble[12]))
            / 4;
        let base_signal = u32::from(preamble[4])
            + u32::from(preamble[10])
            + u32::from(preamble[12]);
        let base_noise = u32::from(preamble[6])
            + u32::from(preamble[7])
            + u32::from(preamble[8]);
        Some((high, base_signal, base_noise))
    } else {
        None
    }
}
