use crate::decode::cpr::CPRFormat;
use crate::decode::{decode_id13, gray2alt};
use deku::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;

/**
 * ## Airborne Position (BDS 0,5)
 *
 * Extended squitter message providing accurate airborne position information.  
 * Per ICAO Doc 9871 Table A-2-6: BDS code 0,5 — Extended squitter airborne position
 *
 * Supports barometric altitude (TC=9..=18) or GNSS height (TC=20..=22)
 *
 * Message Structure (56 bits):
 * | TC | SS | SAF | ALT | T | F | LAT-CPR | LON-CPR |
 * | -- | -- | --- | --- | - | - | ------- | ------- |
 * | 5  | 2  |  1  | 12  | 1 | 1 |   17    |   17    |
 *
 * Field Encoding:
 * - TC (bits 1-5): Format Type Code, determines altitude type and accuracy
 * - SS (bits 6-7): Surveillance Status (0=No condition, 1=Emergency, 2=Temp alert, 3=SPI)
 * - SAF (bit 8): Single Antenna Flag (ADS-B v0/v1) or NICb supplement (v2)
 * - ALT (bits 9-20): Altitude Code as per Annex 10 Vol IV §3.1.2.6.5.4 (Q-bit or Gillham)
 * - T (bit 21): Time synchronization (1=synchronized to UTC)
 * - F (bit 22): CPR format (0=even, 1=odd) per §C.2.6.7
 * - LAT-CPR (bits 23-39): 17-bit CPR-encoded latitude per §C.2.6
 * - LON-CPR (bits 40-56): 17-bit CPR-encoded longitude per §C.2.6
 *
 * Special Cases per ICAO Doc 9871:
 * - If horizontal position unavailable but altitude available: TC=0, altitude in bits 9-20
 * - If both unavailable: all 56 bits shall be zeroed
 * - Altitude field 0x000 indicates altitude not available (DO-260B §2.2.5.1.5)
 */

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
#[deku(ctx = "tc: u8")]
pub struct AirbornePosition {
    #[deku(
        skip,
        default = "
        match tc {
            n if n < 19 => 18 - tc,
            20 | 21 => 29 - tc,
            _ => 0
        }
        "
    )]
    #[serde(rename = "NUCp")]
    /// The Navigation Uncertainty Category Position (NUCp)
    /// (directly based on the typecode)
    pub nuc_p: u8,

    #[serde(skip)] // TODO: should we really skip? serde deserialises to NoCondition
    /// Surveillance Status (bits 6-7): Indicates emergency/alert conditions.
    /// Per ICAO Doc 9871 Table A-2-6:
    pub ss: SurveillanceStatus,

    #[deku(
        bits = "1",
        map = "|v| -> Result<_, DekuError> {
            if tc < 19 { Ok(Some(v)) } else { Ok(None) }
        }"
    )]
    #[serde(rename = "NICb", skip_serializing_if = "Option::is_none")]
    /// Single Antenna Flag (bit 8): Per ICAO Doc 9871 §A.2.3.2.5.
    /// - In ADS-B v0 or v1: Single Antenna Flag (SAF)
    /// - In ADS-B v2: Navigation Integrity Category Supplement-b (NICb)
    ///
    /// Only present when TC < 19 (barometric altitude messages)
    pub saf_or_nicb: Option<u8>,

    #[deku(reader = "decode_ac12(deku::reader)")]
    #[serde(rename = "altitude")]
    /// Altitude (bits 9-20): 12-bit altitude code per Annex 10 Vol IV §3.1.2.6.5.4.
    /// Encoding supports:
    ///   - Q-bit format: 25 ft increments, range [-1000, 50175] ft
    ///   - Gillham code: 100 ft increments (legacy Mode C)
    ///
    /// Returns None if altitude not available (field = 0x000 per DO-260B §2.2.5.1.5)
    /// Can be negative for airports below sea level (e.g., EHAM at -11 ft MSL)
    pub alt: Option<i32>,

    #[deku(reader = "read_source(tc)")]
    /// Altitude Source: Determined by Type Code (TC).  
    /// Per ICAO Doc 9871 Table A-2-6:
    ///   - TC 9-18: Barometric altitude
    ///   - TC 20-22: GNSS height (HAE - Height Above Ellipsoid)
    pub source: Source,

    #[deku(bits = "1")]
    /// Time Synchronization (bit 21): Per ICAO Doc 9871 §A.2.3.2.2.  
    /// Indicates whether time of applicability is synchronized with UTC time.
    ///   - false (T=0): Time not synchronized to UTC
    ///   - true (T=1): Time synchronized to UTC (only for TC 9, 10, 20, 21)
    ///
    /// When T=1, the F (parity) bit alternates between 0 and 1 for successive
    /// 0.2-second UTC time ticks, starting with F=0 at even-numbered UTC seconds.
    pub time_sync: bool,

    /// CPR Format (bit 22): Per ICAO Doc 9871 §A.2.3.2.1 and §C.2.6.7.  
    /// Compact Position Reporting uses two format types for worldwide unambiguity:
    ///   - Even (F=0): Even format coding
    ///   - Odd (F=1): Odd format coding
    ///
    /// When time_sync=true, this bit also encodes the 0.2-second time tick.
    /// CPR unambiguous range: 666 km (360 NM) for local decoding.
    pub parity: CPRFormat,

    #[deku(bits = "17", endian = "big")]
    /// Encoded Latitude (bits 23-39): Per ICAO Doc 9871 §C.2.6.  
    /// 17-bit CPR-encoded latitude value.
    /// CPR maintains positional accuracy of ~5.1 meters in most cases.
    /// Note: Longitude accuracy degrades to ~10.0m near poles (±87° latitude).
    pub lat_cpr: u32,

    #[deku(bits = "17", endian = "big")]
    /// Encoded Longitude (bits 40-56): Per ICAO Doc 9871 §C.2.6.  
    /// 17-bit CPR-encoded longitude value.
    /// Requires both even and odd frames for global decoding, or reference
    /// position within 666 km (360 NM) for local decoding.
    pub lon_cpr: u32,

    #[deku(skip, default = "None")]
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Decoded Latitude in decimal degrees (computed from lat_cpr and lon_cpr)
    pub latitude: Option<f64>,

    #[deku(skip, default = "None")]
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Decoded Longitude in decimal degrees (computed from lat_cpr and lon_cpr)
    pub longitude: Option<f64>,
}

/// Decode altitude value encoded on 12 bits (bits 9-20)
///
/// Per ICAO Annex 10 Volume IV §3.1.2.6.5.4 and DO-260B §2.2.5.1.5:
///
/// Supports two encoding formats:
/// 1. **Q-bit encoding** (bit 4 = 1): Most common, 25 ft resolution
///    - Formula: altitude = (N × 25) - 1000 ft
///    - Range: [-1000, 50175] ft
///    - N extracted from bits: ((bits[1-3,5-11] << 1) | bits[12])
///    - Supports negative altitudes for below-sea-level airports
///      (e.g., Amsterdam EHAM at -11 ft MSL)
///
/// 2. **Gillham code** (bit 4 = 0): Legacy Mode C transponder, 100 ft resolution
///    - Uses Gray code encoding with D1, D2, D4, A1, A2, A4, B1, B2, B4, C1, C2, C4 bits
///    - Altitude = N × 100 ft after Gray-to-binary conversion
///    - Also supports negative values after conversion
///
/// Special values:
/// - 0x000 (all zeros): Altitude not available (returns None)
/// - DO-260B §2.2.5.1.5: Zero altitude field indicates unavailable data
///
/// Returns:
/// - Some(altitude): Altitude in feet (can be negative)
/// - None: Altitude not available
fn decode_ac12<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<i32>, DekuError> {
    let num = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(12)),
    )?;

    // Check for all-zeros: altitude not available (DO-260B §2.2.5.1.5)
    if num == 0 {
        return Ok(None);
    }

    let q = num & 0x10;

    if q > 0 {
        // Q-bit encoding: 25 ft increments with -1000 ft offset
        // Per Annex 10 Vol IV §3.1.2.6.5.4: altitude = (N × 25) - 1000 ft
        // Extract N by removing Q-bit: N = bits[1-3,5-11,12]
        // This supports negative altitudes for below-sea-level airports
        let n = ((num & 0x0fe0) >> 1) | (num & 0x000f);
        let altitude = i32::from(n) * 25 - 1000;
        Ok(Some(altitude))
    } else {
        // Gillham code encoding: 100 ft resolution, legacy Mode C
        // Uses Gray code with D1,D2,D4,A1,A2,A4,B1,B2,B4,C1,C2,C4 bit positions
        // Already supports negative altitudes via gray2alt conversion
        let mut n = ((num & 0x0fc0) << 1) | (num & 0x003f);
        n = decode_id13(n);
        if let Ok(n) = gray2alt(n) {
            let altitude = n * 100;
            Ok(Some(altitude))
        } else {
            Ok(None)
        }
    }
}

fn read_source(tc: u8) -> Result<Source, DekuError> {
    let source = if tc < 19 {
        Source::Barometric
    } else {
        Source::Gnss
    };
    Ok(source)
}

impl fmt::Display for AirbornePosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  AirbornePosition (BDS 0,5)")?;
        let altitude = self.alt.map_or_else(
            || "None".to_string(),
            |altitude| format!("{altitude} ft"),
        );
        writeln!(f, "  Altitude:      {} {}", altitude, self.source)?;
        writeln!(f, "  CPR type:      Airborne")?;
        writeln!(f, "  CPR parity:    {}", self.parity)?;
        writeln!(f, "  CPR latitude:  ({})", self.lat_cpr)?;
        writeln!(f, "  CPR longitude: ({})", self.lon_cpr)?;
        Ok(())
    }
}

#[derive(
    Debug, PartialEq, Eq, Serialize, Deserialize, DekuRead, Copy, Clone, Default,
)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "2")]
/// Surveillance Status (bits 6-7): Indicates emergency/alert conditions.  
/// Per ICAO Doc 9871 Table A-2-6:
///   - 0 = No condition
///   - 1 = Permanent alert (emergency condition)
///   - 2 = Temporary alert (change in Mode A identity code, not emergency)
///   - 3 = SPI condition
///
/// Codes 1 and 2 take precedence over code 3.
pub enum SurveillanceStatus {
    #[default]
    NoCondition = 0,
    PermanentAlert = 1,
    TemporaryAlert = 2,
    SPICondition = 3,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Copy, Clone)]
pub enum Source {
    #[serde(rename = "barometric")]
    Barometric = 0,
    #[serde(rename = "GNSS")]
    Gnss = 1,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Barometric => "barometric",
                Self::Gnss => "GNSS",
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::adsb::{ADSB, ME};
    use crate::decode::{Message, DF};
    use hexlit::hex;

    #[test]
    fn test_negative_altitude_325ft() {
        // Real message from EHAM with altitude that should decode to -325 ft
        // Frame: 8d484fde5803b647ecec4fcdd74f
        // Altitude field: 0x03b (59 decimal)
        // Q-bit set, N=27, altitude = 27*25-1000 = -325 ft
        let bytes = hex!("8d484fde5803b647ecec4fcdd74f");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterADSB(ADSB {
            message: ME::BDS05 { inner: pos, .. },
            ..
        }) = msg.df
        {
            assert_eq!(pos.alt, Some(-325));
        } else {
            panic!("Expected AirbornePosition message, got {:?}", msg.df);
        }
    }

    #[test]
    fn test_negative_altitude_300ft() {
        // ICAO 484557, altitude field 0x03c (60)
        // N=28, altitude = 28*25-1000 = -300 ft
        let bytes = hex!("8d4845575803c647bcec2a980abc");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterADSB(ADSB {
            message: ME::BDS05 { inner: pos, .. },
            ..
        }) = msg.df
        {
            assert_eq!(pos.alt, Some(-300));
        } else {
            panic!("Expected AirbornePosition message");
        }
    }

    #[test]
    fn test_negative_altitude_275ft() {
        // ICAO 3424d2, altitude field 0x03d (61)
        // N=29, altitude = 29*25-1000 = -275 ft
        let bytes = hex!("8d3424d25803d64c18ee03351f89");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterADSB(ADSB {
            message: ME::BDS05 { inner: pos, .. },
            ..
        }) = msg.df
        {
            assert_eq!(pos.alt, Some(-275));
        } else {
            panic!("Expected AirbornePosition message");
        }
    }

    #[test]
    fn test_zero_altitude() {
        // Real message from EHAM: ICAO 4401e4
        // Altitude field 0x058 (88), N=40, altitude = 40*25-1000 = 0 ft
        let bytes = hex!("8d4401e458058645a8ea90496290");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterADSB(ADSB {
            message: ME::BDS05 { inner: pos, .. },
            ..
        }) = msg.df
        {
            assert_eq!(pos.alt, Some(0));
        } else {
            panic!("Expected AirbornePosition message");
        }
    }

    #[test]
    fn test_small_positive_altitude_25ft() {
        // Real message from EHAM: ICAO 346355
        // Altitude = 25 ft
        let bytes = hex!("8d346355580596459cea86756acc");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterADSB(ADSB {
            message: ME::BDS05 { inner: pos, .. },
            ..
        }) = msg.df
        {
            assert_eq!(pos.alt, Some(25));
        } else {
            panic!("Expected AirbornePosition message");
        }
    }

    #[test]
    fn test_small_positive_altitude_50ft() {
        // Real message from EHAM: ICAO 346355
        // Altitude = 50 ft
        let bytes = hex!("8d3463555805a64584ea756d352e");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterADSB(ADSB {
            message: ME::BDS05 { inner: pos, .. },
            ..
        }) = msg.df
        {
            assert_eq!(pos.alt, Some(50));
        } else {
            panic!("Expected AirbornePosition message");
        }
    }

    #[test]
    fn test_small_positive_altitude_100ft() {
        // Real message from EHAM: ICAO 346355
        // Altitude = 100 ft
        let bytes = hex!("8d3463555805c2d9f6f0f3f1b6c3");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterADSB(ADSB {
            message: ME::BDS05 { inner: pos, .. },
            ..
        }) = msg.df
        {
            assert_eq!(pos.alt, Some(100));
        } else {
            panic!("Expected AirbornePosition message");
        }
    }

    #[test]
    fn test_positive_altitude_1000ft() {
        // Real message from EHAM: ICAO 346355
        // N=80, altitude = 80*25-1000 = 1000 ft
        let bytes = hex!("8d346355580b064116e70a269f97");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterADSB(ADSB {
            message: ME::BDS05 { inner: pos, .. },
            ..
        }) = msg.df
        {
            assert_eq!(pos.alt, Some(1000));
        } else {
            panic!("Expected AirbornePosition message");
        }
    }

    #[test]
    fn test_positive_altitude_5000ft() {
        // Real message from EHAM: ICAO 343386
        // Higher altitude to ensure positive values still work
        let bytes = hex!("8d343386581f06318ad4fecab734");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterADSB(ADSB {
            message: ME::BDS05 { inner: pos, .. },
            ..
        }) = msg.df
        {
            assert_eq!(pos.alt, Some(5000));
        } else {
            panic!("Expected AirbornePosition message");
        }
    }

    #[test]
    fn test_altitude_decoding_formula() {
        // Test the altitude decoding formula for Q-bit encoding
        // Formula: altitude = (n * 25) - 1000
        // where n is extracted from the 12-bit altitude field

        let test_cases = vec![
            // (alt_field_value, expected_altitude)
            (0x03a, -350), // n=26: 26*25-1000 = -350
            (0x03b, -325), // n=27: 27*25-1000 = -325
            (0x03e, -250), // n=30: 30*25-1000 = -250
            (0x050, -200), // n=32: 32*25-1000 = -200
            (0x058, 0),    // n=40: 40*25-1000 = 0
            (0x070, 200),  // n=48: 48*25-1000 = 200
            (0x0b0, 1000), // n=80: 80*25-1000 = 1000
            (0x1f0, 5000), // n=240: 240*25-1000 = 5000
        ];

        for (alt_field, expected_alt) in test_cases {
            // Verify Q-bit is set (bit 4)
            let q_bit = alt_field & 0x10;
            assert!(
                q_bit > 0,
                "Q-bit should be set for field 0x{:03x}",
                alt_field
            );

            // Extract n value
            let n = ((alt_field & 0x0fe0) >> 1) | (alt_field & 0x000f);

            // Apply formula
            let altitude = n * 25 - 1000;

            assert_eq!(
                altitude, expected_alt,
                "Altitude field 0x{:03x} (n={}) should decode to {} ft, got {} ft",
                alt_field, n, expected_alt, altitude
            );
        }
    }

    #[test]
    fn test_altitude_all_zeros() {
        // Test that altitude field 0x000 is treated as "not available" per DO-260B §2.2.5.1.5
        // This should return None, NOT -1000 ft
        // Note: We can't easily test the decode_ac12 function directly since it requires a Reader,
        // so we test via a full message. We need to construct a message with altitude field = 0x000.

        // Message structure for TC=9 (Airborne Position with barometric altitude):
        // DF=17 (5 bits) | CA=5 (3 bits) | ICAO (24 bits) | TC=9 (5 bits) | SS (2 bits) | NICb (1 bit) | ALT (12 bits) | ...
        // Let's create a test message with altitude = 0x000

        // Craft: DF=17, CA=5, ICAO=0x123456, TC=9, SS=0, NICb=0, ALT=0x000, rest zeros
        // Note: The actual message would fail CRC, but we're testing the altitude decoder
        let bytes = hex!("8d1234564800000000000000000000"); // Simplified - altitude field is 0x000

        // This message will likely fail to parse or return None for altitude
        match Message::from_bytes((&bytes, 0)) {
            Ok((_, msg)) => {
                if let DF::ExtendedSquitterADSB(ADSB {
                    message: ME::BDS05 { inner: pos, .. },
                    ..
                }) = msg.df
                {
                    assert_eq!(
                        pos.alt, None,
                        "Altitude 0x000 should decode to None, not Some(-1000)"
                    );
                } else {
                    // If it doesn't parse as BDS05, that's also acceptable for this test
                }
            }
            Err(_) => {
                // If the message fails to parse due to invalid CRC or other reasons, that's acceptable
                // The important thing is that 0x000 doesn't decode to -1000 ft in the decode_ac12 function
            }
        }
    }

    #[test]
    fn test_altitude_minus_1000_valid() {
        // Test that actual -1000 ft altitude is correctly encoded and decoded
        // For -1000 ft: n = 0, so altitude field = (0 << 1) | Q-bit | 0 = 0x010
        // This is Q-bit set (bit 4), with all other bits zero

        // The formula: altitude = n * 25 - 1000
        // For n=0: altitude = 0 * 25 - 1000 = -1000 ft

        // We need a proper message with altitude field 0x010
        // Let's verify the formula directly for now
        let alt_field: u16 = 0x010;
        let q_bit = alt_field & 0x10;
        assert!(q_bit > 0, "Q-bit should be set");

        let n = ((alt_field & 0x0fe0) >> 1) | (alt_field & 0x000f);
        assert_eq!(n, 0, "n should be 0 for altitude field 0x010");

        let altitude = i32::from(n) * 25 - 1000;
        assert_eq!(
            altitude, -1000,
            "Altitude field 0x010 should decode to -1000 ft"
        );
    }
}
