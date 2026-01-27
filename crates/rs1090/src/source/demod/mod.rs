use crate::decode::crc::modes_checksum;
use num_complex::Complex;
use std::sync::RwLock;
use tracing::error;

pub mod demod2400;
pub mod demod6000;

/// Mode S message with metadata
#[derive(Clone, Debug)]
pub struct ModeSMessage {
    /// Binary message
    pub msg: [u8; 14],
    /// RSSI, in the range [0..1], as a fraction of full-scale power
    pub signal_level: f64,
    /// Scoring from validation, if used
    pub score: i32,
    /// Sample position in buffer where this message was found
    pub sample_position: usize,
}

// Shared message length constants
pub const MODES_SHORT_MSG_BYTES: usize = 7;
pub const MODES_SHORT_MSG_BITS: usize = 56;
pub const MODES_LONG_MSG_BYTES: usize = 14;
pub const MODES_LONG_MSG_BITS: usize = 112;

/// Convert IQ samples to magnitude values
pub fn magnitude_u16(data: &[Complex<f32>]) -> Vec<u16> {
    let mut magnitudes = Vec::with_capacity(data.len());
    for sample in data {
        let i = sample.re;
        let q = sample.im;

        let mag_sqr = q.mul_add(q, i * i);
        let mag = f32::sqrt(mag_sqr);
        magnitudes.push(mag.mul_add(f32::from(u16::MAX), 0.5) as u16);
    }
    magnitudes
}

pub fn convert_f32_to_i16_iq(buf: &[Complex<f32>]) -> Vec<i16> {
    let mut iq_i16 = Vec::with_capacity(buf.len() * 2);
    for sample in buf {
        // Scale f32 [-1.0, 1.0] to i16 range
        let i = (sample.re * 32767.0).clamp(-32768.0, 32767.0) as i16;
        let q = (sample.im * 32767.0).clamp(-32768.0, 32767.0) as i16;
        iq_i16.push(i);
        iq_i16.push(q);
    }
    iq_i16
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

// icao_filter.c
// The idea is to store plausible icao24 address and avoid returning implausible
// messages.
// Uses RwLock for concurrent reads (multiple demodulators can check filter simultaneously)

const ICAO_FILTER_SIZE: u32 = 4096;
const ICAO_FILTER_ADSB_NT: u32 = 1 << 25;

static ICAO_FILTER_A: RwLock<[u32; 4096]> = RwLock::new([0; 4096]);
static ICAO_FILTER_B: RwLock<[u32; 4096]> = RwLock::new([0; 4096]);

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
    if let Ok(mut icao_filter_a) = ICAO_FILTER_A.write() {
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
        (ICAO_FILTER_A.read(), ICAO_FILTER_B.read())
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

/// Check if ICAO address is in a plausible allocated range
/// Rejects obvious noise like 000000, FFFFFF, and unallocated high ranges
pub fn is_plausible_icao(addr: u32) -> bool {
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

/// Validate and score a Mode S message
/// Returns score if message is valid, -2 if invalid
/// Higher scores indicate higher confidence (known ICAO, perfect CRC, etc.)
pub fn validate_modes_message(msg: &[u8]) -> i32 {
    let validbits = msg.len() * 8;

    if validbits < MODES_SHORT_MSG_BITS {
        return -2;
    }

    // Downlink format
    let df = getbits(msg, 1, 5);
    let msgbits = if (df & 0x10) != 0 {
        MODES_LONG_MSG_BITS
    } else {
        MODES_SHORT_MSG_BITS
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
            let crc = match modes_checksum(msg, MODES_SHORT_MSG_BITS) {
                Ok(c) => c,
                Err(_) => return -2,
            };

            if !is_plausible_icao(crc) {
                return -2;
            }

            if icao_filter_test(crc) {
                1000
            } else {
                -1
            }
        }
        11 => {
            let crc = match modes_checksum(msg, MODES_SHORT_MSG_BITS) {
                Ok(c) => c,
                Err(_) => return -2,
            };

            // 11: All-call reply
            let iid = crc & 0x7f;
            let crc = crc & 0x00ff_ff80;
            let addr = getbits(msg, 9, 32) as u32;

            if !is_plausible_icao(addr) {
                return -2;
            }

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
            let crc = match modes_checksum(msg, MODES_LONG_MSG_BITS) {
                Ok(c) => c,
                Err(_) => return -2,
            };
            let addr = getbits(msg, 9, 32) as u32;

            if !is_plausible_icao(addr) {
                return -2;
            }

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
            let crc = match modes_checksum(msg, MODES_LONG_MSG_BITS) {
                Ok(c) => c,
                Err(_) => return -2,
            };
            if !is_plausible_icao(crc) {
                return -2;
            }
            match icao_filter_test(crc) {
                true => 1000,
                false => -2,
            }
        }
        24..=31 => {
            // 24-31: Comm-D (ELM)
            let crc = match modes_checksum(msg, MODES_LONG_MSG_BITS) {
                Ok(c) => c,
                Err(_) => return -2,
            };
            if !is_plausible_icao(crc) {
                return -2;
            }
            match icao_filter_test(crc) {
                true => 1000,
                false => -2,
            }
        }
        _ => -2,
    }
}
