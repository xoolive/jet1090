use deku::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;

/**
 * ## Aircraft Operational Status (BDS 6,5 / TYPE=31)
 *
 * Extended Squitter ADS-B message providing operational capabilities and modes.  
 * Per ICAO Doc 9871 Table B-2-101: BDS code 6,5 — Extended squitter aircraft operational status
 *
 * Purpose: To provide the capability class and current operational mode of
 * ATC-related applications and other operational information.
 *
 * Message Structure (56 bits):
 * | TYPE | SUBTYPE | CC    | LW  | OM   | VERSION | NIC | NAC_P | BAQ | SIL | NIC_BARO | TRK_HDG | HRD | RES |
 * |------|---------|-------|-----|------|---------|-----|-------|-----|-----|----------|---------|-----|-----|
 * | 5    | 3       | 16    | 4   | 16   | 3       | 1   | 4     | 1   | 2   | 1        | 1       | 1   | 2   |
 *
 * Field Encoding per ICAO Doc 9871 §B.2.3.10:
 *
 * **TYPE Code** (bits 1-5): Fixed value 31 (11111 binary)
 *
 * **Subtype** (bits 6-8): 3-bit subtype field
 *   - 0 = Airborne Status Message
 *   - 1 = Surface Status Message
 *   - 2-7 = Reserved
 *
 * **Capability Class (CC) Codes** (bits 9-24): 16-bit capability field
 *   - Airborne (Subtype 0): ACAS, CDTI, ARV, TS, TC capabilities
 *     * Bits 9-10: Reserved (0)
 *     * Bit 11: ACAS (TCAS Resolution Advisory Active)
 *     * Bit 12: CDTI (Cockpit Display of Traffic Information)
 *     * Bits 13-14: Reserved (0)
 *     * Bit 15: ARV (Air-Referenced Velocity Report Capability)
 *     * Bit 16: TS (Target State Report Capability)
 *     * Bits 17-18: TC (Target Trajectory Change Report Capability)
 *       - 0 = No capability
 *       - 1 = TC+0 reports only
 *       - 2 = Multiple TC reports
 *       - 3 = Reserved
 *     * Bits 19-24: Reserved
 *   - Surface (Subtype 1): Similar capabilities for surface operations
 *
 * **Length/Width (L/W) Codes** (bits 25-28): 4-bit aircraft size (Surface only)
 *   - Encodes aircraft dimensions per §B.2.3.10.11
 *
 * **Operational Mode (OM) Codes** (bits 29-44): 16-bit operational mode field
 *   - Various operational flags and modes per §B.2.3.10.4
 *
 * **Version Number** (bits 45-47): 3-bit ADS-B version
 *   - 0 = Not implemented (DO-260)
 *   - 1 = DO-260A
 *   - 2 = DO-260B
 *   - 3-7 = Reserved for future versions
 *   - Note: Message structure differs significantly between versions
 *
 * **NIC Supplement** (bit 48): Navigation Integrity Category supplement
 *   - Used with NIC from position messages
 *
 * **NAC_P** (bits 49-52): Navigation Accuracy Category - Position
 *   - 4-bit horizontal position accuracy indicator
 *
 * **BAQ** (bit 53): Barometric Altitude Quality (when 0, bits 54-56 reserved)
 *
 * **SIL** (bits 54-55): Source Integrity Level (2 bits)
 *   - Probability of position integrity
 *
 * **NIC_BARO** (bit 56): Barometric altitude cross-check flag
 *
 * **TRK/HDG** (bit 57): Track angle vs. heading (Airborne)
 *   - 0 = True track angle
 *   - 1 = Magnetic heading
 *
 * **HRD** (bit 58): Heading Reference Direction (Airborne)
 *   - 0 = True north
 *   - 1 = Magnetic north
 *
 * **Reserved** (bits 59-56): Reserved for future use
 *
 * Version Differences:
 * - Version 0 (DO-260): Not implemented in practice, basic structure
 * - Version 1 (DO-260A): Added accuracy and integrity indicators
 * - Version 2 (DO-260B): Enhanced with additional operational modes
 *
 * Transmission:
 * - Message delivery accomplished using event-driven protocol
 * - Broadcast when capability or operational status changes
 *
 * Reference: ICAO Doc 9871 Table B-2-101, §B.2.3.10  
 * Additional details: DO-260B §2.2.3.2.7.2
 */

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
#[deku(id_type = "u8", bits = "3")]
#[serde(untagged)]
pub enum AircraftOperationStatus {
    #[deku(id = "0")]
    Airborne(OperationStatusAirborne),

    #[deku(id = "1")]
    Surface(OperationStatusSurface),

    #[deku(id_pat = "2..=7")]
    Reserved { id: u8, status: ReservedStatus },
}

impl fmt::Display for AircraftOperationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  Aircraft Operation Status (BDS 6,5)")?;
        match &self {
            Self::Airborne(airborne) => write!(f, "{airborne}"),
            Self::Surface(surface) => write!(f, "{surface}"),
            Self::Reserved { id, status } => {
                write!(f, " Reserved: id={}, data={:?}", id, status.data)
            }
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct OperationStatusAirborne {
    /// The capacity class
    #[serde(skip, default = "serde_default_capability_class_airborne")]
    pub capability_class: CapabilityClassAirborne,

    /// The operational mode
    #[serde(skip, default = "serde_default_operational_mode")]
    pub operational_mode: OperationalMode,

    #[deku(pad_bytes_before = "1")]
    #[serde(flatten)]
    pub version: ADSBVersionAirborne,
}

impl fmt::Display for OperationStatusAirborne {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "   Capability classes: {}", self.capability_class)?;
        writeln!(f, "   Operational modes:  {}", self.operational_mode)?;
        write!(f, "   {}", self.version)?;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct CapabilityClassAirborne {
    #[deku(bits = "2", assert_eq = "0")]
    #[serde(skip, default = "u8::default")]
    pub reserved0: u8,

    /// TCAS Resolution Advisory Active
    #[deku(bits = "1")]
    #[serde(rename = "ACAS")]
    pub acas: bool,

    /// Cockpit Display of Traffic Information
    #[deku(bits = "1")]
    #[serde(rename = "CDTI")]
    pub cdti: bool,

    #[deku(bits = "2", assert_eq = "0")]
    #[serde(skip, default = "u8::default")]
    pub reserved1: u8,

    /// Air-Referenced Velocity Report Capability
    #[deku(bits = "1")]
    #[serde(rename = "ARV")]
    pub arv: bool,

    #[deku(bits = "1")]
    #[serde(rename = "TS")]
    /// Target State Report Capability
    pub ts: bool,

    #[deku(bits = "2")]
    #[deku(pad_bits_after = "6")] //reserved
    #[serde(rename = "TC")]
    /// Target Trajectory Change Report Capability
    /// - 0: No capability for Trajectory Change Reports
    /// - 1: Support for TC+0 reports only
    /// - 2: Support for multiple TC reports
    /// - 3: Reserved
    pub tc: u8,
}

pub fn serde_default_capability_class_airborne() -> CapabilityClassAirborne {
    CapabilityClassAirborne {
        reserved0: 0,
        acas: false,
        cdti: false,
        reserved1: 0,
        arv: false,
        ts: false,
        tc: 0,
    }
}

impl fmt::Display for CapabilityClassAirborne {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.acas {
            write!(f, " ACAS")?;
        }
        if self.cdti {
            write!(f, " CDTI")?;
        }
        if self.arv {
            write!(f, " ARV")?;
        }
        if self.ts {
            write!(f, " TS")?;
        }
        if self.tc == 1 {
            write!(f, " TC")?;
        }
        Ok(())
    }
}

/// Version 2 support only
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct OperationStatusSurface {
    /// The capacity class
    #[serde(skip, default = "serde_default_capability_class_surface")]
    pub capability_class: CapabilityClassSurface,

    /// The capacity class L/W codes
    #[deku(bits = "4")]
    #[serde(skip, default = "u8::default")]
    pub lw_codes: u8,

    /// The operational mode
    #[serde(skip, default = "serde_default_operational_mode")]
    pub operational_mode: OperationalMode,

    /// The GPS antenna offset (2.2.3.2.7.2.4.7).
    /// Reference: <http://www.anteni.net/adsb/Doc/1090-WP30-18-DRAFT_DO-260B-V42.pdf>
    #[serde(skip, default = "u8::default")]
    pub gps_antenna_offset: u8,

    #[serde(flatten, default = "serde_default_adsb_version_surface")]
    pub version: ADSBVersionSurface,
}

impl fmt::Display for OperationStatusSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.capability_class)?;
        writeln!(f, "   Operational modes: {}", self.operational_mode)?;
        write!(f, "   {}", self.version)?;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct CapabilityClassSurface {
    #[deku(bits = "2", assert_eq = "0")]
    #[serde(skip, default = "u8::default")]
    pub reserved0: u8,

    /// Position Offset Applied
    #[deku(bits = "1")]
    pub poe: bool,

    /// Aircraft has ADS-B 1090ES Receive Capability
    #[deku(bits = "1")]
    #[deku(pad_bits_after = "2")] // reserved
    #[serde(rename = "1090ES")]
    pub es1090: bool,

    /// Class B2 Ground Vehicle transmitting with less than 70 watts
    #[deku(bits = "1")]
    #[serde(rename = "GRND")]
    pub b2_low: bool,

    /// Aircraft has ADS-B UAT Receive Capability
    #[deku(bits = "1")]
    #[serde(rename = "UATin")]
    pub uat_in: bool,

    /// Navigation Accuracy Category for Velocity (NACv), only for version 1 and 2
    #[deku(bits = "3")] // ME 11-13
    #[serde(rename = "NACv")]
    pub nac_v: u8,

    /// NIC Supplement (NICc) used on the Surface
    #[deku(bits = "1")]
    #[serde(rename = "NICc")]
    pub nic_c: u8,
}

pub fn serde_default_capability_class_surface() -> CapabilityClassSurface {
    CapabilityClassSurface {
        reserved0: 0,
        poe: false,
        es1090: false,
        b2_low: false,
        uat_in: false,
        nac_v: 0,
        nic_c: 0,
    }
}

impl fmt::Display for CapabilityClassSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "   NICc:               {}", self.nic_c)?;
        writeln!(f, "   NACv:               {}", self.nac_v)?;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct OperationalMode {
    #[deku(bits = "2", assert_eq = "0")]
    #[serde(skip, default = "u8::default")]
    reserved: u8,

    /// TCAS RA active
    #[deku(bits = "1")]
    tcas_ra_active: bool,

    #[deku(bits = "1")]
    ident_switch_active: bool,

    #[deku(bits = "1")]
    reserved_recv_atc_service: bool,

    #[deku(bits = "1")]
    single_antenna_flag: bool,

    #[deku(bits = "2")]
    system_design_assurance: u8,
}

pub fn serde_default_operational_mode() -> OperationalMode {
    OperationalMode {
        reserved: 0,
        tcas_ra_active: false,
        ident_switch_active: false,
        reserved_recv_atc_service: false,
        single_antenna_flag: false,
        system_design_assurance: 0,
    }
}

impl fmt::Display for OperationalMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.tcas_ra_active {
            write!(f, " TCAS")?;
        }
        if self.ident_switch_active {
            write!(f, " IDENT_SWITCH_ACTIVE")?;
        }
        if self.reserved_recv_atc_service {
            write!(f, " ATC")?;
        }
        if self.single_antenna_flag {
            write!(f, " SAF")?;
        }
        if self.system_design_assurance != 0 {
            write!(f, " SDA={}", self.system_design_assurance)?;
        }
        Ok(())
    }
}

/// ADS-B version as defined from different ICAO documents.
/// Reference: ICAO 9871 (5.3.2.3)
///
/// There are three ADS-B versions implemented so far, starting from version 0
/// (specification defined in RTCA document DO-260). Version 1 was introduced
/// around 2008 (DO-260A), and version 2 around 2012 (DO-260B). Version 3 is
/// currently being developed.
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
#[deku(id_type = "u8", bits = "3")]
#[serde(tag = "version")]
pub enum ADSBVersionAirborne {
    #[deku(id = "0")]
    #[serde(rename = "0")] // useless, never happens
    /// ADS-B version 0 (BDS 6,5 undefined, so these messages should not happen)
    DOC9871AppendixA(Empty),
    #[deku(id = "1")]
    #[serde(rename = "1")]
    /// ADS-B version 1 (2008)
    DOC9871AppendixB(AirborneV1),
    #[deku(id = "2")]
    #[serde(rename = "2")]
    /// ADS-B version 2 (2012)
    DOC9871AppendixC(AirborneV2),
    #[deku(id_pat = "3..=7")]
    #[serde(rename = "3to7")]
    Reserved { id: u8 },
}

impl ADSBVersionAirborne {
    /// Get the ADS-B version number as an integer
    ///
    /// Returns:
    /// - 0 for DO-260 (not implemented in practice)
    /// - 1 for DO-260A
    /// - 2 for DO-260B
    /// - 3-7 for reserved/future versions
    pub fn version_number(&self) -> u8 {
        match self {
            Self::DOC9871AppendixA(_) => 0,
            Self::DOC9871AppendixB(_) => 1,
            Self::DOC9871AppendixC(_) => 2,
            Self::Reserved { id } => *id,
        }
    }
}

impl fmt::Display for ADSBVersionAirborne {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DOC9871AppendixA(_) => write!(f, "Version 0 (DO-260)"),
            Self::DOC9871AppendixB(v1) => {
                writeln!(f, "Version 1 (DO-260A)")?;
                writeln!(f, "   NIC supplement:     {}", v1.nic_s)?;
                writeln!(f, "   NACp:               {}", v1.nac_p)?;
                writeln!(
                    f,
                    "   BAQ:                {}",
                    v1.barometric_altitude_quality
                )?;
                writeln!(f, "   SIL:                {}", v1.sil)?;
                writeln!(
                    f,
                    "   BAI:                {}",
                    v1.barometric_altitude_integrity
                )?;
                write!(
                    f,
                    "   HRD:                {}",
                    if v1.horizontal_reference_direction == 0 {
                        "True North"
                    } else {
                        "Magnetic North"
                    }
                )
            }
            Self::DOC9871AppendixC(v2) => {
                writeln!(f, "Version 2 (DO-260B)")?;
                writeln!(f, "   NIC supplement A:   {}", v2.nic_a)?;
                writeln!(f, "   NACp:               {}", v2.nac_p)?;
                writeln!(
                    f,
                    "   GVA:                {}",
                    v2.geometry_vertical_accuracy
                )?;
                writeln!(f, "   SIL:                {}", v2.sil)?;
                writeln!(f, "   SIL supplement:     {}", v2.sil_s)?;
                writeln!(
                    f,
                    "   BAI:                {}",
                    v2.barometric_altitude_integrity
                )?;
                write!(
                    f,
                    "   HRD:                {}",
                    if v2.horizontal_reference_direction == 0 {
                        "True North"
                    } else {
                        "Magnetic North"
                    }
                )
            }
            Self::Reserved { id } => write!(f, "Version {} (Reserved)", id),
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct AirborneV1 {
    #[deku(bits = "1")]
    #[serde(rename = "NICs")]
    /// NIC Supplement bit (NICs)
    pub nic_s: u8,

    #[deku(bits = "4")]
    #[serde(rename = "NACp")]
    /// Navigation Accuracy Category for position (NACp)
    pub nac_p: u8,

    #[deku(bits = "2")]
    #[serde(rename = "BAQ")]
    /// Barometric Altitude Quality (BAQ)
    pub barometric_altitude_quality: u8,

    #[deku(bits = "2")]
    #[serde(rename = "SIL")]
    /// Surveillance Integrity Level (SIL)
    pub sil: u8,

    #[deku(bits = "1")]
    #[serde(rename = "BAI")]
    /// Barometric Altitude Integrity (BAI)
    pub barometric_altitude_integrity: u8,

    #[deku(bits = "1")]
    #[deku(pad_bits_after = "2")] // reserved
    #[serde(rename = "HRD")]
    // 1 for magnetic, 0 for true north
    pub horizontal_reference_direction: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct AirborneV2 {
    #[deku(bits = "1")]
    #[serde(rename = "NICa")]
    /// NIC supplement A (NICa)
    pub nic_a: u8,

    #[deku(bits = "4")]
    #[serde(rename = "NACp")]
    /// Navigation Accuracy Category for position (NACp)
    pub nac_p: u8,

    #[deku(bits = "2")]
    #[serde(rename = "GVA")]
    /// Geometry Vertical Accuracy (GVA)
    pub geometry_vertical_accuracy: u8,

    #[deku(bits = "2")] // ME 51-52
    #[serde(rename = "SIL")]
    /// Surveillance Integrity Level (SIL)
    pub sil: u8,

    #[deku(bits = "1")]
    #[serde(rename = "BAI")]
    /// Barometric Altitude Integrity (BAI)
    pub barometric_altitude_integrity: u8,

    #[deku(bits = "1")]
    #[serde(rename = "HRD")]
    // 1 for magnetic, 0 for true north
    pub horizontal_reference_direction: u8,

    #[deku(bits = "1")]
    #[deku(pad_bits_after = "1")]
    #[serde(rename = "SILs")]
    /// SIL supplement bit, only in version 2:
    /// 0 means per hour,
    /// 1 means per sample.
    pub sil_s: u8,
}

/// ADS-B version as defined from different ICAO documents.  
/// Reference: ICAO 9871 (5.3.2.3)
///
/// There are three ADS-B versions implemented so far, starting from version 0
/// (specification defined in RTCA document DO-260).  
/// Version 1 was introduced around 2008 (DO-260A), and version 2 around 2012 (DO-260B).
/// Version 3 is currently being developed.
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
#[deku(id_type = "u8", bits = "3")]
#[serde(tag = "version")]
pub enum ADSBVersionSurface {
    #[deku(id = "0")]
    #[serde(rename = "0")]
    /// ADS-B version 0 (BDS 6,5 undefined, so these messages should not happen)
    DOC9871AppendixA(Empty),
    #[deku(id = "1")]
    #[serde(rename = "1")]
    /// ADS-B version 1 (2008)
    DOC9871AppendixB(SurfaceV1),
    #[deku(id = "2")]
    #[serde(rename = "2")]
    /// ADS-B version 2 (2012)
    DOC9871AppendixC(SurfaceV2),
    #[deku(id_pat = "3..=7")]
    #[serde(rename = "3to7")]
    Reserved { id: u8 },
}

pub fn serde_default_adsb_version_surface() -> ADSBVersionSurface {
    ADSBVersionSurface::DOC9871AppendixA(Empty {})
}

impl ADSBVersionSurface {
    /// Get the ADS-B version number as an integer
    ///
    /// Returns:
    /// - 0 for DO-260 (not implemented in practice)
    /// - 1 for DO-260A
    /// - 2 for DO-260B
    /// - 3-7 for reserved/future versions
    pub fn version_number(&self) -> u8 {
        match self {
            Self::DOC9871AppendixA(_) => 0,
            Self::DOC9871AppendixB(_) => 1,
            Self::DOC9871AppendixC(_) => 2,
            Self::Reserved { id } => *id,
        }
    }
}

impl fmt::Display for ADSBVersionSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DOC9871AppendixA(_) => write!(f, "Version 0 (DO-260)"),
            Self::DOC9871AppendixB(v1) => {
                writeln!(f, "Version 1 (DO-260A)")?;
                writeln!(f, "   NIC supplement:     {}", v1.nic_s)?;
                writeln!(f, "   NACp:               {}", v1.nac_p)?;
                writeln!(f, "   SIL:                {}", v1.sil)?;
                writeln!(
                    f,
                    "   TAH:                {}",
                    v1.track_angle_or_heading
                )?;
                write!(
                    f,
                    "   HRD:                {}",
                    if v1.horizontal_reference_direction == 0 {
                        "True North"
                    } else {
                        "Magnetic North"
                    }
                )
            }
            Self::DOC9871AppendixC(v2) => {
                writeln!(f, "Version 2 (DO-260B)")?;
                writeln!(f, "   NIC supplement A:   {}", v2.nic_a)?;
                writeln!(f, "   NACp:               {}", v2.nac_p)?;
                writeln!(f, "   SIL:                {}", v2.sil)?;
                writeln!(f, "   SIL supplement:     {}", v2.sil_supplement)?;
                writeln!(
                    f,
                    "   TAH:                {}",
                    v2.track_angle_or_heading
                )?;
                write!(
                    f,
                    "   HRD:                {}",
                    if v2.horizontal_reference_direction == 0 {
                        "True North"
                    } else {
                        "Magnetic North"
                    }
                )
            }
            Self::Reserved { id } => write!(f, "Version {} (Reserved)", id),
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct SurfaceV1 {
    #[deku(bits = "1")]
    #[serde(rename = "NICs")]
    /// NIC supplement bit (NICs)
    pub nic_s: u8,

    #[deku(bits = "4")]
    #[deku(pad_bits_after = "2")] // reserved
    #[serde(rename = "NACp")]
    /// Navigation Accuracy Category for position, version 1,2
    pub nac_p: u8,

    #[deku(bits = "2")]
    #[serde(rename = "SIL")]
    /// Surveillance Integrity Level (SIL)
    pub sil: u8,

    #[deku(bits = "1")]
    #[serde(rename = "TAH")]
    pub track_angle_or_heading: u8,

    #[deku(bits = "1")]
    #[deku(pad_bits_after = "2")]
    #[serde(rename = "HRD")]
    // 1 for magnetic, 0 for true north
    pub horizontal_reference_direction: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct SurfaceV2 {
    #[deku(bits = "1")]
    #[serde(rename = "NICa")]
    /// NIC suppelement A (NICa)
    pub nic_a: u8,

    #[deku(bits = "4")]
    #[deku(pad_bits_after = "2")]
    #[serde(rename = "NACp")]
    /// Navigation Accuracy Category for position (NACp)
    pub nac_p: u8,

    #[deku(bits = "2")]
    #[serde(rename = "SIL")]
    /// Surveillance Integrity Level (SIL)
    pub sil: u8,

    #[deku(bits = "1")]
    #[serde(rename = "TAH")]
    pub track_angle_or_heading: u8,

    #[deku(bits = "1")]
    #[serde(rename = "HRD")]
    // 1 for magnetic, 0 for true north
    pub horizontal_reference_direction: u8,

    #[deku(bits = "1")] // ME 55
    #[deku(pad_bits_after = "1")] // reserved
    #[serde(rename = "SILs")]
    /// SIL supplement bit, only in version 2:
    /// 0 means per hour,
    /// 1 means per sample.
    pub sil_supplement: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct Empty {}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct EmptyU8 {
    pub id: u8,
    pub unused: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct ReservedStatus {
    pub data: [u8; 5],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::DF;
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_bds65_version2_surface() {
        // Real message from 3a33ff - Surface status, version 2 (DO-260B)
        // Version: 2, NICa: 0, NACp: 9, SIL: 0, TAH: 0, HRD: 0, SILs: 0
        let bytes = hex!("903a33fff90200040049001ea8e2");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();

        if let DF::ExtendedSquitterTisB { cf, .. } = msg.df {
            if let ME::BDS65(AircraftOperationStatus::Surface(surface)) = cf.me
            {
                // Check version number method
                assert_eq!(surface.version.version_number(), 2);

                // Check it's a version 2 message
                if let ADSBVersionSurface::DOC9871AppendixC(v2) =
                    surface.version
                {
                    assert_eq!(v2.nic_a, 0);
                    assert_eq!(v2.nac_p, 9);
                    assert_eq!(v2.sil, 0);
                    assert_eq!(v2.sil_supplement, 0);
                    assert_eq!(v2.track_angle_or_heading, 0);
                    assert_eq!(v2.horizontal_reference_direction, 0);
                } else {
                    panic!("Expected version 2");
                }

                // Test Display implementation
                let display = format!("{}", surface.version);
                assert!(display.contains("Version 2 (DO-260B)"));
                assert!(display.contains("NIC supplement A:"));
                assert!(display.contains("NACp:"));
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_adsb_version_display_airborne() {
        // Test Display implementation for airborne versions
        // Since TC=31 subtype 1 (airborne) messages are rare in our samples,
        // we construct the version structures directly to test formatting

        // Test Version 0 (DO-260) - uses Empty struct
        let v0 = ADSBVersionAirborne::DOC9871AppendixA(Empty {});
        let display = format!("{}", v0);
        assert!(display.contains("Version 0 (DO-260)"));
        assert_eq!(v0.version_number(), 0);

        // Test Version 1 (DO-260A)
        let v1 = ADSBVersionAirborne::DOC9871AppendixB(AirborneV1 {
            nic_s: 1,
            nac_p: 9,
            barometric_altitude_quality: 1,
            sil: 2,
            barometric_altitude_integrity: 1,
            horizontal_reference_direction: 0,
        });
        let display = format!("{}", v1);
        assert!(display.contains("Version 1 (DO-260A)"));
        assert!(display.contains("NIC supplement:"));
        assert!(display.contains("NACp:               9"));
        assert!(display.contains("BAQ:                1"));
        assert!(display.contains("SIL:                2"));
        assert!(display.contains("BAI:                1"));
        assert!(display.contains("HRD:                True North"));
        assert_eq!(v1.version_number(), 1);

        // Test Version 2 (DO-260B)
        let v2 = ADSBVersionAirborne::DOC9871AppendixC(AirborneV2 {
            nic_a: 0,
            nac_p: 9,
            geometry_vertical_accuracy: 2,
            sil: 3,
            barometric_altitude_integrity: 1,
            horizontal_reference_direction: 0,
            sil_s: 0,
        });
        let display = format!("{}", v2);
        assert!(display.contains("Version 2 (DO-260B)"));
        assert!(display.contains("NIC supplement A:   0"));
        assert!(display.contains("NACp:               9"));
        assert!(display.contains("GVA:                2"));
        assert!(display.contains("SIL:                3"));
        assert!(display.contains("BAI:                1"));
        assert!(display.contains("HRD:                True North"));
        assert!(display.contains("SIL supplement:     0"));
        assert_eq!(v2.version_number(), 2);

        // Test Reserved version
        let vr = ADSBVersionAirborne::Reserved { id: 5 };
        let display = format!("{}", vr);
        assert!(display.contains("Version 5 (Reserved)"));
        assert_eq!(vr.version_number(), 5);
    }

    #[test]
    fn test_version_number_method() {
        // Test the version_number() helper method for all versions

        // Version 0
        let v0 = ADSBVersionAirborne::DOC9871AppendixA(Empty {});
        assert_eq!(v0.version_number(), 0);

        // Version 1
        let v1 = ADSBVersionAirborne::DOC9871AppendixB(AirborneV1 {
            nic_s: 0,
            nac_p: 0,
            barometric_altitude_quality: 0,
            sil: 0,
            barometric_altitude_integrity: 0,
            horizontal_reference_direction: 0,
        });
        assert_eq!(v1.version_number(), 1);

        // Version 2
        let v2 = ADSBVersionAirborne::DOC9871AppendixC(AirborneV2 {
            nic_a: 0,
            nac_p: 0,
            geometry_vertical_accuracy: 0,
            sil: 0,
            barometric_altitude_integrity: 0,
            horizontal_reference_direction: 0,
            sil_s: 0,
        });
        assert_eq!(v2.version_number(), 2);

        // Reserved versions
        for id in 3..=7 {
            let vr = ADSBVersionAirborne::Reserved { id };
            assert_eq!(vr.version_number(), id);
        }
    }
}
