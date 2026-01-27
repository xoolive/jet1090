use deku::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;
use tracing::{debug, trace};

/**
 * ## Aircraft Identification and Category (BDS 0,8)
 *
 * Extended squitter message providing aircraft callsign and wake vortex category.  
 * Per ICAO Doc 9871 Table A-2-8: BDS code 0,8 — Extended squitter aircraft
 * identification and category
 *
 * Message Structure (56 bits):
 * | TC  | CA  | C1  | C2  | C3  | C4  | C5  | C6  | C7  | C8  |
 * | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
 * | 5   | 3   | 6   | 6   | 6   | 6   | 6   | 6   | 6   | 6   |
 *
 * Field Encoding per ICAO Doc 9871 §A.2.3.4:
 * - TC (bits 1-5): Format Type Code, determines category set
 *   * TC=1 (D): Reserved
 *   * TC=2 (C): Surface vehicles and obstructions (category set C)
 *   * TC=3 (B): Aircraft without engines - gliders, ultralights, etc. (set B)
 *   * TC=4 (A): Normal aircraft (category set A)
 * - CA (bits 6-8): Aircraft/Vehicle Category (3 bits)
 *   * Combined with TC to determine wake vortex category
 * - C1-C8 (bits 9-56): Eight 6-bit characters encoding callsign
 *
 * Character Encoding per Annex 10 Vol IV Table 3-8:
 * - 6-bit subset of IA-5 (International Alphabet #5)
 * - Supports: A-Z (letters), 0-9 (digits), space (0x20)
 * - Special character '#' at position 0
 * - Trailing spaces typically omitted from callsign string
 * - Callsign should be flight plan identification or aircraft registration
 *
 * Wake Vortex Categories (TC=4, Category Set A):
 * - CA=0: No category information
 * - CA=1: Light (< 7,031 kg / 15,500 lbs) → ICAO WTC L
 * - CA=2: Medium 1 (7,031 to 34,019 kg) → ICAO WTC M
 * - CA=3: Medium 2 (34,019 to 136,078 kg) → ICAO WTC M
 * - CA=4: High vortex aircraft
 * - CA=5: Heavy (> 136,078 kg / 300,000 lbs) → ICAO WTC H/J
 * - CA=6: High performance (>5g accel, >400 kt)
 * - CA=7: Rotorcraft
 *
 * Note: ADS-B wake vortex categories differ from ICAO WTC definitions.
 */

#[derive(Debug, PartialEq, DekuRead, Serialize, Deserialize, Clone)]
#[deku(ctx = "id: u8")]
pub struct AircraftIdentification {
    /// Type Code Category (bits 1-5): Per ICAO Doc 9871 Table A-2-8
    #[serde(skip)]
    #[deku(skip, default = "Typecode::try_from(id)?")]
    pub tc: Typecode,

    /// Aircraft/Vehicle Category (bits 6-8): Per ICAO Doc 9871 Table A-2-8  
    /// 3-bit field combined with TC to determine wake vortex category.  
    /// See wake_vortex() function for complete category mapping.
    #[deku(bits = "3")]
    #[serde(skip)]
    pub ca: u8,

    /// Wake Vortex Category: Derived from TC and CA fields  
    /// Per ICAO Doc 9871 Table A-2-8 category sets A, B, C, D.  
    /// Note: ADS-B categories differ from ICAO Wake Turbulence Category (WTC).
    #[deku(reader = "wake_vortex(*tc, *ca)")]
    pub wake_vortex: WakeVortex,

    /// Callsign (bits 9-56): Aircraft identification per ICAO Doc 9871 Table A-2-32  
    /// Eight 6-bit characters using IA-5 subset (Annex 10 Vol IV Table 3-8).
    ///
    /// Character set: A-Z, 0-9, space (0x20), and '#' (position 0).  
    /// Should contain flight plan identification or aircraft registration.  
    /// Trailing spaces are typically omitted from the decoded string.
    #[deku(reader = "callsign_read(deku::reader)")]
    pub callsign: String,
}

#[derive(Debug, PartialEq, Copy, Clone, Default)]
/// Type Code Category (bits 1-5): Per ICAO Doc 9871 Table A-2-8  
/// Determines the category set:
///   - D (TC=1): Reserved
///   - C (TC=2): Surface vehicles and obstructions
///   - B (TC=3): Aircraft without engines (gliders, ultralights, etc.)
///   - A (TC=4): Normal aircraft (most common)
pub enum Typecode {
    /// Reserved
    D = 1,
    /// Ground vehicles
    C = 2,
    /// Without an engine (glider, hangglider, etc.)
    B = 3,
    /// Aircraft
    #[default]
    A = 4,
}

impl fmt::Display for Typecode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::D => "D: Reserved",
                Self::C => "C: Surface vehicles",
                Self::B => "B: Without an engine",
                Self::A => "A: Aircraft",
            }
        )
    }
}

use std::convert::TryFrom;

impl TryFrom<u8> for Typecode {
    type Error = DekuError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::D),
            2 => Ok(Self::C),
            3 => Ok(Self::B),
            4 => Ok(Self::A),
            _ => Err(DekuError::InvalidParam(
                "Invalid value for Typecode".into(),
            )),
        }
    }
}

/**
 * Decode wake vortex category from Type Code (TC) and Category (CA)
 *
 * Per ICAO Doc 9871 Table A-2-8: Aircraft/vehicle category coding
 *
 * The CA value (3 bits) combined with TC value determines wake vortex category.
 * Four category sets are defined:
 *
 * **Set A (TC=4)**: Normal aircraft
 * - CA=0: No information
 * - CA=1: Light (< 7,031 kg / 15,500 lbs)
 * - CA=2: Medium 1 (7,031 to 34,019 kg / 15,500 to 75,000 lbs)
 * - CA=3: Medium 2 (34,019 to 136,078 kg / 75,000 to 300,000 lbs)
 * - CA=4: High vortex aircraft
 * - CA=5: Heavy (> 136,078 kg / 300,000 lbs)
 * - CA=6: High performance (>5g acceleration and >400 kt speed)
 * - CA=7: Rotorcraft
 *
 * **Set B (TC=3)**: Aircraft without engines
 * - CA=0: No information
 * - CA=1: Glider/sailplane
 * - CA=2: Lighter-than-air
 * - CA=3: Parachutist/skydiver
 * - CA=4: Ultralight/hang-glider/paraglider
 * - CA=5: Reserved
 * - CA=6: Unmanned aerial vehicle
 * - CA=7: Space/transatmospheric vehicle
 *
 * **Set C (TC=2)**: Surface vehicles
 * - CA=0: No information
 * - CA=1: Surface emergency vehicle
 * - CA=2: (Reserved/Obstruction)
 * - CA=3: Surface service vehicle
 * - CA=4-7: Fixed ground or tethered obstruction
 *
 * **Set D (TC=1)**: Reserved
 *
 * Note: ADS-B wake vortex categories differ from ICAO Wake Turbulence
 * Category (WTC) definitions:
 * - ICAO WTC L ≈ ADS-B (TC=4, CA=1)
 * - ICAO WTC M ≈ ADS-B (TC=4, CA=2 or CA=3)
 * - ICAO WTC H/J ≈ ADS-B (TC=4, CA=5)
 */
#[derive(Debug, PartialEq, Serialize, Deserialize, Copy, Clone)]
pub enum WakeVortex {
    Reserved,

    // Category C
    #[serde(rename = "n/a")]
    /// No category information
    NoInformation,
    #[serde(rename = "Surface emergency vehicle")]
    /// Surface emergency vehicle
    EmergencyVehicle,
    #[serde(rename = "Surface service vehicle")]
    /// Surface service vehicle
    ServiceVehicle,
    /// Ground obstruction
    Obstruction,

    // Category B
    /// Glider, sailplane
    Glider,
    #[serde(rename = "Lighter than air")]
    /// Lighter than air
    Lighter,
    /// Parachutist, Skydiver
    Parachutist,
    /// Ultralight, hang-glider, paraglider
    Ultralight,
    #[serde(rename = "UAV")]
    /// Unmanned Air Vehicle
    Unmanned,
    /// Space or transatmospheric vehicle
    Space,

    // Category A
    #[serde(rename = "<7000kg")]
    /// Light (< 7,000 kg)
    Light,
    #[serde(rename = "<34,000kg")]
    /// Medium1 (7,000kg to 34,000kg)
    Medium1,
    #[serde(rename = "<136,000kg")]
    /// Medium2 (34,000kg to 136,000kg)
    Medium2,
    #[serde(rename = "High vortex")]
    /// High vortex aircraft
    HighVortex,
    /// Heavy (> 136,000 kg)
    Heavy,
    #[serde(rename = "High performance")]
    /// High performance (>5 g acceleration and >400 kt)
    HighPerformance,
    /// Rotorcraft
    Rotorcraft,
}

impl fmt::Display for WakeVortex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let string = match &self {
            Self::Reserved => "Reserved",
            Self::NoInformation => "No category information",
            Self::EmergencyVehicle => "Surface Emergency Vehicle",
            Self::ServiceVehicle => "Surface Service Vehicle",
            Self::Obstruction => "Ground Obstruction",
            Self::Glider => "Glider, sailplane",
            Self::Lighter => "Lighter than air",
            Self::Parachutist => "Parachutist, Skydiver",
            Self::Ultralight => "Ultralight, hang-glider, paraglider",
            Self::Unmanned => "Unmanned Air Vehicle",
            Self::Space => "Space or transatmospheric vehicle",
            Self::Light => "Light (less than 7000 kg)",
            Self::Medium1 => "Medium 1 (between 7000 kg and 34000 kg)",
            Self::Medium2 => "Medium 2 (between 34000 kg to 136000 kg)",
            Self::HighVortex => "High vortex aircraft",
            Self::Heavy => "Heavy (larger than 136000 kg)",
            Self::HighPerformance => {
                "High performance (>5 g acceleration) and high speed (>400 kt)"
            }
            Self::Rotorcraft => "Rotorcraft",
        };
        write!(f, "{string}")?;
        Ok(())
    }
}

/// Decode wake vortex category from Type Code (TC) and Category (CA)
pub fn wake_vortex(tc: Typecode, ca: u8) -> Result<WakeVortex, DekuError> {
    let wake_vortex = match (tc, ca) {
        (Typecode::D, _) => WakeVortex::Reserved,
        (_, 0) => WakeVortex::NoInformation,
        (Typecode::C, 1) => WakeVortex::EmergencyVehicle,
        (Typecode::C, 3) => WakeVortex::ServiceVehicle,
        (Typecode::C, _) => WakeVortex::Obstruction,
        (Typecode::B, 1) => WakeVortex::Glider,
        (Typecode::B, 2) => WakeVortex::Lighter,
        (Typecode::B, 3) => WakeVortex::Parachutist,
        (Typecode::B, 4) => WakeVortex::Ultralight,
        (Typecode::B, 5) => WakeVortex::Reserved,
        (Typecode::B, 6) => WakeVortex::Unmanned,
        (Typecode::B, 7) => WakeVortex::Space,
        (Typecode::A, 1) => WakeVortex::Light,
        (Typecode::A, 2) => WakeVortex::Medium1,
        (Typecode::A, 3) => WakeVortex::Medium2,
        (Typecode::A, 4) => WakeVortex::HighVortex,
        (Typecode::A, 5) => WakeVortex::Heavy,
        (Typecode::A, 6) => WakeVortex::HighPerformance,
        (Typecode::A, 7) => WakeVortex::Rotorcraft,
        _ => WakeVortex::Reserved, // only 3 bits anyway
    };
    Ok(wake_vortex)
}

/// Character lookup table for 6-bit IA-5 subset encoding
///
/// Per Annex 10 Volume IV Table 3-8: Character coding for aircraft identification
///
/// 6-bit encoding (64 possible values):
/// - 0x00 (000000): '#'  
/// - 0x01-0x1A: 'A'-'Z' (letters)
/// - 0x20 (100000): ' ' (space, used for padding)
/// - 0x30-0x39: '0'-'9' (digits)
/// - Other positions: '#' (invalid/reserved)
///
/// This is a subset of the IA-5 (International Alphabet #5) character set,
/// optimized for aircraft callsign transmission.
const CHAR_LOOKUP: &[u8; 64] =
    b"#ABCDEFGHIJKLMNOPQRSTUVWXYZ##### ###############0123456789######";

/// Decode 8-character callsign from 48 bits (8 × 6-bit characters)
///
/// Per ICAO Doc 9871 Table A-2-32 and Annex 10 Vol IV Table 3-8
///
/// Each character is encoded in 6 bits using IA-5 subset:
/// - Bits 9-14: Character 1 (MSB)
/// - Bits 15-20: Character 2
/// - Bits 21-26: Character 3
/// - Bits 27-32: Character 4
/// - Bits 33-38: Character 5
/// - Bits 39-44: Character 6
/// - Bits 45-50: Character 7
/// - Bits 51-56: Character 8 (LSB)
///
/// Trailing spaces (0x20) are omitted from the returned string.
///
/// Callsign content per ICAO Doc 9871:
/// - Should be aircraft identification from flight plan
/// - If no flight plan, use aircraft registration marking
///
/// Returns: Decoded callsign string (1-8 characters, trailing spaces removed)
pub fn callsign_read<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<String, DekuError> {
    let mut chars = vec![];
    for _ in 1..=8 {
        let c = u8::from_reader_with_ctx(reader, deku::ctx::BitSize(6))?;
        trace!("Reading letter {}", CHAR_LOOKUP[c as usize] as char);
        if c != 32 {
            chars.push(c);
        }
    }
    let encoded = chars
        .into_iter()
        .map(|b| CHAR_LOOKUP[b as usize] as char)
        .collect::<String>();

    debug!("Reading callsign {}", encoded);
    Ok(encoded)
}

impl fmt::Display for AircraftIdentification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  Aircraft identification and category (BDS 0,8)")?;
        writeln!(f, "  Callsign:      {}", &self.callsign)?;
        writeln!(f, "  Category:      {}", &self.wake_vortex)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Typecode;
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_callsign() {
        let bytes = hex!("8d406b902015a678d4d220aa4bda");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS08 {
                inner:
                    AircraftIdentification {
                        tc,
                        ca,
                        callsign,
                        wake_vortex,
                    },
                ..
            } = adsb_msg.message
            {
                // Check type code and category directly
                assert_eq!(tc, Typecode::A);
                assert_eq!(ca, 0);
                assert_eq!(format!("{wake_vortex}"), "No category information");
                assert_eq!(callsign, "EZY85MH");
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_format() {
        let bytes = hex!("8d406b902015a678d4d220aa4bda");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        assert_eq!(
            format!("{msg}"),
            r#" DF17. Extended Squitter
  Address:       406b90
  Air/Ground:    airborne
  Aircraft identification and category (BDS 0,8)
  Callsign:      EZY85MH
  Category:      No category information
"#
        )
    }
}
