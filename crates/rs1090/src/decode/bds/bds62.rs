#![allow(clippy::suspicious_else_formatting)]

use deku::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;

/**
 * ## Target State and Status Information (BDS 6,2 / TYPE=29)
 *
 * Extended Squitter ADS-B message providing aircraft target state and status.  
 * Per DO-260B §2.2.3.2.7.1: Target State and Status Messages (TYPE=29, Subtype=1)
 *
 * Purpose: Provides the selected altitude, heading, barometric setting, and
 * autopilot/flight mode status for trajectory prediction and conflict detection.
 *
 * Message Structure (56 bits):
 * | TYPE | SUB | SIL | SRC | ALT  | QNH | HDGS | HDG | NACP | NICB | SIL | STAT | AP | VN | AH | IMF | APR | TCAS | LN | RES |
 * |------|-----|-----|-----|------|-----|------|-----|------|------|-----|------|----|----|----|----|-----|------|----|----|
 * | 5    | 2   | 1   | 1   | 11   | 9   | 1    | 9   | 4    | 1    | 2   | 1    | 1  | 1  | 1  | 1  | 1   | 1    | 1  | 2  |
 *
 * Field Encoding per DO-260B §2.2.3.2.7.1.3:
 *
 * **Subtype** (bits 6-7): Must be 01 (1) for DO-260B compliance
 *
 * **SIL Supplement** (bit 8): Source Integrity Level probability basis
 *   - 0 = per hour basis (GNSS sources)
 *   - 1 = per sample basis (IRU, DME/DME sources)
 *
 * **Selected Altitude Source** (bit 9):
 *   - 0 = MCP/FCU (Mode Control Panel / Flight Control Unit)
 *   - 1 = FMS (Flight Management System)
 *   - Set to 0 if no valid altitude data available
 *
 * **Selected Altitude** (bits 10-20):
 *   - 11-bit field containing MCP/FCU or FMS selected altitude
 *   - LSB = 32 ft
 *   - Formula: altitude = value × 32 ft (if value > 1)
 *   - Range: [0, 65,472] ft
 *   - value=0: No data or invalid data
 *   - value=1: 0 ft
 *   - value=2047: 65,472 ft
 *   - Implementation rounds to nearest 100 ft: ((value - 1) × 32 + 16) / 100 × 100
 *   - Note: May not reflect true intention during VNAV/approach modes
 *
 * **Barometric Pressure Setting** (bits 21-29):
 *   - 9-bit field encoding QNH/QFE minus 800 millibars
 *   - LSB = 0.8 mbar
 *   - Formula: pressure = 800 + value × 0.8 mbar (if value > 0)
 *   - Range: [800, 1208.4] mbar
 *   - value=0: No data or invalid data
 *   - value=1: 800.0 mbar
 *   - value=511: 1208.0 mbar
 *   - Values outside [800, 1208.4] mbar encoded as 0
 *   - Note: Can represent QFE or QNH depending on local procedures
 *
 * **Selected Heading Status** (bit 30):
 *   - 0 = heading data not available or invalid
 *   - 1 = heading data available and valid
 *
 * **Selected Heading Sign** (bit 31):
 *   - 0 = Positive (0° to 179.9°)
 *   - 1 = Negative (180° to 359.9°, encoded as -180° to -0.7°)
 *
 * **Selected Heading** (bits 32-39):
 *   - 8-bit field encoding selected heading (magnetic)
 *   - LSB = 180/256 degrees (≈0.703125°)
 *   - Formula: heading = value × (180/256) degrees
 *   - Range: [0, 359.9] degrees
 *   - Angular system: [-180, +180] degrees internally, converted to [0, 360]°
 *   - Returns None if status bit = 0
 *
 * **NAC_P** (bits 40-43): Navigation Accuracy Category - Position (4 bits)
 *   - Indicates horizontal position accuracy
 *   - Values 0-15, higher values = better accuracy
 *
 * **NIC_BARO** (bit 44): Navigation Integrity Category - Barometric
 *   - 0 = barometric altitude not cross-checked
 *   - 1 = barometric altitude cross-checked with another pressure source
 *
 * **SIL** (bits 45-46): Source Integrity Level (2 bits)
 *   - Per sample or per hour probability (see SIL Supplement)
 *   - Values 0-3, higher values = better integrity
 *
 * **Mode Status** (bit 47): Status bit for following mode flags
 *   - 0 = mode flags invalid
 *   - 1 = mode flags valid
 *
 * - **Autopilot Engaged** (bit 48): 1 = autopilot engaged (if mode_status=1)
 * - **VNAV Mode Engaged** (bit 49): 1 = VNAV active (if mode_status=1)
 * - **Altitude Hold Mode** (bit 50): 1 = altitude hold active (if mode_status=1)
 * - **IMF** (bit 51): Reserved for ADS-R flag
 * - **Approach Mode** (bit 52): 1 = approach mode active (if mode_status=1)
 * - **TCAS Operational** (bit 53): 1 = TCAS/ACAS operational (ALWAYS valid)
 * - **LNAV Mode Engaged** (bit 54): 1 = LNAV active (if mode_status=1)
 * - **Reserved** (bits 55-56): Reserved bits
 *
 * Important Notes per DO-260B:
 * - Selected altitude may not reflect true intention during certain flight modes
 * - Many aircraft only transmit MCP/FCU selected altitude, not FMS altitude
 * - Target altitude (next level-off altitude) may differ from selected altitude
 * - Barometric setting represents value currently being used to fly the aircraft
 * - Mode flags are only valid if mode_status bit is set
 * - TCAS operational flag is always valid regardless of mode_status
 *
 * Reference: DO-260B §2.2.3.2.7.1, Figure 2-10
 */
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, DekuRead)]
pub struct TargetStateAndStatusInformation {
    /// Subtype (bits 6-7): Per DO-260B §2.2.3.2.7.1.2  
    /// Identifies format of Target State and Status Message.  
    /// Encoding:
    ///   - 0 = Reserved (DO-260A format)
    ///   - 1 = DO-260B format (required for MOPS compliance)
    ///   - 2-3 = Reserved
    ///
    /// DO-260B compliant systems must transmit subtype=1.
    #[deku(bits = "2")]
    #[serde(skip)]
    pub subtype: u8,

    /// Selected Altitude Source (bit 9): Per DO-260B §2.2.3.2.7.1.3.2  
    /// Indicates source of selected altitude data.  
    /// Encoding:
    ///   - 0 = MCP/FCU (Mode Control Panel / Flight Control Unit)
    ///   - 1 = FMS (Flight Management System)
    ///
    /// Set to 0 if no valid altitude data available.  
    /// Note: Many aircraft only provide MCP/FCU altitude, not FMS altitude.
    #[deku(pad_bits_before = "1")]
    #[serde(rename = "source")]
    pub alt_source: AltSource,

    /// Selected Altitude (bits 10-20): Per DO-260B §2.2.3.2.7.1.3.3
    /// MCP/FCU or FMS selected altitude in feet.
    /// Encoding details:
    ///   - LSB = 32 ft
    ///   - Formula: altitude = value × 32 ft (if value > 1)
    ///   - Range: [0, 65,472] ft
    ///   - value=0: No data or invalid data
    ///   - value=1: 0 ft
    ///   - value=2047: 65,472 ft
    ///
    /// Implementation rounds to nearest 100 ft: ((value - 1) × 32 + 16) / 100 × 100  
    /// Returns None if value ≤ 1.  
    /// Note: May not reflect true intention during VNAV or approach modes.  
    /// This is the selected altitude, not necessarily the target altitude.
    #[deku(
        bits = "11",
        endian = "big",
        map = "|altitude: u16| -> Result<_, DekuError> {
            Ok(
                if altitude > 1 {Some(((altitude - 1) * 32 + 16) / 100 * 100)}
                else {None}
            )
        }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_altitude: Option<u16>,

    /// Barometric Pressure Setting (bits 21-29): Per DO-260B §2.2.3.2.7.1.3.4  
    /// QNH or QFE setting in millibars.  
    /// Encoding details:
    ///   - LSB = 0.8 mbar
    ///   - Formula: pressure = 800 + value × 0.8 mbar (if value > 0)
    ///   - Range: [800, 1208.4] mbar
    ///   - value=0: No data or invalid data
    ///   - value=1: 800.0 mbar
    ///   - value=511: 1208.0 mbar
    ///
    /// Values outside [800, 1208.4] mbar are encoded as 0.  
    /// Returns None if value = 0.  
    /// Note: Can represent QFE or QNH depending on local procedures.  
    /// This is the value currently being used to fly the aircraft.
    #[deku(
        bits = "9",
        endian = "big",
        map = "|qnh: u32| -> Result<_, DekuError> {
            if qnh == 0 { Ok(None) }
            else { Ok(Some(800.0 + ((qnh - 1) as f32) * 0.8)) }
        }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub barometric_setting: Option<f32>,

    /// Selected Heading Status (bit 30): Per DO-260B §2.2.3.2.7.1.3.5  
    /// Status bit for selected heading validity.  
    /// Encoding:
    ///   - 0 = heading data not available or invalid
    ///   - 1 = heading data available and valid
    #[deku(bits = "1")]
    #[serde(skip)]
    pub heading_status: bool,

    /// Selected Heading (bits 31-39): Per DO-260B §2.2.3.2.7.1.3.6/3.7  
    /// Selected heading in degrees (magnetic north reference).  
    /// Encoding details:
    ///   - Bit 31: Sign (0=positive [0-179.9°], 1=negative [180-359.9°])
    ///   - Bits 32-39: 8-bit heading magnitude
    ///   - LSB = 180/256 degrees (≈0.703125°)
    ///   - Formula: heading = value × (180/256) degrees
    ///   - Range: [0, 359.9] degrees
    ///   - Angular system: [-180, +180]° internally, converted to [0, 360]°
    ///
    /// Returns None if heading_status = 0.
    #[deku(
        bits = "9",
        endian = "big",
        map = "|heading: u16| -> Result<_, DekuError> {
            if *heading_status {Ok(Some(heading as f32 * 180.0 / 256.0))} 
            else {Ok(None)}
        }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_heading: Option<f32>,

    /// NAC_P (bits 40-43): Per DO-260B §2.2.3.2.7.1.3.8  
    /// Navigation Accuracy Category - Position.  
    /// 4-bit value indicating horizontal position accuracy.  
    /// Values 0-15, higher values indicate better accuracy.
    #[deku(bits = "4")]
    #[serde(rename = "NACp")]
    pub nac_p: u8,

    /// NIC_BARO (bit 44): Per DO-260B §2.2.3.2.7.1.3.9  
    /// Navigation Integrity Category - Barometric.  
    /// Encoding:
    ///   - 0 = barometric altitude not cross-checked with another source
    ///   - 1 = barometric altitude cross-checked with another pressure source
    #[deku(bits = "1")]
    #[serde(skip)]
    pub nic_baro: bool,

    /// SIL (bits 45-46): Per DO-260B §2.2.3.2.7.1.3.10  
    /// Source Integrity Level (2 bits).  
    /// Probability basis determined by SIL Supplement (bit 8).  
    /// Values 0-3, higher values indicate better integrity.
    #[deku(bits = "2")]
    #[serde(skip)]
    pub sil: u8,

    /// Mode Status (bit 47): Per DO-260B §2.2.3.2.7.1.3.11  
    /// Status bit for MCP/FCU mode flags.  
    /// Encoding:
    ///   - 0 = mode flags (autopilot, VNAV, LNAV, altitude hold, approach) invalid
    ///   - 1 = mode flags valid
    ///
    /// Note: TCAS operational flag is always valid regardless of this bit.
    #[deku(bits = "1")]
    #[serde(skip)]
    pub mode_status: bool,

    /// Autopilot Engaged (bit 48): Per DO-260B §2.2.3.2.7.1.3.12  
    /// Autopilot engagement status.  
    /// Encoding:
    ///   - 0 = autopilot not engaged
    ///   - 1 = autopilot engaged
    ///
    /// Returns None if mode_status = 0.
    #[deku(
        bits = "1",
        map = "|val: bool| -> Result<_, DekuError> {
            if *mode_status {Ok(Some(val))} else {Ok(None)}
        }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopilot: Option<bool>,

    /// VNAV Mode Engaged (bit 49): Per DO-260B §2.2.3.2.7.1.3.13  
    /// Vertical Navigation mode status.  
    /// Encoding:
    ///   - 0 = VNAV mode not engaged
    ///   - 1 = VNAV mode engaged
    ///
    /// Returns None if mode_status = 0.
    #[deku(
        bits = "1",
        map = "|val: bool| -> Result<_, DekuError> {
            if *mode_status {Ok(Some(val))} else {Ok(None)}
        }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vnav_mode: Option<bool>,

    /// Altitude Hold Mode (bit 50): Per DO-260B §2.2.3.2.7.1.3.14  
    /// Altitude hold mode status.  
    /// Encoding:
    ///   - 0 = altitude hold mode not active
    ///   - 1 = altitude hold mode active
    ///
    /// Returns None if mode_status = 0.
    #[deku(
        bits = "1",
        map = "|val: bool| -> Result<_, DekuError> {
            if *mode_status {Ok(Some(val))} else {Ok(None)}
        }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_hold: Option<bool>,

    /// IMF (bit 51): Per DO-260B §2.2.3.2.7.1.3.15  
    /// Reserved for ADS-R (Automatic Dependent Surveillance-Rebroadcast) flag.
    #[deku(bits = "1")]
    #[serde(skip)]
    pub imf: bool,

    /// Approach Mode (bit 52): Per DO-260B §2.2.3.2.7.1.3.16  
    /// Approach mode status.  
    /// Encoding:
    ///   - 0 = approach mode not active
    ///   - 1 = approach mode active
    ///
    /// Returns None if mode_status = 0.
    #[deku(
        bits = "1",
        map = "|val: bool| -> Result<_, DekuError> {
            if *mode_status {Ok(Some(val))} else {Ok(None)}
        }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_mode: Option<bool>,

    /// TCAS Operational (bit 53): Per DO-260B §2.2.3.2.7.1.3.17  
    /// TCAS/ACAS operational status.  
    /// Encoding:
    ///   - 0 = TCAS/ACAS not operational
    ///   - 1 = TCAS/ACAS operational
    ///
    /// Note: This flag is ALWAYS valid, regardless of mode_status bit.
    #[deku(bits = "1")]
    pub tcas_operational: bool,

    /// LNAV Mode Engaged (bit 54): Per DO-260B §2.2.3.2.7.1.3.18  
    /// Lateral Navigation mode status.  
    /// Encoding:
    ///   - 0 = LNAV mode not engaged
    ///   - 1 = LNAV mode engaged
    ///
    /// Returns None if mode_status = 0.
    #[deku(
        bits = "1",
        map = "|val: bool| -> Result<_, DekuError> {
            if *mode_status {Ok(Some(val))} else {Ok(None)}
        }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[deku(pad_bits_after = "2")]
    pub lnav_mode: Option<bool>,
}

impl fmt::Display for TargetStateAndStatusInformation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  Target state and status (BDS 6,2)")?;
        if let Some(sel_alt) = &self.selected_altitude {
            writeln!(
                f,
                "  Selected alt:  {} ft {}",
                sel_alt, &self.alt_source
            )?;
        }
        if let Some(sel_hdg) = &self.selected_heading {
            writeln!(f, "  Selected hdg:  {sel_hdg:.1}°")?;
        }
        if let Some(qnh) = &self.barometric_setting {
            writeln!(f, "  QNH:           {qnh:.1} mbar")?;
        }
        if self.mode_status {
            write!(f, "  Mode:         ")?;
            if let Some(value) = self.autopilot {
                if value {
                    write!(f, " autopilot")?;
                }
            }
            if let Some(value) = self.vnav_mode {
                if value {
                    write!(f, " VNAV")?;
                }
            }
            if let Some(value) = self.lnav_mode {
                if value {
                    write!(f, " LNAV")?;
                }
            }
            if let Some(value) = self.alt_hold {
                if value {
                    write!(f, " alt_hold")?;
                }
            }
            if let Some(value) = self.approach_mode {
                if value {
                    write!(f, " approach")?;
                }
            }
            writeln!(f)?;
        }
        writeln!(
            f,
            "  TCAS:          {}",
            if self.tcas_operational { "on" } else { "off" }
        )
    }
}

/// Selected Altitude Source: Per DO-260B §2.2.3.2.7.1.3.2
/// Indicates the source of the selected altitude data.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, DekuRead)]
#[deku(id_type = "u8", bits = "1")]
pub enum AltSource {
    /// Mode Control Panel / Flight Control Unit
    /// Manual altitude selection by pilot via MCP/FCU panel.
    #[deku(id = "0")]
    #[serde(rename = "MCP/FCU")]
    MCP,

    /// Flight Management System
    /// Altitude from FMS flight plan.
    /// Note: Many aircraft only provide MCP/FCU altitude, not FMS.
    #[deku(id = "1")]
    FMS,
}
impl fmt::Display for AltSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::MCP => write!(f, "MCP/FCU"),
            Self::FMS => write!(f, "FMS"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use approx::assert_relative_eq;
    use hexlit::hex;

    #[test]
    fn test_surface_position() {
        let bytes = hex!("8DA05629EA21485CBF3F8CADAEEB");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS62(TargetStateAndStatusInformation {
                selected_altitude,
                alt_source,
                barometric_setting,
                selected_heading,
                mode_status,
                autopilot,
                vnav_mode,
                lnav_mode,
                alt_hold,
                approach_mode,
                tcas_operational,
                ..
            }) = adsb_msg.message
            {
                assert_eq!(selected_altitude, Some(17000));
                assert_eq!(alt_source, AltSource::MCP);
                assert_eq!(barometric_setting, Some(1012.8));
                assert_relative_eq!(
                    selected_heading.unwrap(),
                    66.8,
                    max_relative = 1e-2
                );
                assert!(mode_status);
                assert_eq!(autopilot, Some(true));
                assert_eq!(vnav_mode, Some(true));
                assert_eq!(lnav_mode, Some(true));
                assert_eq!(alt_hold, Some(false));
                assert_eq!(approach_mode, Some(false));
                assert!(tcas_operational);
            }
            return;
        }
        unreachable!();
    }

    #[test]
    fn test_format_groundspeed() {
        let bytes = hex!("8DA05629EA21485CBF3F8CADAEEB");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        assert_eq!(
            format!("{msg}"),
            r#" DF17. Extended Squitter
  Address:       a05629
  Air/Ground:    airborne
  Target state and status (BDS 6,2)
  Selected alt:  17000 ft MCP/FCU
  Selected hdg:  66.8°
  QNH:           1012.8 mbar
  Mode:          autopilot VNAV LNAV
  TCAS:          on
"#
        )
    }
}
