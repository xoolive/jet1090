//! CAT48 ASTERIX parser for Mode S surveillance data (Monoradar Target Reports)
//!
//! Based on EUROCONTROL ASTERIX CAT048 v1.32 specification.
//!
//! # Supported Data Items
//!
//! | FRN | Data Item | Description | Size |
//! |-----|-----------|-------------|------|
//! | 1 | I048/010 | Data Source Identifier | 2 bytes |
//! | 2 | I048/140 | Time of Day | 3 bytes |
//! | 3 | I048/020 | Target Report Descriptor | Variable (FX) |
//! | 4 | I048/040 | Measured Position in Polar Coordinates | 4 bytes |
//! | 5 | I048/070 | Mode-3/A Code in Octal | 2 bytes |
//! | 6 | I048/090 | Flight Level in Binary | 2 bytes |
//! | 7 | I048/130 | Radar Plot Characteristics | Compound |
//! | 8 | I048/220 | Aircraft Address (ICAO) | 3 bytes |
//! | 9 | I048/240 | Aircraft Identification | 6 bytes |
//! | 10 | I048/250 | Mode S MB Data | Repetitive |
//! | 11 | I048/161 | Track Number | 2 bytes |
//! | 12 | I048/042 | Calculated Position in Cartesian | 4 bytes |
//! | 13 | I048/200 | Calculated Track Velocity | 4 bytes |
//! | 14 | I048/170 | Track Status | Variable (FX) |
//! | 15 | I048/210 | Track Quality | 4 bytes |
//! | 16 | I048/030 | Warning/Error Conditions | Variable (FX) |
//! | 17 | I048/080 | Mode-3/A Code Confidence | 2 bytes |
//! | 18 | I048/100 | Mode-C Code and Confidence | 4 bytes |
//! | 19 | I048/110 | Height Measured by 3D Radar | 2 bytes |
//! | 20 | I048/120 | Radial Doppler Speed | Compound |
//! | 21 | I048/230 | Communications/ACAS Capability | 2 bytes |
//! | 22 | I048/260 | ACAS Resolution Advisory | 7 bytes |
//! | 23 | I048/055 | Mode-1 Code | 1 byte |
//! | 24 | I048/050 | Mode-2 Code | 2 bytes |
//! | 25 | I048/065 | Mode-1 Code Confidence | 1 byte |
//! | 26 | I048/060 | Mode-2 Code Confidence | 2 bytes |
//! | 27 | SP | Special Purpose Field | Explicit |
//! | 28 | RE | Reserved Expansion Field | Explicit |

use super::bds::{
    bds08::callsign_read, decode_payload, DecodedBds, DecodingError,
};
use super::ICAO;
use deku::prelude::*;
use serde::{Serialize, Serializer};

/// Serialize a u8 as a lowercase hex string (e.g., 0x40 → "40")
fn serialize_u8_as_hex<S: Serializer>(v: &u8, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{:02x}", v))
}

/// CAT48 ASTERIX record for Mode S surveillance data
///
/// All data items are optional based on the FSPEC (Field Specification).
/// The FSPEC bits indicate which data items are present in the record.
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct Cat48Record {
    /// Category number (always 48 for this category)
    #[deku(assert_eq = "48")]
    pub cat: u8,

    /// Record length in bytes (including CAT and LEN fields)
    #[deku(endian = "big")]
    #[serde(skip)]
    pub length: u16,

    /// Field Specification (FSPEC) - variable length with FX extension bits
    /// Each byte's bit 0 (FX) indicates if another FSPEC byte follows
    #[deku(reader = "Self::read_fspec(deku::reader)")]
    #[serde(skip)]
    pub fspec: Vec<u8>,

    // FSPEC Byte 1 (FRN 1-7)
    /// I048/010: Data Source Identifier (FRN 1)
    /// Identification of the radar station from which the data are received.
    #[deku(cond = "Self::has_frn(&fspec, 1)")]
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub data_source_id: Option<DataSourceIdentifier>,

    /// I048/140: Time of Day (FRN 2)
    /// Absolute time stamping expressed as UTC in seconds since midnight.
    /// Resolution: 1/128 second ≈ 7.8125 ms
    #[deku(
        cond = "Self::has_frn(&fspec, 2)",
        endian = "big",
        bits = 24,
        map = "|v: Option<u32>| -> Result<_, DekuError> { Ok(v.map(|t| t as f64 / 128.0)) }"
    )]
    #[serde(rename = "time_of_day", skip_serializing_if = "Option::is_none")]
    pub time_of_day: Option<f64>,

    /// I048/020: Target Report Descriptor (FRN 3)
    /// Type and characteristics of the target report.
    #[deku(cond = "Self::has_frn(&fspec, 3)")]
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub target_report_descriptor: Option<TargetReportDescriptor>,

    /// I048/040: Measured Position in Polar Coordinates (FRN 4)
    /// Measured position of an aircraft in local polar coordinates.
    #[deku(cond = "Self::has_frn(&fspec, 4)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured_position: Option<MeasuredPosition>,

    /// I048/070: Mode-3/A Code in Octal Representation (FRN 5)
    /// Mode-3/A code (squawk) converted into octal representation.
    /// Layout: V(1) + G(1) + L(1) + spare(1) + Mode3A(12)
    /// Stored as a 4-digit octal string (e.g., "7700"). None if not validated (V=1).
    #[deku(
        cond = "Self::has_frn(&fspec, 5)",
        endian = "big",
        map = "|v: Option<u16>| -> Result<_, DekuError> { Ok(v.and_then(|code| { if code & 0x8000 != 0 { return None; } Some(format!(\"{:04o}\", code & 0x0FFF)) })) }"
    )]
    #[serde(rename = "squawk", skip_serializing_if = "Option::is_none")]
    pub mode_3a_code: Option<String>,

    /// I048/090: Flight Level in Binary Representation (FRN 6)
    /// Flight level converted to altitude in feet (LSB = 1/4 FL = 25 ft, signed 14 bits).
    /// None if not validated (V=1).
    #[deku(
        cond = "Self::has_frn(&fspec, 6)",
        endian = "big",
        map = "|v: Option<u16>| -> Result<_, DekuError> { Ok(v.and_then(|fl| { if fl & 0x8000 != 0 { return None; } Some((((fl as i16) << 2 >> 2) as i32) * 25) })) }"
    )]
    #[serde(rename = "altitude", skip_serializing_if = "Option::is_none")]
    pub flight_level: Option<i32>,

    /// I048/130: Radar Plot Characteristics (FRN 7)
    /// Additional information on the quality of the target report.
    #[deku(cond = "Self::has_frn(&fspec, 7)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radar_plot_characteristics: Option<RadarPlotCharacteristics>,

    // FSPEC Byte 2 (FRN 8-14)
    /// I048/220: Aircraft Address (FRN 8)
    /// 24-bit Mode S address.
    #[deku(cond = "Self::has_frn(&fspec, 8)")]
    #[serde(rename = "icao24", skip_serializing_if = "Option::is_none")]
    pub aircraft_address: Option<ICAO>,

    /// I048/240: Aircraft Identification (FRN 9)
    /// Target (aircraft or vehicle) identification in 8 characters.
    #[deku(cond = "Self::has_frn(&fspec, 9)")]
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub aircraft_id: Option<AircraftIdentification>,

    /// I048/250: Mode S MB Data (FRN 10)
    /// Mode S Comm B data extracted from the aircraft transponder.
    #[deku(cond = "Self::has_frn(&fspec, 10)")]
    #[serde(rename = "mode_s", skip_serializing_if = "Option::is_none")]
    pub mode_s_mb_data: Option<ModeSMbData>,

    /// I048/161: Track Number (FRN 11)
    /// An integer value representing a unique reference to a track record.
    #[deku(cond = "Self::has_frn(&fspec, 11)", endian = "big")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u16>,

    /// I048/042: Calculated Position in Cartesian Coordinates (FRN 12)
    /// Calculated position of an aircraft in Cartesian coordinates.
    #[deku(cond = "Self::has_frn(&fspec, 12)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculated_position: Option<CalculatedPosition>,

    /// I048/200: Calculated Track Velocity in Polar Representation (FRN 13)
    /// Calculated track velocity expressed in polar coordinates.
    #[deku(cond = "Self::has_frn(&fspec, 13)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_velocity: Option<TrackVelocity>,

    /// I048/170: Track Status (FRN 14)
    /// Status of monoradar track (primary and/or secondary radar).
    #[deku(cond = "Self::has_frn(&fspec, 14)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_status: Option<TrackStatus>,

    // FSPEC Byte 3 (FRN 15-21)
    /// I048/210: Track Quality (FRN 15)
    /// Track quality in the form of a vector of standard deviations.
    #[deku(cond = "Self::has_frn(&fspec, 15)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_quality: Option<TrackQuality>,

    /// I048/030: Warning/Error Conditions (FRN 16)
    /// Warning/error conditions detected by the radar station.
    #[deku(cond = "Self::has_frn(&fspec, 16)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_error: Option<WarningError>,

    /// I048/080: Mode-3/A Code Confidence Indicator (FRN 17)
    /// Confidence level for each bit of a Mode-3/A reply.
    #[deku(cond = "Self::has_frn(&fspec, 17)", endian = "big")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_3a_confidence: Option<u16>,

    /// I048/100: Mode-C Code and Code Confidence Indicator (FRN 18)
    /// Mode-C height in Gray notation with confidence level.
    #[deku(cond = "Self::has_frn(&fspec, 18)", endian = "big")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_c_code: Option<u32>,

    /// I048/110: Height Measured by 3D Radar (FRN 19)
    /// Height measured by a 3D radar (MSL reference).
    #[deku(cond = "Self::has_frn(&fspec, 19)", endian = "big")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_3d: Option<u16>,

    /// I048/120: Radial Doppler Speed (FRN 20)
    /// Information on the Doppler Speed of the target report.
    #[deku(cond = "Self::has_frn(&fspec, 20)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radial_doppler_speed: Option<RadialDopplerSpeed>,

    /// I048/230: Communications/ACAS Capability and Flight Status (FRN 21)
    /// Communications capability of the transponder and ACAS status.
    #[deku(cond = "Self::has_frn(&fspec, 21)", endian = "big")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comms_acas: Option<u16>,

    // FSPEC Byte 4 (FRN 22-28)
    /// I048/260: ACAS Resolution Advisory Report (FRN 22)
    /// Currently active Resolution Advisory.
    #[deku(cond = "Self::has_frn(&fspec, 22)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acas_ra: Option<AcasResolutionAdvisory>,

    /// I048/055: Mode-1 Code in Octal Representation (FRN 23)
    /// Reply to Mode-1 interrogation.
    #[deku(cond = "Self::has_frn(&fspec, 23)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_1_code: Option<u8>,

    /// I048/050: Mode-2 Code in Octal Representation (FRN 24)
    /// Reply to Mode-2 interrogation.
    #[deku(cond = "Self::has_frn(&fspec, 24)", endian = "big")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_2_code: Option<u16>,

    /// I048/065: Mode-1 Code Confidence Indicator (FRN 25)
    /// Confidence level for each bit of a Mode-1 reply.
    #[deku(cond = "Self::has_frn(&fspec, 25)")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_1_confidence: Option<u8>,

    /// I048/060: Mode-2 Code Confidence Indicator (FRN 26)
    /// Confidence level for each bit of a Mode-2 reply.
    #[deku(cond = "Self::has_frn(&fspec, 26)", endian = "big")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_2_confidence: Option<u16>,

    /// SP: Special Purpose Field (FRN 27)
    /// Special Purpose field with explicit length.
    #[deku(
        cond = "Self::has_frn(&fspec, 27)",
        reader = "Self::read_explicit_field(deku::reader)"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special_purpose: Option<Vec<u8>>,

    /// RE: Reserved Expansion Field (FRN 28)
    /// Reserved Expansion field with explicit length.
    #[deku(
        cond = "Self::has_frn(&fspec, 28)",
        reader = "Self::read_explicit_field(deku::reader)"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved_expansion: Option<Vec<u8>>,
}

/// I048/010: Data Source Identifier
/// Identification of the radar station from which the data are received.
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct DataSourceIdentifier {
    /// System Area Code - identifies the geographical area
    #[serde(serialize_with = "serialize_u8_as_hex")]
    pub sac: u8,
    /// System Identification Code - identifies the radar station
    #[serde(serialize_with = "serialize_u8_as_hex")]
    pub sic: u8,
}

/// I048/020: Target Report Descriptor (CAT48 v1.32+)
/// Type and characteristics of the target report.
/// Variable length with FX extension bits (supports up to 3 octets).
/// Octet 1: TYP(3) + SIM(1) + RDP(1) + SPI(1) + RAB(1) + FX(1)
/// Octet 2 (if FX=1): TST(1) + spare(1) + XPP(1) + ME(1) + MI(1) + FOE_FRI(2) + FX(1)
/// Octet 3 (if Extension 1 FX=1): ADSB(2) + SCN(2) + PAI(2) + FX(1) + spare(1)
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct TargetReportDescriptor {
    /// TYP: Detection type (3 bits)
    pub typ: DetectionType,
    /// SIM: Simulated target report
    #[deku(bits = "1")]
    #[serde(skip)]
    pub sim: bool,
    /// RDP: Report from RDP Chain (0=Chain 1, 1=Chain 2)
    #[deku(bits = "1")]
    #[serde(skip)]
    pub rdp: u8,
    /// SPI: Special Position Identification
    #[deku(bits = "1")]
    #[serde(skip)]
    pub spi: bool,
    /// RAB: Report source (0=aircraft transponder, 1=fixed transponder)
    #[deku(bits = "1")]
    #[serde(skip)]
    pub rab: bool,
    /// FX: Extension indicator for first octet
    #[deku(bits = "1")]
    #[serde(skip)]
    fx1: bool,
    /// First extension fields (present if FX bit 1 of octet 1 is set)
    #[deku(cond = "*fx1")]
    #[serde(skip)]
    pub extension1: Option<TrdExtension1>,
}

/// First extension octet of Target Report Descriptor
/// Layout: TST(1) + spare(1) + XPP(1) + ME(1) + MI(1) + FOE_FRI(2) + FX(1)
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct TrdExtension1 {
    /// TST: Test target report
    #[deku(bits = "1", pad_bits_after = "1")]
    pub tst: bool,
    /// XPP: X-Pulse present (v1.21+)
    #[deku(bits = "1")]
    pub xpp: bool,
    /// ME: Military emergency
    #[deku(bits = "1")]
    pub me: bool,
    /// MI: Military identification
    #[deku(bits = "1")]
    pub mi: bool,
    /// FOE/FRI: Friend or Foe identification (2 bits)
    pub foe_fri: FoeFri,
    /// FX: Extension indicator
    #[deku(bits = "1")]
    #[serde(skip)]
    fx: bool,
    /// Second extension fields (present if FX bit of first extension is set)
    #[deku(cond = "*fx")]
    pub extension2: Option<TrdExtension2>,
}

/// Extension indicator from Target Report Descriptor extensions
/// Used in I048/020 Extension 2 (ADSB, SCN, PAI fields)
/// Layout: EP(1) + VAL(1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, Serialize)]
pub struct ExtensionIndicator {
    /// EP: Element Populated (0=not populated, 1=populated)
    #[deku(bits = "1")]
    pub populated: bool,
    /// VAL: Value (0=not available, 1=available)
    #[deku(bits = "1")]
    pub available: bool,
}

/// Second extension octet of Target Report Descriptor (v1.32+)
/// Layout: ADSB(2) + SCN(2) + PAI(2) + FX(1) + spare(1)
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct TrdExtension2 {
    /// ADSB: On-Site ADS-B Information availability
    pub adsb: ExtensionIndicator,
    /// SCN: Surveillance Cluster Network Information availability
    pub scn: ExtensionIndicator,
    /// PAI: Passive Acquisition Interface availability
    pub pai: ExtensionIndicator,
    /// FX: Extension indicator (bit 0) - for future extensions
    #[deku(bits = "1")]
    fx: bool,
    /// Spare bit
    #[deku(bits = "1", pad_bits_after = "0")]
    _spare: u8,
    /// Skip any further FX extension octets beyond Extension 2 (for future compatibility)
    #[deku(cond = "*fx", reader = "TrdExtension2::skip_fx_tail(deku::reader)")]
    #[serde(skip)]
    _fx_tail: Option<()>,
}

impl TrdExtension2 {
    /// Skip any additional FX extension octets beyond Extension 2
    fn skip_fx_tail<R: std::io::Read + std::io::Seek>(
        reader: &mut deku::reader::Reader<R>,
    ) -> Result<Option<()>, DekuError> {
        loop {
            let byte = u8::from_reader_with_ctx(reader, ())?;
            if byte & 0x01 == 0 {
                break;
            }
        }
        Ok(Some(()))
    }
}

/// Detection type from I048/020 TYP field (bits 8-6)
#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, Serialize)]
#[deku(id_type = "u8", bits = "3")]
pub enum DetectionType {
    /// No detection
    #[deku(id = "0")]
    NoDetection,
    /// Single PSR detection
    #[deku(id = "1")]
    SinglePsr,
    /// Single SSR detection
    #[deku(id = "2")]
    SingleSsr,
    /// SSR + PSR detection
    #[deku(id = "3")]
    SsrPsr,
    /// Single Mode S All-Call
    #[deku(id = "4")]
    ModeSAllCall,
    /// Single Mode S Roll-Call
    #[deku(id = "5")]
    ModeSRollCall,
    /// Mode S All-Call + PSR
    #[deku(id = "6")]
    ModeSAllCallPsr,
    /// Mode S Roll-Call + PSR
    #[deku(id = "7")]
    ModeSRollCallPsr,
}

/// FOE/FRI field values from I048/020 extension
#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, Serialize)]
#[deku(id_type = "u8", bits = "2")]
pub enum FoeFri {
    /// No Mode 4 interrogation
    #[deku(id = "0")]
    NoMode4,
    /// Friendly target
    #[deku(id = "1")]
    Friendly,
    /// Unknown target
    #[deku(id = "2")]
    Unknown,
    /// No reply
    #[deku(id = "3")]
    NoReply,
}

/// I048/040: Measured Position in Polar Coordinates
/// Measured position of an aircraft in local polar coordinates.
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct MeasuredPosition {
    /// Range in nautical miles (RHO, resolution: 1/256 NM ≈ 7.2 meters)
    #[deku(
        endian = "big",
        map = "|v: u16| -> Result<_, DekuError> { Ok(v as f64 / 256.0) }"
    )]
    pub rho: f64,
    /// Azimuth in degrees (THETA, resolution: 360/2^16 ≈ 0.0055°)
    #[deku(
        endian = "big",
        map = "|v: u16| -> Result<_, DekuError> { Ok(v as f64 * 360.0 / 65536.0) }"
    )]
    pub theta: f64,
}

/// I048/240: Aircraft Identification (Callsign)
/// 48 bits = 8 characters in IA-5 encoding (6 bits per char).
/// Reuses the callsign parser from BDS 0,8.
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct AircraftIdentification {
    /// Decoded callsign string (8 chars max, trailing spaces removed)
    #[deku(reader = "callsign_read(deku::reader)")]
    pub callsign: String,
}

/// I048/250: Mode S MB Data (repetitive)
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct ModeSMbData {
    /// Number of BDS register data records
    pub count: u8,
    /// BDS register data records (8 bytes each)
    #[deku(count = "count")]
    pub records: Vec<BdsRecord>,
}

/// Custom serialization for Result<DecodedBds, DecodingError>
/// Serializes Ok(value) directly, and Err(error) as {"error": error_message}
fn serialize_decode_result<S>(
    result: &Result<DecodedBds, DecodingError>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match result {
        Ok(decoded) => decoded.serialize(serializer),
        Err(err) => {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("error", &err.to_string())?;
            map.end()
        }
    }
}

/// BDS Register Data record (part of I048/250)
/// Each record is 8 bytes: 7 bytes payload + 1 byte BDS code.
/// The decoded BDS content is eagerly parsed during DekuRead.
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct BdsRecord {
    /// Mode S Comm-B Message Data (56 bits = 7 bytes)
    #[deku(count = "7")]
    #[serde(serialize_with = "super::as_hex")]
    pub payload: Vec<u8>,
    /// BDS Register Address (BDS1 in upper nibble, BDS2 in lower nibble)
    #[serde(rename = "bds", serialize_with = "serialize_u8_as_hex")]
    pub bds_code: u8,
    /// Decoded BDS payload (includes errors from decoding)
    #[deku(skip, default = "decode_payload(&payload, *bds_code)")]
    #[serde(serialize_with = "serialize_decode_result")]
    pub decoded: Result<DecodedBds, DecodingError>,

    /// Inferred BDS codes from payload (using infer_bds)
    #[deku(skip, default = "super::bds::infer_bds(&payload, None)")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inferred: Vec<DecodedBds>,
}

impl BdsRecord {
    /// Get payload as hex string
    pub fn payload_hex(&self) -> String {
        self.payload.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Get BDS code as formatted string (e.g., "40", "50", "60")
    pub fn bds_string(&self) -> String {
        format!("{:02x}", self.bds_code)
    }
}

/// I048/042: Calculated Position in Cartesian Coordinates
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct CalculatedPosition {
    /// X position in nautical miles (LSB = 1/128 NM)
    #[deku(
        endian = "big",
        map = "|v: i16| -> Result<_, DekuError> { Ok(v as f64 / 128.0) }"
    )]
    pub x: f64,
    /// Y position in nautical miles (LSB = 1/128 NM)
    #[deku(
        endian = "big",
        map = "|v: i16| -> Result<_, DekuError> { Ok(v as f64 / 128.0) }"
    )]
    pub y: f64,
}

/// I048/200: Calculated Track Velocity in Polar Representation
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct TrackVelocity {
    /// Ground speed in knots (LSB = 2^-14 NM/s, converted to knots)
    #[deku(
        endian = "big",
        map = "|v: u16| -> Result<_, DekuError> { Ok(v as f64 * 3600.0 / 16384.0) }"
    )]
    pub ground_speed: f64,
    /// Heading in degrees (LSB = 360/2^16)
    #[deku(
        endian = "big",
        map = "|v: u16| -> Result<_, DekuError> { Ok(v as f64 * 360.0 / 65536.0) }"
    )]
    pub heading: f64,
}

/// I048/130: Radar Plot Characteristics (Compound)
/// Additional information on the quality of the target report.
/// First byte is an indicator; bits 7-1 select which subfields follow.
/// Bit 0 (FX) indicates further indicator bytes.
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct RadarPlotCharacteristics {
    /// Indicator byte: bits 7-1 select subfields, bit 0 = FX
    #[serde(skip)]
    indicator: u8,
    /// SRL: SSR Plot Runlength in degrees (LSB = 360/2^13 = 0.0439453125°)
    #[deku(
        cond = "*indicator & 0x80 != 0",
        map = "|v: Option<u8>| -> Result<_, DekuError> { Ok(v.map(|b| b as f64 * 0.0439453125)) }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub srl: Option<f64>,
    /// SRR: Number of received replies for Mode S or Mode A/C
    #[deku(cond = "*indicator & 0x40 != 0")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub srr: Option<u8>,
    /// SAM: SSR reply amplitude in dBm (signed)
    #[deku(
        cond = "*indicator & 0x20 != 0",
        map = "|v: Option<u8>| -> Result<_, DekuError> { Ok(v.map(|b| b as i8)) }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sam: Option<i8>,
    /// PRL: Primary Plot Runlength in degrees (LSB = 360/2^13 = 0.0439453125°)
    #[deku(
        cond = "*indicator & 0x10 != 0",
        map = "|v: Option<u8>| -> Result<_, DekuError> { Ok(v.map(|b| b as f64 * 0.0439453125)) }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prl: Option<f64>,
    /// PAM: Primary plot amplitude in dBm (signed)
    #[deku(
        cond = "*indicator & 0x08 != 0",
        map = "|v: Option<u8>| -> Result<_, DekuError> { Ok(v.map(|b| b as i8)) }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pam: Option<i8>,
    /// RPD: Range difference between PSR and SSR in NM (LSB = 1/256 NM, signed)
    #[deku(
        cond = "*indicator & 0x04 != 0",
        map = "|v: Option<u8>| -> Result<_, DekuError> { Ok(v.map(|b| b as i8 as f64 * 0.00390625)) }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpd: Option<f64>,
    /// APD: Azimuth difference between PSR and SSR in degrees (LSB = 360/2^14 ≈ 0.02197265625°, signed)
    #[deku(
        cond = "*indicator & 0x02 != 0",
        map = "|v: Option<u8>| -> Result<_, DekuError> { Ok(v.map(|b| b as i8 as f64 * 0.02197265625)) }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apd: Option<f64>,
    /// Consume any FX extension bytes
    #[deku(
        cond = "*indicator & 0x01 != 0",
        reader = "RadarPlotCharacteristics::skip_fx_tail(deku::reader)"
    )]
    #[serde(skip)]
    _fx_tail: Option<()>,
}

impl RadarPlotCharacteristics {
    /// Skip any additional FX extension octets
    fn skip_fx_tail<R: std::io::Read + std::io::Seek>(
        reader: &mut deku::reader::Reader<R>,
    ) -> Result<Option<()>, DekuError> {
        loop {
            let byte = u8::from_reader_with_ctx(reader, ())?;
            if byte & 0x01 == 0 {
                break;
            }
        }
        Ok(Some(()))
    }
}

/// I048/170: Track Status (Variable with FX extension)
/// Status of monoradar track (primary and/or secondary radar).
/// Layout octet 1: CNF(1) + RAD(2) + DOU(1) + MAH(1) + CDM(2) + FX(1)
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct TrackStatus {
    /// CNF: Confirmed vs Tentative Track (0=confirmed → true, 1=tentative → false)
    #[deku(
        bits = "1",
        map = "|cnf: bool| -> Result<_, DekuError> { Ok(!cnf) }"
    )]
    pub confirmed: bool,
    /// RAD: Type of sensor maintaining track (2 bits)
    pub sensor_type: SensorType,
    /// DOU: Signals level of confidence in detection
    #[deku(bits = "1")]
    pub doubtful: bool,
    /// MAH: Manoeuvre detection in horizontal sense
    #[deku(bits = "1")]
    pub manoeuvring: bool,
    /// CDM: Climbing/Descending Mode (2 bits)
    pub climb_mode: ClimbMode,
    /// FX: Extension indicator
    #[deku(bits = "1")]
    #[serde(skip)]
    fx: bool,
    /// Extension fields (present if FX=1)
    #[deku(cond = "*fx")]
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub extension: Option<TrackStatusExtension>,
}

/// First extension octet of Track Status (I048/170)
/// Layout: TRE(1) + GHO(1) + SUP(1) + TCC(1) + spare(3) + FX(1)
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct TrackStatusExtension {
    /// TRE: Signal for end of track
    #[deku(bits = "1")]
    pub track_end: bool,
    /// GHO: Ghost vs true target
    #[deku(bits = "1")]
    pub ghost: bool,
    /// SUP: Track maintained with complementary data from neighbouring radar
    #[deku(bits = "1")]
    pub supplementary: bool,
    /// TCC: Type of plot coordinate transformation (0=radar plane, 1=slant range)
    #[deku(bits = "1", pad_bits_after = "3")]
    pub tcc_slant: bool,
    /// FX: Extension indicator
    #[deku(bits = "1")]
    #[serde(skip)]
    fx: bool,
    /// Consume any further FX extension octets (not parsed)
    #[deku(
        cond = "*fx",
        reader = "TrackStatusExtension::skip_fx_tail(deku::reader)"
    )]
    #[serde(skip)]
    _fx_tail: Option<()>,
}

impl TrackStatusExtension {
    /// Skip any additional FX extension octets beyond the first extension
    fn skip_fx_tail<R: std::io::Read + std::io::Seek>(
        reader: &mut deku::reader::Reader<R>,
    ) -> Result<Option<()>, DekuError> {
        loop {
            let byte = u8::from_reader_with_ctx(reader, ())?;
            if byte & 0x01 == 0 {
                break;
            }
        }
        Ok(Some(()))
    }
}

/// Sensor type maintaining the track (RAD field)
#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, Serialize)]
#[deku(id_type = "u8", bits = "2")]
pub enum SensorType {
    /// Combined (PSR + SSR)
    #[deku(id = "0")]
    Combined,
    /// PSR only
    #[deku(id = "1")]
    PsrOnly,
    /// SSR/Mode S only
    #[deku(id = "2")]
    SsrModeS,
    /// Invalid combination
    #[deku(id = "3")]
    Invalid,
}

/// Climbing/Descending Mode (CDM field)
#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, Serialize)]
#[deku(id_type = "u8", bits = "2")]
pub enum ClimbMode {
    /// Maintaining altitude
    #[deku(id = "0")]
    Maintaining,
    /// Climbing
    #[deku(id = "1")]
    Climbing,
    /// Descending
    #[deku(id = "2")]
    Descending,
    /// Unknown
    #[deku(id = "3")]
    Unknown,
}

/// I048/210: Track Quality (4 bytes fixed)
/// Track quality in the form of a vector of standard deviations.
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct TrackQuality {
    /// Standard deviation on X component in NM (LSB = 1/128 NM)
    #[deku(map = "|v: u8| -> Result<_, DekuError> { Ok(v as f64 / 128.0) }")]
    pub sigma_x: f64,
    /// Standard deviation on Y component in NM (LSB = 1/128 NM)
    #[deku(map = "|v: u8| -> Result<_, DekuError> { Ok(v as f64 / 128.0) }")]
    pub sigma_y: f64,
    /// Standard deviation on ground speed in knots (LSB = 2^-14 NM/s)
    #[deku(
        map = "|v: u8| -> Result<_, DekuError> { Ok(v as f64 * 3600.0 / 16384.0) }"
    )]
    pub sigma_v: f64,
    /// Standard deviation on heading in degrees (LSB = 360/2^12)
    #[deku(
        map = "|v: u8| -> Result<_, DekuError> { Ok(v as f64 * 360.0 / 4096.0) }"
    )]
    pub sigma_h: f64,
}

/// I048/030: Warning/Error Conditions (Variable with FX extension)
/// Warning/error conditions detected by the radar station.
/// Each octet: W/E_VALUE(7 bits) + FX(1 bit).
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct WarningError {
    /// List of warning/error codes
    #[deku(reader = "Self::read_codes(deku::reader)")]
    pub codes: Vec<WarningErrorCode>,
}

impl WarningError {
    fn read_codes<R: std::io::Read + std::io::Seek>(
        reader: &mut deku::reader::Reader<R>,
    ) -> Result<Vec<WarningErrorCode>, DekuError> {
        let mut codes = Vec::new();
        loop {
            let entry = WarningErrorEntry::from_reader_with_ctx(reader, ())?;
            let has_fx = entry.fx;
            codes.push(entry.code);
            if !has_fx {
                break;
            }
        }
        Ok(codes)
    }
}

/// Single warning/error entry: code(7 bits) + FX(1 bit)
#[derive(Debug, Clone, PartialEq, DekuRead)]
struct WarningErrorEntry {
    /// Warning/Error code value (bits 7-1)
    code: WarningErrorCode,
    /// FX: Extension indicator (bit 0)
    #[deku(bits = "1")]
    fx: bool,
}

/// Warning/Error Code values from I048/030 (7 bits)
#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, Serialize)]
#[deku(id_type = "u8", bits = "7")]
pub enum WarningErrorCode {
    /// 0: Not defined, application dependent
    #[deku(id = "0")]
    NotDefined,
    /// 1: Multipath Reply (Reflections)
    #[deku(id = "1")]
    MultipathReply,
    /// 2: Reply due to sidelobe interrogation/reception
    #[deku(id = "2")]
    SidelobeReply,
    /// 3: Split plot
    #[deku(id = "3")]
    SplitPlot,
    /// 4: Second time around reply
    #[deku(id = "4")]
    SecondTimeAround,
    /// 5: Angel
    #[deku(id = "5")]
    Angel,
    /// 6: Slow moving target correlated with road infrastructure
    #[deku(id = "6")]
    SlowMovingTarget,
    /// 7: Fixed PSR plot
    #[deku(id = "7")]
    FixedPsrPlot,
    /// 8: Slow PSR target
    #[deku(id = "8")]
    SlowPsrTarget,
    /// 9: Low quality PSR plot
    #[deku(id = "9")]
    LowQualityPsrPlot,
    /// 10: Phantom SSR plot
    #[deku(id = "10")]
    PhantomSsrPlot,
    /// 11: Non-matching Mode-3/A Code
    #[deku(id = "11")]
    NonMatchingMode3A,
    /// 12: Mode C code / Mode S altitude code abnormal value compared to track
    #[deku(id = "12")]
    AbnormalModeC,
    /// 13: Target in clutter area
    #[deku(id = "13")]
    TargetInClutter,
    /// 14: Maximum Doppler Response in Zero filter
    #[deku(id = "14")]
    MaxDopplerInZeroFilter,
    /// 15: Transponder anomaly detected
    #[deku(id = "15")]
    TransponderAnomaly,
    /// 16: Duplicated or illegal Mode S aircraft address
    #[deku(id = "16")]
    DuplicatedModeS,
    /// 17: Mode S error correction applied
    #[deku(id = "17")]
    ModeSErrorCorrected,
    /// 18: Undecodable Mode C code / Mode S altitude code
    #[deku(id = "18")]
    UndecodableModeC,
    /// Unknown code (19-127)
    #[deku(id_pat = "_")]
    Unknown,
}

/// I048/120: Radial Doppler Speed (Compound)
/// Radial Doppler speed calculated from the Doppler filter bank.
/// First byte is an indicator; bit 7 = CAL present, bit 6 = RDS present,
/// bits 5-1 = spare, bit 0 = FX (always 0 for this item).
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct RadialDopplerSpeed {
    /// Indicator byte: bit 7 = CAL, bit 6 = RDS, bits 5-0 = spare+FX
    #[serde(skip)]
    indicator: u8,
    /// Calculated Doppler Speed (if present)
    #[deku(cond = "*indicator & 0x80 != 0")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculated: Option<CalculatedDopplerSpeed>,
    /// Raw Doppler Speed records (repetitive, if present)
    #[deku(
        cond = "*indicator & 0x40 != 0",
        reader = "RadialDopplerSpeed::read_raw_doppler(deku::reader)"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Vec<RawDopplerSpeed>>,
}

impl RadialDopplerSpeed {
    /// Read the repetitive Raw Doppler Speed subfield: 1 byte rep count + N × 6-byte records
    fn read_raw_doppler<R: std::io::Read + std::io::Seek>(
        reader: &mut deku::reader::Reader<R>,
    ) -> Result<Option<Vec<RawDopplerSpeed>>, DekuError> {
        let rep = u8::from_reader_with_ctx(reader, ())?;
        let mut records = Vec::with_capacity(rep as usize);
        for _ in 0..rep {
            records.push(RawDopplerSpeed::from_reader_with_ctx(reader, ())?);
        }
        Ok(Some(records))
    }
}

/// Calculated Doppler Speed subfield of I048/120 (2 bytes = 16 bits)
/// Layout: D(1) + spare(5) + CAL(10 signed)
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct CalculatedDopplerSpeed {
    /// D: Doubtful (0=No, 1=Doppler speed doubtful)
    #[deku(bits = "1", pad_bits_after = "5")]
    pub doubtful: bool,
    /// CAL: Calculated Doppler Speed in m/s (signed, 10 bits, LSB = 1 m/s)
    #[deku(bits = "10")]
    pub speed_ms: i16,
}

/// Raw Doppler Speed subfield of I048/120 (6 bytes)
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct RawDopplerSpeed {
    /// DOP: Doppler Speed (16 bits, LSB = 1 m/s)
    #[deku(endian = "big")]
    pub doppler: u16,
    /// AMB: Ambiguity range (16 bits, LSB = 1 m/s)
    #[deku(endian = "big")]
    pub ambiguity: u16,
    /// FRQ: Transmitter frequency (16 bits, LSB = 1 MHz)
    #[deku(endian = "big")]
    pub frequency: u16,
}

/// I048/260: ACAS Resolution Advisory Report (7 bytes = 56 bits fixed)
/// Currently active Resolution Advisory, if any.
/// Layout: TYP(4) + STYP(4) + ARA(14) + RAC(4) + RAT(1) + MTE(1) + TTI(2) + TID(26) = 56 bits
#[derive(Debug, Clone, PartialEq, DekuRead, Serialize)]
pub struct AcasResolutionAdvisory {
    /// TYP: Message type (4 bits) - 0=RA
    #[deku(bits = "4")]
    pub message_type: u8,
    /// STYP: Message sub-type (4 bits)
    #[deku(bits = "4")]
    pub sub_type: u8,
    /// ARA: Active Resolution Advisory (14 bits)
    #[deku(bits = "14", endian = "big")]
    pub ara: u16,
    /// RAC: RA Complement (4 bits)
    #[deku(bits = "4")]
    pub rac: u8,
    /// RAT: RA Terminated (1 bit)
    #[deku(bits = "1")]
    pub ra_terminated: bool,
    /// MTE: Multiple Threat Encounter (1 bit)
    #[deku(bits = "1")]
    pub multiple_threat: bool,
    /// TTI: Threat Type Indicator (2 bits)
    pub threat_type: ThreatType,
    /// TID: Threat Identity Data (26 bits)
    #[deku(bits = "26", endian = "big")]
    pub threat_id: u32,
}

/// Threat Type Indicator for I048/260
#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, Serialize)]
#[deku(id_type = "u8", bits = "2")]
pub enum ThreatType {
    /// No identity data in TID
    #[deku(id = "0")]
    NoIdentity,
    /// TID contains Mode S address
    #[deku(id = "1")]
    ModeS,
    /// TID contains altitude, range, bearing
    #[deku(id = "2")]
    AltitudeRangeBearing,
    /// Reserved
    #[deku(id = "3")]
    Reserved,
}

impl Cat48Record {
    // FSPEC Reader Functions

    /// Read variable-length FSPEC field (continues while FX bit = 1)
    fn read_fspec<R: std::io::Read + std::io::Seek>(
        reader: &mut deku::reader::Reader<R>,
    ) -> Result<Vec<u8>, DekuError> {
        let mut fspec = Vec::new();
        loop {
            let byte = u8::from_reader_with_ctx(reader, ())?;
            let has_fx = (byte & 0x01) != 0;
            fspec.push(byte);
            if !has_fx {
                break;
            }
        }
        Ok(fspec)
    }

    /// Read explicit length field (first byte is length including itself)
    fn read_explicit_field<R: std::io::Read + std::io::Seek>(
        reader: &mut deku::reader::Reader<R>,
    ) -> Result<Option<Vec<u8>>, DekuError> {
        let len = u8::from_reader_with_ctx(reader, ())?;
        let mut data = vec![len];
        for _ in 1..len {
            let byte = u8::from_reader_with_ctx(reader, ())?;
            data.push(byte);
        }
        Ok(Some(data))
    }

    // FSPEC Helper Functions

    /// Check if a specific FRN (Field Reference Number) is present in FSPEC
    /// FRN is 1-indexed: FRN 1-7 are in byte 0, FRN 8-14 in byte 1, etc.
    fn has_frn(fspec: &[u8], frn: u8) -> bool {
        if frn == 0 {
            return false;
        }
        let byte_idx = ((frn - 1) / 7) as usize;
        let bit_pos = 7 - ((frn - 1) % 7);

        if byte_idx >= fspec.len() {
            return false;
        }

        (fspec[byte_idx] & (1 << bit_pos)) != 0
    }

    // Accessor Methods

    /// Get SAC (System Area Code) if present
    pub fn sac(&self) -> Option<u8> {
        self.data_source_id.as_ref().map(|d| d.sac)
    }

    /// Get SIC (System Identification Code) if present
    pub fn sic(&self) -> Option<u8> {
        self.data_source_id.as_ref().map(|d| d.sic)
    }

    /// Get range in nautical miles (if position is present)
    pub fn range_nm(&self) -> Option<f64> {
        self.measured_position.as_ref().map(|p| p.rho)
    }

    /// Get azimuth in degrees (if position is present)
    pub fn azimuth_deg(&self) -> Option<f64> {
        self.measured_position.as_ref().map(|p| p.theta)
    }

    /// Get aircraft callsign (if aircraft identification is present)
    pub fn callsign(&self) -> Option<&str> {
        self.aircraft_id.as_ref().map(|id| id.callsign.as_str())
    }

    /// Get track number (bits 11-0)
    pub fn track_num(&self) -> Option<u16> {
        self.track_number.map(|t| t & 0x0FFF)
    }

    /// Get ground speed in knots (if track velocity is present)
    pub fn ground_speed_kt(&self) -> Option<f64> {
        self.track_velocity.as_ref().map(|v| v.ground_speed)
    }

    /// Get heading in degrees (if track velocity is present)
    pub fn heading_deg(&self) -> Option<f64> {
        self.track_velocity.as_ref().map(|v| v.heading)
    }

    /// Get target report type from TRD
    pub fn target_type(&self) -> Option<DetectionType> {
        self.target_report_descriptor.as_ref().map(|trd| trd.typ)
    }

    /// Check if target is simulated (SIM bit in TRD)
    pub fn is_simulated(&self) -> bool {
        self.target_report_descriptor
            .as_ref()
            .map(|trd| trd.sim)
            .unwrap_or(false)
    }

    /// Check if this record contains BDS data
    pub fn has_bds_data(&self) -> bool {
        self.mode_s_mb_data
            .as_ref()
            .map(|mb| mb.count > 0)
            .unwrap_or(false)
    }

    /// Get BDS records if present
    pub fn bds_records(&self) -> Option<&[BdsRecord]> {
        self.mode_s_mb_data.as_ref().map(|mb| mb.records.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test parsing format CAT48 record with BDS data
    #[test]
    fn test_parse_record_with_bds() {
        let data = hex::decode(
            "300045fda30301b834010d407fa1003ff612e087a105a0780a89\
             0458b984034a980805d9ca2933e00ffe60803a6b300004f650\
             c6500030a400004000002040210d00919cc2",
        )
        .unwrap();

        let (_, record) = Cat48Record::from_bytes((&data, 0))
            .expect("Failed to parse record");

        // Basic fields
        assert_eq!(record.cat, 48);
        assert_eq!(record.length, 69);
        assert_eq!(record.fspec, vec![0xFD, 0xA3, 0x03, 0x01, 0xB8]);

        // I048/010: Data Source
        assert_eq!(record.sac(), Some(52));
        assert_eq!(record.sic(), Some(1));

        // I048/140: Time of Day
        assert!((record.time_of_day.unwrap() - 6784.992).abs() < 0.001);

        // I048/020: TRD
        let trd = record.target_report_descriptor.as_ref().unwrap();
        assert_eq!(trd.typ, DetectionType::ModeSRollCall);
        assert!(!trd.sim);
        assert_eq!(trd.rdp, 0);
        assert!(!trd.spi);
        assert!(!trd.rab);
        assert!(trd.extension1.is_some());
        assert_eq!(record.target_type(), Some(DetectionType::ModeSRollCall));

        // I048/040: Position
        assert!((record.range_nm().unwrap() - 63.961).abs() < 0.01);
        assert!((record.azimuth_deg().unwrap() - 26.543).abs() < 0.01);

        // I048/070: Mode 3/A (V=1, invalid)
        assert!(record.mode_3a_code.is_none());

        // I048/090: Flight Level
        assert_eq!(record.flight_level, Some(36000));

        // I048/220: ICAO
        assert_eq!(
            format!("{}", record.aircraft_address.as_ref().unwrap()),
            "780a89"
        );

        // I048/250: BDS data
        assert!(record.has_bds_data());
        let bds = record.bds_records().unwrap();
        assert_eq!(bds.len(), 4);
        assert_eq!(bds[0].bds_code, 0x05);
        assert_eq!(bds[1].bds_code, 0x60);
        assert_eq!(bds[2].bds_code, 0x50);
        assert_eq!(bds[3].bds_code, 0x40);

        // I048/170: Track Status (0x00: confirmed, combined, no doubtful/manoeuvre, maintaining)
        let ts = record.track_status.as_ref().unwrap();
        assert!(ts.confirmed);
        assert_eq!(ts.sensor_type, SensorType::Combined);
        assert!(!ts.doubtful);
        assert!(!ts.manoeuvring);
        assert_eq!(ts.climb_mode, ClimbMode::Maintaining);
        assert!(ts.extension.is_none()); // No FX extension

        // I048/230: Comms/ACAS
        assert_eq!(record.comms_acas, Some(0x0020));
    }

    /// Test parsing format CAT48 record without BDS data
    #[test]
    fn test_parse_record_without_bds() {
        let data = hex::decode(
            "300024fd830301b834010d40b1a100bf821cb48c1a0640c058c000002040210d00919cc9",
        )
        .unwrap();

        let (_, record) = Cat48Record::from_bytes((&data, 0))
            .expect("Failed to parse record without BDS");

        assert_eq!(record.cat, 48);
        assert_eq!(record.length, 36);
        assert_eq!(record.fspec, vec![0xFD, 0x83, 0x03, 0x01, 0xB8]);
        assert_eq!(record.sac(), Some(0x34));
        assert_eq!(record.sic(), Some(0x01));
        assert!((record.time_of_day.unwrap() - 6785.38).abs() < 0.01);
        assert!((record.range_nm().unwrap() - 191.508).abs() < 0.01);
        assert!((record.azimuth_deg().unwrap() - 40.364).abs() < 0.01);
        assert_eq!(record.flight_level, Some(40000));
        assert_eq!(
            format!("{}", record.aircraft_address.as_ref().unwrap()),
            "c058c0"
        );
        assert!(!record.has_bds_data());
    }

    /// Test parsing CroatiaControl sample CAT48 record
    #[test]
    fn test_parse_croatia_control_sample() {
        let data = hex::decode(
            "300030fdf70219c9356d4da0c5aff1e00200052\
             83c660c10c236d41820\
             01c0780031bc000040\
             0deb07b9582e41002\
             0f5",
        )
        .unwrap();

        let (_, record) = Cat48Record::from_bytes((&data, 0))
            .expect("Failed to parse CroatiaControl sample");

        // Basic fields
        assert_eq!(record.cat, 48);
        assert_eq!(record.length, 48);
        assert_eq!(record.fspec, vec![0xFD, 0xF7, 0x02]);

        // I048/010: Data Source
        assert_eq!(record.sac(), Some(25));
        assert_eq!(record.sic(), Some(201));

        // I048/140: Time of Day (~27354.6 seconds = ~7:35:54 UTC)
        assert!((record.time_of_day.unwrap() - 27354.602).abs() < 0.01);

        // I048/020: TRD (single byte, no FX)
        let trd = record.target_report_descriptor.as_ref().unwrap();
        assert_eq!(trd.typ, DetectionType::ModeSRollCall);
        assert!(!trd.sim);
        assert!(trd.extension1.is_none()); // No FX extension
        assert_eq!(record.target_type(), Some(DetectionType::ModeSRollCall));

        // I048/040: Position
        assert!((record.range_nm().unwrap() - 197.684).abs() < 0.01);
        assert!((record.azimuth_deg().unwrap() - 340.137).abs() < 0.01);

        // I048/070: Mode 3/A = 1000 (octal)
        assert_eq!(record.mode_3a_code, Some("1000".to_string()));

        // I048/090: Flight Level = 33000 ft
        assert_eq!(record.flight_level, Some(33000));

        // I048/220: ICAO = 3C660C
        assert_eq!(
            format!("{}", record.aircraft_address.as_ref().unwrap()),
            "3c660c"
        );

        // I048/240: Aircraft Identification (callsign)
        assert!(record.aircraft_id.is_some());
        // Callsign decoding - raw bytes: 10 c2 36 d4 18 20

        // I048/250: BDS data (1 record with BDS 40)
        assert!(record.has_bds_data());
        let bds = record.bds_records().unwrap();
        assert_eq!(bds.len(), 1);
        assert_eq!(bds[0].bds_code, 0x40);
        assert_eq!(bds[0].payload_hex(), "c0780031bc0000");

        // I048/161: Track Number
        assert_eq!(record.track_num(), Some(3563));

        // I048/200: Track Velocity
        assert!((record.ground_speed_kt().unwrap() - 434.4).abs() < 1.0);
        assert!((record.heading_deg().unwrap() - 124.0).abs() < 0.1);

        // I048/170: Track Status (0x41=0b01000001 with FX, 0x00 extension)
        let ts = record.track_status.as_ref().unwrap();
        assert!(ts.confirmed); // CNF=0
        assert_eq!(ts.sensor_type, SensorType::SsrModeS); // RAD=10
        assert!(!ts.doubtful);
        assert!(!ts.manoeuvring);
        assert_eq!(ts.climb_mode, ClimbMode::Maintaining); // CDM=00
        let ext = ts.extension.as_ref().unwrap();
        assert!(!ext.track_end); // TRE=0
        assert!(!ext.ghost); // GHO=0
        assert!(!ext.supplementary); // SUP=0
        assert!(!ext.tcc_slant); // TCC=0

        // I048/230: Comms/ACAS
        assert_eq!(record.comms_acas, Some(0x20F5));
    }

    /// Test JSON serialization of CAT48 record with converted values
    #[test]
    fn test_json_serialization() {
        let data = hex::decode(
            "300030fdf70219c9356d4da0c5aff1e00200052\
             83c660c10c236d41820\
             01c0780031bc000040\
             0deb07b9582e41002\
             0f5",
        )
        .unwrap();

        let (_, record) = Cat48Record::from_bytes((&data, 0))
            .expect("Failed to parse CroatiaControl sample");

        let json = serde_json::to_string_pretty(&record).unwrap();
        println!("{}", json);

        // Parse back and verify converted values are in the JSON
        let json_value: serde_json::Value =
            serde_json::from_str(&json).unwrap();

        // Check measured_position has converted values (rho in NM, theta in degrees)
        let mp = &json_value["measured_position"];
        assert!((mp["rho"].as_f64().unwrap() - 197.684).abs() < 0.01);
        assert!((mp["theta"].as_f64().unwrap() - 340.137).abs() < 0.01);

        // Check track_velocity has converted values (ground_speed in kt, heading in deg)
        let tv = &json_value["track_velocity"];
        assert!((tv["ground_speed"].as_f64().unwrap() - 434.4).abs() < 1.0);
        assert!((tv["heading"].as_f64().unwrap() - 124.0).abs() < 0.1);

        // Check track_status is properly decoded
        let ts = &json_value["track_status"];
        assert!(ts["confirmed"].as_bool().unwrap()); // CNF=0 means confirmed
        assert_eq!(ts["sensor_type"].as_str().unwrap(), "SsrModeS");

        // Verify ICAO is serialized as hex string
        assert_eq!(json_value["icao24"].as_str().unwrap(), "3c660c");

        // Verify callsign is flattened into the top-level object
        assert!(json_value["callsign"].as_str().is_some());
    }

    /// Test parsing CAT48 record with Target Report Descriptor Extension 2 (v1.32)
    /// Extension 2 contains ADSB, SCN, and PAI availability bits
    #[test]
    fn test_trd_extension2_parsing() {
        // Construct a minimal CAT48 record with I048/020 containing Extension 2
        // CAT: 48, LEN: 9 bytes
        // FSPEC: 0xA0 (bits for FRN1=I048/010 and FRN3=I048/020)
        // I048/010: SAC=0x00, SIC=0x01
        // I048/020:
        //   Octet 1: TYP=001(PSR) + SIM=0 + RDP=0 + SPI=0 + RAB=0 + FX=1
        //   Ext 1: TST=0 + spare=0 + XPP=0 + ME=0 + MI=0 + FOE_FRI=00 + FX=1
        //   Ext 2: ADSB(01: not populated, available) + SCN(10: populated, not available)
        //          + PAI(11: populated, available) + FX=0 + spare=0
        let data = hex::decode("300009a0000121016c").unwrap();

        let (_, record) = Cat48Record::from_bytes((&data, 0))
            .expect("Failed to parse CAT48 with Extension 2");

        // Verify SAC/SIC
        assert_eq!(record.sac(), Some(0x00));
        assert_eq!(record.sic(), Some(0x01));

        // Verify TRD basic fields
        assert_eq!(
            record.target_report_descriptor.as_ref().unwrap().typ,
            DetectionType::SinglePsr
        );

        // Verify Extension 1 exists
        let ext1 = record
            .target_report_descriptor
            .as_ref()
            .unwrap()
            .extension1
            .as_ref();
        assert!(ext1.is_some(), "Extension 1 should be present");

        // Verify Extension 2 exists and contains correct values
        let ext2 = ext1.unwrap().extension2.as_ref();
        assert!(ext2.is_some(), "Extension 2 should be present");

        let ext2_data = ext2.unwrap();
        // ADSB: EP=0, VAL=1
        assert!(!ext2_data.adsb.populated);
        assert!(ext2_data.adsb.available);
        // SCN: EP=1, VAL=0
        assert!(ext2_data.scn.populated);
        assert!(!ext2_data.scn.available);
        // PAI: EP=1, VAL=1
        assert!(ext2_data.pai.populated);
        assert!(ext2_data.pai.available);
    }
}
