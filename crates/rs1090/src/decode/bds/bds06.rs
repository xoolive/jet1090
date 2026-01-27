#![allow(clippy::suspicious_else_formatting)]

use super::super::cpr::CPRFormat;
use deku::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;
use tracing::debug;

/**
 * ## Surface Position (BDS 0,6)
 *
 * Extended squitter message providing accurate surface position information
 * for aircraft on the ground.  
 * Per ICAO Doc 9871 Table A-2-7: BDS code 0,6 — Extended squitter surface position
 *
 * Unlike airborne position messages, surface position messages include ground speed
 * and track angle instead of altitude (which is not needed for ground operations).
 *
 * Message Structure (56 bits):
 * | TC  | MOV | S   | TRK | T   | F   | LAT-CPR | LON-CPR |
 * | --- | --- | --- | --- | --- | --- | ------- | ------- |
 * | 5   | 7   | 1   | 7   | 1   | 1   | 17      | 17      |
 *
 * Field Encoding per ICAO Doc 9871 §A.2.3.3:
 * - TC (bits 1-5): Format Type Code, determines position accuracy
 * - MOV (bits 6-12): Movement (ground speed), 7-bit non-linear encoding
 * - S (bit 13): Ground track status (0=invalid, 1=valid)
 * - TRK (bits 14-20): Ground track angle (true north), 7-bit encoding
 * - T (bit 21): Time synchronization (1=synchronized to UTC)
 * - F (bit 22): CPR format (0=even, 1=odd) per §C.2.6.7
 * - LAT-CPR (bits 23-39): 17-bit CPR-encoded latitude (low-order 17 bits of 19-bit value)
 * - LON-CPR (bits 40-56): 17-bit CPR-encoded longitude (low-order 17 bits of 19-bit value)
 *
 * Update Rates per §A.2.3.3:
 * - MOV and TRK fields: minimum once per 1.3s
 * - All other fields: minimum once per 0.2s
 *
 * CPR Characteristics for Surface:
 * - Unambiguous range: 166.5 km (90 NM) for local decoding
 * - Positional accuracy: ~1.25 meters (typical), ~3.0m near poles (±87° latitude)
 */

#[derive(Debug, PartialEq, DekuRead, Serialize, Deserialize, Copy, Clone)]
#[deku(ctx = "tc: u8")]
pub struct SurfacePosition {
    #[deku(skip, default = "14 - tc")]
    #[serde(rename = "NUCp")]
    /// Navigation Uncertainty Category (position), based on the typecode
    pub nuc_p: u8,

    #[deku(reader = "read_groundspeed(deku::reader)")]
    /// Ground Speed (bits 6-12): Per ICAO Doc 9871 §A.2.3.3.1
    ///
    /// 7-bit non-linear encoding optimized for ground operations:
    ///   - Lower speeds encoded with higher precision
    ///   - Range: [0, 175+] kt
    ///   - Returns None if movement field = 0 (no information available)
    ///   - Returns None if movement field ≥ 125 (reserved)
    ///   - See read_groundspeed() for complete encoding table
    ///
    /// Minimum update rate: once per 1.3s
    pub groundspeed: Option<f64>,

    #[deku(bits = "1")] // bit 13
    #[serde(skip)]
    /// Ground Track Status (bit 13): Per ICAO Doc 9871 §A.2.3.3.2.1
    ///
    /// Indicates validity of ground track value:
    ///   - false (0): Ground track invalid/not available
    ///   - true (1): Ground track valid
    ///
    /// Minimum update rate: once per 1.3s
    pub track_status: bool,

    #[deku(
        bits = "7",
        map = "|value: u8| -> Result<_, DekuError> {
            if *track_status {
                Ok(Some(value as f64 * 360. / 128.))
            } else {
                Ok(None)
            }
        }"
    )]
    /// Ground Track Angle (bits 14-20): Per ICAO Doc 9871 §A.2.3.3.2.2  
    /// Direction of aircraft motion on the surface, clockwise from true north.
    ///
    /// 7-bit unsigned angular weighted binary encoding:
    ///   - MSB = 180 degrees
    ///   - LSB = 360/128 degrees (2.8125°)
    ///   - Formula: angle = value × (360/128) degrees
    ///   - Range: [0, 357.1875] degrees
    ///   - Zero indicates true north
    ///   - Rounded to nearest multiple of 360/128 degrees
    ///
    /// Returns None if track_status is false.
    /// Minimum update rate: once per 1.3s
    pub track: Option<f64>,

    // Time synchronization flag
    #[deku(bits = "1")]
    #[serde(skip)]
    /// Time Synchronization (bit 21): Per ICAO Doc 9871 §A.2.3.3.4
    ///
    /// Indicates whether time of applicability is synchronized with UTC:
    ///   - false (T=0): Time not synchronized to UTC
    ///   - true (T=1): Time synchronized to UTC (only for TC 5 and 6)
    ///
    /// When T=1, the F (parity) bit alternates for successive 0.2s UTC time ticks.
    /// Minimum update rate: once per 0.2s
    pub t: bool,

    /// CPR Format (bit 22): Per ICAO Doc 9871 §A.2.3.3.3 and §C.2.6.7
    ///
    /// Compact Position Reporting format type:
    ///   - Even (F=0): Even format coding
    ///   - Odd (F=1): Odd format coding
    ///
    /// When t=true, this bit also encodes the 0.2-second time tick.
    /// CPR uses 19-bit encoding for surface (vs 17-bit for airborne).
    pub parity: CPRFormat,

    #[deku(bits = "17", endian = "big")]
    /// Encoded Latitude (bits 23-39): Per ICAO Doc 9871 §A.2.3.3.5
    ///
    /// Low-order 17 bits of 19-bit CPR-encoded latitude value for surface.
    /// CPR maintains positional accuracy of ~1.25 meters (typical).
    ///
    /// Note: Longitude accuracy degrades to ~3.0m near poles (±87° latitude).
    pub lat_cpr: u32,

    #[deku(bits = "17", endian = "big")]
    /// Encoded Longitude (bits 40-56): Per ICAO Doc 9871 §A.2.3.3.5
    ///
    /// Low-order 17 bits of 19-bit CPR-encoded longitude value for surface.
    /// Unambiguous range: 166.5 km (90 NM) for local decoding.
    ///
    /// Requires both even and odd frames for global decoding, or reference
    /// position within 166.5 km for local decoding.
    pub lon_cpr: u32,

    #[deku(skip, default = "None")]
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Decoded Latitude in decimal degrees, if available
    pub latitude: Option<f64>,

    #[deku(skip, default = "None")]
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Decoded Longitude in decimal degrees, if available
    pub longitude: Option<f64>,
}

/**
 * Decode ground speed from 7-bit movement field (bits 6-12)
 *
 * Per ICAO Doc 9871 §A.2.3.3.1 Table: Movement field encoding
 *
 * Non-linear encoding optimized for ground operations, providing higher
 * precision at lower speeds where accurate taxi/parking positioning matters.
 *
 * Encoding ranges (speeds in knots):
 * | Code    | Meaning                          | Quantization (LSB)  |
 * |---------|----------------------------------|---------------------|
 * | 0       | No information available         | N/A                 |
 * | 1       | Aircraft stopped (< 0.125 kt)    | 0 kt                |
 * | 2-8     | [0.125, 1.0) kt                  | 0.125 kt steps      |
 * | 9-12    | [1.0, 2.0) kt                    | 0.25 kt steps       |
 * | 13-38   | [2.0, 15.0) kt                   | 0.5 kt steps        |
 * | 39-93   | [15.0, 70.0) kt                  | 1.0 kt steps        |
 * | 94-108  | [70.0, 100.0) kt                 | 2.0 kt steps        |
 * | 109-123 | [100.0, 175.0) kt                | 5.0 kt steps        |
 * | 124     | ≥ 175 kt                         | 175 kt              |
 * | 125-127 | Reserved                         | N/A (returns None)  |
 *
 * Examples:
 * - mov=1: 0.0 kt (stopped)
 * - mov=2: 0.125 kt (0.125 + (2-2)*0.125)
 * - mov=39: 15.0 kt (15 + (39-39)*1.0)
 * - mov=124: 175.0 kt (≥175 kt threshold)
 *
 * Returns:
 * - Some(speed): Ground speed in knots
 * - None: No information available (mov=0) or reserved (mov≥125)
 */
fn read_groundspeed<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<f64>, DekuError> {
    let mov = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(7)),
    )?;
    let value = match mov {
        0 => None,
        1 => Some(0.),
        2..=8 => Some(0.125 + (mov - 2) as f64 * 0.125),
        9..=12 => Some(1. + (mov - 9) as f64 * 0.25),
        13..=38 => Some(2. + (mov - 13) as f64 * 0.5),
        39..=93 => Some(15. + (mov - 39) as f64 * 1.),
        94..=108 => Some(70. + (mov - 94) as f64 * 2.),
        109..=123 => Some(100. + (mov - 109) as f64 * 5.),
        124 => Some(175.),
        125..=u8::MAX => None, // Reserved
    };
    debug!("Groundspeed value: {:?}", value);
    Ok(value)
}

#[derive(Debug, PartialEq, DekuRead, Copy, Clone)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "1")]
pub enum StatusForGroundTrack {
    Invalid = 0,
    Valid = 1,
}

impl fmt::Display for SurfacePosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  Surface position (BDS 0,6)")?;
        let groundspeed = self
            .groundspeed
            .map_or_else(|| "None".to_string(), |gs| format!("{gs} kts"));
        let track = self
            .track
            .map_or_else(|| "None".to_string(), |track| format!("{track}°"));
        writeln!(f, "  Groundspeed:   {groundspeed}")?;
        writeln!(f, "  Track angle:   {track}")?;
        writeln!(f, "  CPR parity:    {}", self.parity)?;
        writeln!(f, "  CPR latitude:  ({})", self.lat_cpr)?;
        writeln!(f, "  CPR longitude: ({})", self.lon_cpr)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_surface_position() {
        tracing_subscriber::fmt::init();
        let bytes = hex!("8c4841753a9a153237aef0f275be");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS06 {
                inner:
                    SurfacePosition {
                        track, groundspeed, ..
                    },
                ..
            } = adsb_msg.message
            {
                assert_eq!(track, Some(92.8125));
                assert_eq!(groundspeed, Some(17.));
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_format() {
        let bytes = hex!("8c4841753a9a153237aef0f275be");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        assert_eq!(
            format!("{msg}"),
            r#" DF17. Extended Squitter
  Address:       484175
  Air/Ground:    ground
  Surface position (BDS 0,6)
  Groundspeed:   17 kts
  Track angle:   92.8125°
  CPR parity:    odd
  CPR latitude:  (39195)
  CPR longitude: (110320)
"#
        )
    }

    // Corner case tests for movement field (groundspeed) encoding
    // These tests validate the fix for BDS 0,6 ground speed formula bug
    // where movement codes 13-38 were incorrectly using 0.25 kt steps instead of 0.5 kt

    #[test]
    fn test_movement_2_15kt_range() {
        // Real message with groundspeed 8.0 kt (movement code 25 in 13-38 range)
        // This validates the 0.5 kt step fix for codes 13-38
        // Note: Original JSONL showed 5.0 kt (buggy decode with 0.25 kt steps)
        // Correct decode with 0.5 kt steps: 2.0 + (25-13)*0.5 = 8.0 kt
        let bytes = hex!("8c3461cf399d6059814ea81483a9");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { groundspeed, .. },
                ..
            } = adsb_msg.message
            {
                assert_eq!(groundspeed, Some(8.0));
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_movement_4_75kt() {
        // Real message with groundspeed 7.5 kt (movement code 24 in 13-38 range)
        // Tests movement code 13-38 range with 0.5 kt steps
        // Note: Original JSONL showed 4.75 kt (buggy decode with 0.25 kt steps)
        // Correct decode with 0.5 kt steps: 2.0 + (24-13)*0.5 = 7.5 kt
        let bytes = hex!("8c3461cf398d60597b4ea434c4d7");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { groundspeed, .. },
                ..
            } = adsb_msg.message
            {
                assert_eq!(groundspeed, Some(7.5));
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_movement_stopped() {
        // Real message with groundspeed 0.0 kt (movement code = 1, aircraft stopped)
        let bytes = hex!("903a33ff40100858d34ff3cce976");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterTisB { cf, .. } = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { groundspeed, .. },
                ..
            } = cf.me
            {
                assert_eq!(groundspeed, Some(0.0));
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_movement_no_info() {
        // Real message with movement code 0 (no information available)
        // Movement field = 0 should return None
        let bytes = hex!("8c3944f8400002acb23cda192b95");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { groundspeed, .. },
                ..
            } = adsb_msg.message
            {
                // Movement code 0 should return None
                assert_eq!(groundspeed, None);
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_movement_1_2kt_range() {
        // Real message with movement codes 9-12 (1.0-2.0 kt range with 0.25 kt steps)
        // Movement code 9 = 1.0 kt
        let bytes = hex!("8c394c0f389b1667e947db7bb8bc");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { groundspeed, .. },
                ..
            } = adsb_msg.message
            {
                assert_eq!(groundspeed, Some(1.0));
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_movement_15_70kt_range() {
        // Real message with movement codes 39-93 (15-70 kt range with 1.0 kt steps)
        // Movement code 39 = 15.0 kt (lower bound)
        let bytes = hex!("8c3461cf3a7f3059c94e5bf4e169");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { groundspeed, .. },
                ..
            } = adsb_msg.message
            {
                assert_eq!(groundspeed, Some(15.0));
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_movement_70_100kt_range() {
        // Real message with movement codes 94-108 (70-100 kt range with 2.0 kt steps)
        // Movement code 94 = 70.0 kt (lower bound)
        let bytes = hex!("8c3950cf3dede47bac304d3b5122");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { groundspeed, .. },
                ..
            } = adsb_msg.message
            {
                assert_eq!(groundspeed, Some(70.0));
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_movement_100_175kt_range() {
        // Real message with movement codes 109-123 (100-175 kt range with 5.0 kt steps)
        // Movement code 109 = 100.0 kt (lower bound)
        let bytes = hex!("8c3933203edde47b9e2ffa5e77b8");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { groundspeed, .. },
                ..
            } = adsb_msg.message
            {
                assert_eq!(groundspeed, Some(100.0));
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_movement_175kt_plus() {
        // Real message with movement code 124 (≥175 kt)
        let bytes = hex!("8d3933203fcde2a84e39e1c6c5bc");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { groundspeed, .. },
                ..
            } = adsb_msg.message
            {
                assert_eq!(groundspeed, Some(175.0));
                return;
            }
        }
        unreachable!();
    }

    // TODO: Add test for movement codes 125-127 (reserved, should return None)
    // This requires either finding a real message or computing valid CRC for synthetic message
    //
    // #[test]
    // fn test_movement_reserved() {
    //     // Movement codes 125-127 are reserved and should return None
    //     let bytes = hex!("...");
    //     let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
    //     ...
    //     assert_eq!(groundspeed, None);
    // }

    #[test]
    fn test_track_invalid() {
        // Test track_status = 0 (track invalid, should return None)
        let bytes = hex!("903a33ff40100858d34ff3cce976");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterTisB { cf, .. } = msg.df {
            if let ME::BDS06 {
                inner: SurfacePosition { track, .. },
                ..
            } = cf.me
            {
                // Track status = 0, so track should be None
                assert_eq!(track, None);
                return;
            }
        }
        unreachable!();
    }
}
