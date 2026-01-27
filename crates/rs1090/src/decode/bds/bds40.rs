#![allow(clippy::suspicious_else_formatting)]

use deku::prelude::*;
use serde::{Deserialize, Serialize};

/**
 * ## Selected Vertical Intention (BDS 4,0)
 *
 * Comm-B message providing aircraft's current vertical flight intentions.  
 * Per ICAO Doc 9871 Table A-2-64: BDS code 4,0 — Selected vertical intention
 *
 * Designed for air traffic control to verify aircraft compliance with altitude
 * commands and improve conflict detection through knowledge of short-term intent.
 *
 * Message Structure (56 bits):
 * | MCP ALT | FMS ALT | QNH    | RSVD | MODE | RSVD | SRC |
 * |---------|---------|--------|------|------|------|-----|
 * | 1+12    | 1+12    | 1+12   | 8    | 1+3  | 2    | 1+2 |
 *
 * Field Encoding per ICAO Doc 9871:
 *
 * **MCP/FCU Selected Altitude** (bits 1-13):
 * - Bit 1: Status (0=invalid, 1=valid)
 * - Bits 2-13: 12-bit altitude value
 *   * MSB = 32,768 ft
 *   * LSB = 16 ft
 *   * Formula: altitude = value × 16 ft (rounded to nearest 100 ft)
 *   * Range: [0, 65,520] ft
 *   * Source: Mode Control Panel / Flight Control Unit
 *
 * **FMS Selected Altitude** (bits 14-26):
 * - Bit 14: Status (0=invalid, 1=valid)
 * - Bits 15-26: 12-bit altitude value
 *   * Same encoding as MCP altitude (LSB=16 ft)
 *   * Source: Flight Management System vertical profile
 *
 * **Barometric Pressure Setting** (bits 27-39):
 * - Bit 27: Status (0=invalid, 1=valid)
 * - Bits 28-39: 12-bit pressure value
 *   * MSB = 204.8 mb
 *   * LSB = 0.1 mb
 *   * Formula: pressure = (value × 0.1) + 800 mb
 *   * Range: [800.0, 1209.5] mb
 *   * Status bit set to 0 if pressure <800 mb or >1209.5 mb
 *
 * **Reserved** (bits 40-47): Must be all zeros
 *
 * **MCP/FCU Mode Status** (bits 48-51):
 * - Bit 48: Mode status (0=no mode info, 1=mode info provided)
 * - Bit 49: VNAV mode (0=not active, 1=active)
 * - Bit 50: Altitude hold mode (0=not active, 1=active)
 * - Bit 51: Approach mode (0=not active, 1=active)
 *
 * **Reserved** (bits 52-53): Must be all zeros
 *
 * **Target Altitude Source** (bits 54-56):
 * - Bit 54: Source status (0=no source info, 1=source info provided)
 * - Bits 55-56: Target source (2 bits)
 *   * 00 = Unknown
 *   * 01 = Aircraft altitude (leveled off)
 *   * 10 = FCU/MCP selected altitude
 *   * 11 = FMS selected altitude
 *
 * Target Altitude Definition (per ICAO Doc 9871 Note 1):  
 * "Short-term intent value at which aircraft will level off (or has leveled off)
 * at end of current maneuver. Represents real aircraft intent when available,
 * from altitude control panel, FMS, or current altitude depending on flight mode."
 */

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
#[serde(tag = "bds", rename = "40")]
pub struct SelectedVerticalIntention {
    #[deku(reader = "read_selected(deku::reader)")]
    #[serde(rename = "selected_mcp", skip_serializing_if = "Option::is_none")]
    /// MCP/FCU Selected Altitude (bits 1-13): Per ICAO Doc 9871 Table A-2-64  
    /// Altitude from Mode Control Panel / Flight Control Unit.  
    /// Formula: altitude = (value × 16 ft), rounded to nearest 100 ft  
    /// Range: [0, 65,520] ft  
    /// Returns None if status bit (bit 1) is 0.
    pub selected_altitude_mcp: Option<u16>,

    #[deku(reader = "read_selected(deku::reader)")]
    #[serde(rename = "selected_fms", skip_serializing_if = "Option::is_none")]
    /// FMS Selected Altitude (bits 14-26): Per ICAO Doc 9871 Table A-2-64  
    /// Altitude from Flight Management System vertical profile.  
    /// Formula: altitude = (value × 16 ft), rounded to nearest 100 ft  
    /// Range: [0, 65,520] ft  
    /// Returns None if status bit (bit 14) is 0.
    pub selected_altitude_fms: Option<u16>,

    #[deku(reader = "read_qnh(deku::reader)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Barometric Pressure Setting (bits 27-39): Per ICAO Doc 9871 Table A-2-64  
    /// Current altimeter barometric pressure setting (QNH).  
    /// Formula: pressure = (value × 0.1) + 800 mb  
    /// LSB = 0.1 mb, MSB = 204.8 mb  
    /// Range: [800.0, 1209.5] mb  
    /// Returns None if status bit (bit 27) is 0 or value out of range.
    pub barometric_setting: Option<f64>,

    #[serde(skip)]
    #[deku(map = "|v: u8| {
        if v == 0 { Ok(v) } else {
            Err(DekuError::Assertion(\"Reserved bits must be 0\".into()))
        }
    }")]
    #[deku(bits = 8)]
    /// Reserved (bits 40-47): Per ICAO Doc 9871 Note 5  
    /// Must be all zeros. Assertion fails if non-zero.
    pub reserved: u8,

    #[deku(bits = 1)]
    #[serde(skip)]
    /// MCP/FCU Mode Status (bit 48): Per ICAO Doc 9871 Table A-2-64 Note 6  
    /// Indicates if mode bits (49-51) are populated:
    ///   - false (0): No mode information provided
    ///   - true (1): Mode information deliberately provided
    pub mcp_status: bool,

    #[deku(bits = 1)]
    #[serde(skip)]
    /// VNAV Mode (bit 49): 0=not active, 1=active
    pub vnav_mode: bool,

    #[deku(bits = 1)]
    #[serde(skip)]
    /// Altitude Hold Mode (bit 50): 0=not active, 1=active
    pub alt_hold_mode: bool,

    #[deku(bits = 1)]
    #[serde(skip)]
    /// Approach Mode (bit 51): 0=not active, 1=active
    pub approach_mode: bool,

    #[deku(map = "|v: u8| {
        if v == 0 { Ok(v) } else {
            Err(DekuError::Assertion(\"Reserved bits must be 0\".into()))
        }
    }")]
    #[deku(bits = 2)]
    #[serde(skip)]
    /// Reserved (bits 52-53): Must be all zeros
    pub reserved1: u8,

    #[deku(bits = 1)]
    #[serde(skip)]
    /// Target Altitude Source Status (bit 54): Per ICAO Doc 9871 Table A-2-64  
    /// Indicates if source bits (55-56) are populated:
    ///   - false (0): No source information provided
    ///   - true (1): Source information deliberately provided
    pub source_status: bool,

    #[serde(
        rename = "target_source",
        skip_serializing_if = "TargetSource::is_unknown"
    )]
    /// Target Altitude Source (bits 55-56): Per ICAO Doc 9871 Table A-2-64  
    /// Indicates which source defines the target altitude (short-term intent):
    ///   - Unknown (00): Source unknown
    ///   - AircraftAltitude (01): Currently at target (leveled off)
    ///   - FcpMcuSelectedAltitude (10): Using MCP/FCU selected altitude
    ///   - FmsSelectedAltitude (11): Using FMS selected altitude
    pub target_altitude_source: TargetSource,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
#[deku(id_type = "u8", bits = "2")]
pub enum TargetSource {
    #[deku(id = "0")]
    Unknown,
    #[deku(id = "1")]
    AircraftAltitude,
    #[deku(id = "2")]
    FcpMcuSelectedAltitude,
    #[deku(id = "3")]
    FmsSelectedAltitude,
}
impl TargetSource {
    fn is_unknown(&self) -> bool {
        self == &Self::Unknown
    }
}

/// Decode selected altitude from 13-bit field (1 status + 12 value)
///
/// Per ICAO Doc 9871 Table A-2-64: MCP/FCU or FMS selected altitude
///
/// Structure:
/// - Bit 1 (or 14): Status bit (0=invalid, 1=valid)
/// - Bits 2-13 (or 15-26): 12-bit altitude value
///
/// Encoding:
/// - MSB = 32,768 ft
/// - LSB = 16 ft
/// - Formula: altitude = value × 16 ft
/// - Range: [0, 65,520] ft
/// - Values rounded to nearest 100 ft in implementation
///
/// Validation:
/// - If status=0, value must be 0 (returns None)
/// - Decoded values >45,000 ft rejected as invalid
/// - Rounding: (value × 16 + 8) / 100 × 100 for nearest 100 ft
///
/// Returns:
/// - Some(altitude): Altitude in feet (rounded to 100 ft)
/// - None: Status bit 0 or invalid data
fn read_selected<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<u16>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(12)),
    )?;

    if !status {
        if value != 0 {
            return Err(DekuError::Assertion(
                "Invalid selected_fms or mcp value".into(),
            ));
        } else {
            return Ok(None);
        }
    }
    let value = value * 16;
    // (encoded as a multiple of 16, but rounded to the closest 100 ft)
    let value = (value + 8) / 100 * 100;
    if value > 45000 {
        let msg = format!(
            "Value for selected_fms or selected_mcp: {value} ft > 45000 ft"
        );
        return Err(DekuError::Assertion(msg.into()));
    }

    Ok(Some(value))
}

/// Decode barometric pressure setting from 13-bit field (1 status + 12 value)
///
/// Per ICAO Doc 9871 Table A-2-64 Note 4: Barometric pressure setting minus 800 mb
///
/// Structure:
/// - Bit 27: Status bit (0=invalid, 1=valid)
/// - Bits 28-39: 12-bit pressure value
///
/// Encoding:
/// - MSB = 204.8 mb
/// - LSB = 0.1 mb
/// - Formula: pressure = (value × 0.1) + 800 mb
/// - Range: [800.0, 1209.5] mb
///
/// Validation:
/// - If status=0, value must be 0 (returns None)
/// - Pressure <800 mb or >1209.5 mb sets status bit to 0
///
/// Note: Standard atmospheric pressure is 1013.25 mb (29.92 inHg).
/// The 800 mb offset allows encoding typical operational pressures
/// (e.g., 980-1050 mb) with good resolution.
///
/// Returns:
/// - Some(pressure): Barometric pressure in millibars
/// - None: Status bit 0 or invalid data
fn read_qnh<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<f64>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(12)),
    )?;

    if !status {
        if value != 0 {
            return Err(DekuError::Assertion("Invalid QNH value".into()));
        } else {
            return Ok(None);
        }
    }

    Ok(Some(value as f64 * 0.1 + 800.))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use approx::assert_relative_eq;
    use hexlit::hex;

    #[test]
    fn test_valid_bds40() {
        let bytes = hex!("a000029c85e42f313000007047d3");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            let SelectedVerticalIntention {
                selected_altitude_fms,
                selected_altitude_mcp,
                barometric_setting,
                ..
            } = bds.bds40.unwrap();
            assert_eq!(selected_altitude_fms.unwrap(), 3000);
            assert_eq!(selected_altitude_mcp.unwrap(), 3000);
            assert_relative_eq!(
                barometric_setting.unwrap(),
                1020.,
                max_relative = 1e-3
            );
        } else {
            unreachable!();
        }
    }
    #[test]
    fn test_invalid_bds40() {
        let bytes = hex!("a0000638fa81c10000000081a92f");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            assert_eq!(bds.bds40, None);
        } else {
            unreachable!();
        }
    }
}
