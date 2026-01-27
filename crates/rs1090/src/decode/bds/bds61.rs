use crate::decode::IdentityCode;
use deku::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;

/**
 * ## Aircraft Status (BDS 6,1 / TYPE=28)
 *
 * Extended Squitter ADS-B message providing aircraft emergency/priority status.  
 * Per ICAO Doc 9871 Table B-2-97a/97b: BDS code 6,1 — Aircraft status
 *
 * Purpose: To provide additional information on aircraft emergency status
 * and ACAS Resolution Advisories.
 *
 * Message Structure (56 bits):
 * | TYPE | SUBTYPE | EMERGENCY | SQUAWK | RESERVED |
 * |------|---------|-----------|--------|----------|
 * | 5    | 3       | 3         | 13     | 32       |
 *
 * Field Encoding per ICAO Doc 9871:
 *
 * **TYPE Code** (bits 1-5): Fixed value 28 (11100 binary)
 *
 * **Subtype** (bits 6-8): 3-bit subtype field
 *   - 0 = No information
 *   - 1 = Emergency/priority status (Table B-2-97a)
 *   - 2 = ACAS RA Broadcast (Table B-2-97b)
 *   - 3-7 = Reserved
 *
 * **Emergency State** (bits 9-11): 3-bit emergency code (Subtype 1)
 *   - 0 = No emergency
 *   - 1 = General emergency (Mode A code 7700)
 *   - 2 = Lifeguard/Medical emergency
 *   - 3 = Minimum fuel
 *   - 4 = No communications (Mode A code 7600)
 *   - 5 = Unlawful interference (Mode A code 7500)
 *   - 6 = Downed aircraft
 *   - 7 = Reserved
 *
 * **Mode A Code (Squawk)** (bits 12-24): 13-bit identity code
 *   - Standard 4-digit octal squawk code
 *   - Encoded in Gillham format
 *
 * **Reserved** (bits 25-56): Reserved bits
 *
 * Transmission Rules per ICAO Doc 9871:
 * - Message delivery accomplished once per 0.8 seconds (event-driven protocol)
 * - Subtype 2 (ACAS RA) takes priority over Subtype 1 (emergency)
 * - Termination detected by coding in surveillance status field of
 *   airborne position message
 *
 * Mode A Code Mapping:
 * - Emergency State 1 set when Mode A code 7700 provided to transponder
 * - Emergency State 4 set when Mode A code 7600 provided to transponder
 * - Emergency State 5 set when Mode A code 7500 provided to transponder
 *
 * Important Note per ICAO Doc 9871:
 * - This data is NOT intended for extraction using GICB or ACAS cross-link
 * - Read out of this register is discouraged (contents are indeterminate)
 * - This is an Extended Squitter message, not a Comm-B register
 *
 * Reference: ICAO Doc 9871 Tables B-2-97a and B-2-97b
 */
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct AircraftStatus {
    /// Subtype (bits 6-8): Per ICAO Doc 9871 Table B-2-97a
    /// Identifies message content type.
    /// Encoding:
    ///   - 0 = No information
    ///   - 1 = Emergency/priority status
    ///   - 2 = ACAS RA Broadcast
    ///   - 3-7 = Reserved
    ///
    /// Subtype 2 takes priority over Subtype 1 for transmission.
    pub subtype: AircraftStatusType,

    /// Emergency State (bits 9-11): Per ICAO Doc 9871 Table B-2-97a
    /// 3-bit emergency status code (valid for Subtype 1).
    /// Encoding:
    ///   - 0 = No emergency
    ///   - 1 = General emergency (Mode A code 7700)
    ///   - 2 = Lifeguard/Medical emergency
    ///   - 3 = Minimum fuel
    ///   - 4 = No communications (Mode A code 7600)
    ///   - 5 = Unlawful interference (Mode A code 7500)
    ///   - 6 = Downed aircraft
    ///   - 7 = Reserved
    ///
    /// Message delivered once per 0.8 seconds (event-driven).
    /// Termination detected via surveillance status in airborne position message.
    pub emergency_state: EmergencyState,

    /// Mode A Code / Squawk (bits 12-24): Per ICAO Doc 9871 Table B-2-97a
    /// 13-bit identity code (standard 4-digit octal squawk).
    /// Encoded in Gillham format.
    pub squawk: IdentityCode,
}

impl fmt::Display for AircraftStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  Aircraft Status (BDS 6,1)")?;
        writeln!(f, "  Squawk:        {:x?}", &self.squawk)?;
        writeln!(f, "  Emergency/priority:    {}", &self.emergency_state)?;
        Ok(())
    }
}

/// Aircraft Status Subtype: Per ICAO Doc 9871 Table B-2-97a
/// Identifies message content type.
/// Encoding:
///   - 0 = No information
///   - 1 = Emergency/priority status
///   - 2 = ACAS RA Broadcast
///   - 3-7 = Reserved
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
#[deku(id_type = "u8", bits = "3")]
#[serde(rename_all = "snake_case")]
pub enum AircraftStatusType {
    /// Subtype 0: No information available
    #[deku(id = "0")]
    NoInformation,

    /// Subtype 1: Emergency/priority status
    /// Contains emergency state and Mode A code.
    /// Transmitted once per 0.8 seconds when emergency active.
    #[deku(id = "1")]
    #[serde(rename = "emergency_priority")]
    EmergencyPriorityStatus,

    /// Subtype 2: ACAS RA Broadcast
    /// Contains ACAS Resolution Advisory information.
    /// Takes priority over Subtype 1 for transmission.
    #[deku(id = "2")]
    #[serde(rename = "acas_ra")]
    ACASRaBroadcast,

    /// Subtypes 3-7: Reserved for future use
    #[deku(id_pat = "_")]
    Reserved,
}

/// Emergency State (bits 9-11): Per ICAO Doc 9871 Table B-2-97a
/// 3-bit emergency status code (valid for Subtype 1).
/// Encoding:
///   - 0 = No emergency
///   - 1 = General emergency (Mode A code 7700)
///   - 2 = Lifeguard/Medical emergency
///   - 3 = Minimum fuel
///   - 4 = No communications (Mode A code 7600)
///   - 5 = Unlawful interference (Mode A code 7500)
///   - 6 = Downed aircraft
///   - 7 = Reserved
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "3")]
#[serde(rename_all = "snake_case")]
pub enum EmergencyState {
    /// No emergency condition
    None = 0,

    /// General emergency (Mode A code 7700)
    /// Set when Mode A code 7700 provided to transponder.
    General = 1,

    /// Lifeguard/Medical emergency
    Medical = 2,

    /// Minimum fuel condition
    MinimumFuel = 3,

    /// No communications (Mode A code 7600)
    /// Set when Mode A code 7600 provided to transponder.
    NoCommunication = 4,

    /// Unlawful interference (Mode A code 7500)
    /// Set when Mode A code 7500 provided to transponder.
    UnlawfulInterference = 5,

    /// Downed aircraft
    DownedAircraft = 6,

    /// Reserved for future use
    Reserved = 7,
}

impl fmt::Display for EmergencyState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::None => "No emergency",
            Self::General => "General emergency (7700)",
            Self::Medical => "Lifeguard/Medical emergency",
            Self::MinimumFuel => "Minimum fuel",
            Self::NoCommunication => "No communication (7600)",
            Self::UnlawfulInterference => "Unlawful interference (7500)",
            Self::DownedAircraft => "Downed aircraft",
            Self::Reserved => "Reserved",
        };
        write!(f, "{s}")?;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq, DekuRead, Copy, Clone)]
pub struct OperationCodeSurface {
    #[deku(bits = "1")]
    pub poe: u8,
    #[deku(bits = "1")]
    pub cdti: u8,
    #[deku(bits = "1")]
    pub b2_low: u8,
    #[deku(bits = "3")]
    #[deku(pad_bits_before = "6")]
    pub lw: u8,
}
