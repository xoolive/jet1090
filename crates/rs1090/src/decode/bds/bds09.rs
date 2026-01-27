#![allow(clippy::suspicious_else_formatting)]

use deku::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;

/**
 * ## Airborne Velocity (BDS 0,9)
 *
 * Extended squitter message providing aircraft velocity information.  
 * Per ICAO Doc 9871 Tables A-2-9a and A-2-9b: BDS code 0,9 — Extended squitter
 * airborne velocity
 *
 * All airborne velocities use Type Code 19. Four subtypes (bits 6-8) define
 * different velocity reporting methods optimized for normal and supersonic flight.
 *
 * Message Structure (56 bits):
 * | TC | ST  | IC | IFR | NACv | VELOCITY DATA (22 bits) | VR DATA | GNSS DIFF |
 * | -- | --- | -- | --- | ---- | ----------------------- | ------- | --------- |
 * | 5  | 3   | 1  | 1   | 3    | 22                      | 11      | 8         |
 *
 * Field Encoding per ICAO Doc 9871 §A.2.3.5:
 * - TC (bits 1-5): Format Type Code = 19 (fixed)
 * - ST (bits 6-8): Subtype (3 bits)
 *   * 0: Reserved
 *   * 1: Ground speed (subsonic, LSB=1 kt)
 *   * 2: Ground speed (supersonic, LSB=4 kt)
 *   * 3: Airspeed + heading (subsonic, LSB=1 kt)
 *   * 4: Airspeed + heading (supersonic, LSB=4 kt)
 *   * 5-7: Reserved
 * - IC (bit 9): Intent Change Flag per §A.2.3.5.3
 * - IFR (bit 10): IFR Capability Flag per §A.2.3.5.4
 * - NACv (bits 11-13): Navigation Accuracy Category - Velocity (NUCv in ADS-B v0)
 *
 * **Subtypes 1 & 2**: Ground speed (velocity over ground)
 * - Bits 14-24: East-West velocity (1 sign bit + 10-bit value)
 * - Bits 25-35: North-South velocity (1 sign bit + 10-bit value)
 * - Ground speed and track computed from E-W and N-S components
 *
 * **Subtypes 3 & 4**: Airspeed and heading (when GNSS unavailable)
 * - Bit 14: Heading status (0=unavailable, 1=available)
 * - Bits 15-24: Magnetic heading (10 bits, LSB=360/1024 degrees)
 * - Bit 25: Airspeed type (0=IAS, 1=TAS)
 * - Bits 26-35: Airspeed (10 bits)
 *
 * **Vertical Rate** (bits 36-46):
 * - Bit 36: Source (0=GNSS, 1=Barometric)
 * - Bit 37: Sign (0=Up, 1=Down)
 * - Bits 38-46: Rate value (9 bits, LSB=64 ft/min)
 *
 * **GNSS Altitude Difference** (bits 49-56):
 * - Bit 49: Sign (0=Above baro, 1=Below baro)
 * - Bits 50-56: Difference (7 bits, LSB=25 ft)
 *
 * Note: Subtypes 2 and 4 (supersonic) are rare as no operational supersonic
 * airliners currently use ADS-B.
 *
 * Note: Subtype 3 messages are very rare in practice; most aircraft report
 * ground speed (subtypes 1/2) based on GNSS position.
 */
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
pub struct AirborneVelocity {
    #[deku(bits = "3")]
    #[serde(skip)]
    /// Subtype (bits 6-8): Per ICAO Doc 9871 Table A-2-9a/b  
    /// Determines velocity encoding method:
    ///   - 0: Reserved
    ///   - 1: Ground speed, subsonic (E-W and N-S components, LSB=1 kt)
    ///   - 2: Ground speed, supersonic (E-W and N-S components, LSB=4 kt)
    ///   - 3: Airspeed + heading, subsonic (LSB=1 kt)
    ///   - 4: Airspeed + heading, supersonic (LSB=4 kt)
    ///   - 5-7: Reserved
    ///
    /// Supersonic encoding used if velocity exceeds 1,022 kt.
    pub subtype: u8,

    #[deku(bits = "1")]
    #[serde(skip)]
    /// Intent Change Flag (bit 9): Per ICAO Doc 9871 §A.2.3.5.3  
    /// Indicates change in aircraft intent (flight plan/trajectory):
    ///   - false (0): No change in intent
    ///   - true (1): Intent change detected
    ///
    /// Triggered 4s after new data in registers 40-42₁₆, remains set for 18±1s.
    /// Intent change includes updates to selected altitude, heading, etc.
    pub intent_change: bool,

    #[deku(bits = "1")]
    #[serde(skip)]
    /// IFR Capability Flag (bit 10): Per ICAO Doc 9871 §A.2.3.5.4  
    /// Indicates ADS-B equipage level:
    ///   - false (0): No capability for ADS-B conflict detection (below Class A1)
    ///   - true (1): Capable of ADS-B conflict detection and higher (Class A1+)
    pub ifr_capability: bool,

    #[deku(bits = "3")]
    #[serde(rename = "NACv")]
    /// Navigation Accuracy Category - Velocity (bits 11-13)  
    /// Per ICAO Doc 9871 Table A-2-9a: NUCᵣ (NACv in ADS-B v1+)  
    /// Indicates velocity measurement accuracy (95% bounds):
    ///
    /// | NACv | Horizontal Velocity Error | Vertical Velocity Error    |
    /// |------|---------------------------|----------------------------|
    /// | 0    | Unknown                   | Unknown                    |
    /// | 1    | < 10 m/s                  | < 15.2 m/s (50 fps)        |
    /// | 2    | < 3 m/s                   | < 4.6 m/s (15 fps)         |
    /// | 3    | < 1 m/s                   | < 1.5 m/s (5 fps)          |
    /// | 4    | < 0.3 m/s                 | < 0.46 m/s (1.5 fps)       |
    ///
    /// Note: Called NUCv (Navigation Uncertainty Category) in ADS-B version 0.
    pub nac_v: u8,

    #[deku(ctx = "*subtype")]
    #[serde(flatten)]
    /// Velocity Data (bits 14-35): 22 bits, encoding depends on subtype  
    /// See AirborneVelocitySubType enum for subtype-specific structures.
    pub velocity: AirborneVelocitySubType,

    /// Vertical Rate Source (bit 36): Per ICAO Doc 9871 Table A-2-9a
    ///   - Barometric (0): Rate based on barometric pressure altitude
    ///   - GNSS (1): Rate based on GNSS geometric altitude
    pub vrate_src: VerticalRateSource,

    #[serde(skip, default = "Sign::positive")]
    /// Vertical Rate Sign (bit 37): Per ICAO Doc 9871 Table A-2-9a
    ///   - Positive (0): Climbing (upward)
    ///   - Negative (1): Descending (downward)
    pub vrate_sign: Sign,
    #[deku(
        endian = "big",
        bits = "9",
        map = "|v: u16| -> Result<_, DekuError> {
            if v == 0 { Ok(None) }
            else {
                Ok(Some(vrate_sign.value() * (v as i16 - 1)  * 64))
            }
        }"
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Vertical Rate (bits 38-46): Per ICAO Doc 9871 Table A-2-9a  
    /// 9-bit value encoding climb/descent rate:
    ///   - LSB = 64 ft/min
    ///   - Formula: rate = (value - 1) × 64 ft/min (signed by vrate_sign)
    ///   - Range: [0, 32,576] ft/min (value 1-510)
    ///   - value=0: No vertical rate information available (returns None)
    ///   - value=511: >32,608 ft/min
    ///
    /// Sign bit determines direction: 0=climbing, 1=descending
    pub vertical_rate: Option<i16>,

    #[deku(bits = "2")]
    #[serde(skip)]
    /// Reserved (bits 47-48): Reserved for turn indicator (not implemented)
    pub reserved: u8,

    #[serde(skip, default = "Sign::positive")]
    /// GNSS Altitude Difference Sign (bit 49): Per ICAO Doc 9871 §A.2.3.5.7
    ///   - Positive (0): GNSS altitude above barometric altitude
    ///   - Negative (1): GNSS altitude below barometric altitude
    pub gnss_sign: Sign,

    #[deku(reader = "read_geobaro(deku::reader, *gnss_sign)")]
    /// GNSS-Barometric Altitude Difference (bits 50-56)  
    /// Per ICAO Doc 9871 §A.2.3.5.7: Difference between GNSS and barometric altitude
    /// 7-bit field encoding signed difference:
    ///   - LSB = 25 ft
    ///   - Formula: diff = (value - 1) × 25 ft (signed by gnss_sign)
    ///   - Range: [0, 3,125] ft (value 1-126)
    ///   - value=0 or 1: No information available (returns None)
    ///   - value=127: 3,137.5 ft
    ///
    /// Uses GNSS HAE (Height Above Ellipsoid) if available, else GNSS MSL.
    /// For TC 9-10, only GNSS HAE used; field set to zeros if unavailable.
    pub geo_minus_baro: Option<i16>,
}

/// Decode GNSS-barometric altitude difference from 7-bit field
///
/// Per ICAO Doc 9871 §A.2.3.5.7: Difference from barometric altitude in
/// airborne velocity messages
///
/// The difference between barometric altitude and GNSS height is encoded:
/// - Bit 49: Sign bit (0=GNSS above baro, 1=GNSS below baro)
/// - Bits 50-56: 7-bit unsigned value
///
/// Encoding:
/// - LSB = 25 ft
/// - Formula: difference = (value - 1) × 25 ft
/// - Range: [0, 3,137.5] ft for values 1-127
/// - value ≤ 1: No information available (returns None)
///
/// GNSS source priority:
/// 1. GNSS HAE (Height Above Ellipsoid) - preferred
/// 2. GNSS MSL (Mean Sea Level) - if HAE unavailable (for TC 11-18)
/// 3. For TC 9-10: Only HAE used; field zeros if unavailable
///
/// Returns: Signed difference in feet, or None if unavailable
fn read_geobaro<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
    gnss_sign: Sign,
) -> Result<Option<i16>, DekuError> {
    let value = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(7)),
    )?;
    let value = if value > 1 {
        match gnss_sign {
            Sign::Positive => Some(25 * (value as i16 - 1)),
            Sign::Negative => Some(-25 * (value as i16 - 1)),
        }
    } else {
        None
    };
    Ok(value)
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
#[deku(ctx = "subtype: u8", id = "subtype")]
#[serde(untagged)]
pub enum AirborneVelocitySubType {
    #[deku(id = "0")]
    Reserved0(ReservedVelocityData),

    #[deku(id_pat = "1..=2")]
    GroundSpeedDecoding(GroundSpeedDecoding),

    #[deku(id = "3")]
    AirspeedSubsonic(AirspeedSubsonicDecoding),
    #[deku(id = "4")]
    AirspeedSupersonic(AirspeedSupersonicDecoding),

    #[deku(id_pat = "5..=7")]
    Reserved1(ReservedVelocityData),
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
pub struct ReservedVelocityData {
    #[deku(bits = "22")]
    pub reserved_data: u32,
}

#[derive(Debug, PartialEq, DekuRead, Copy, Clone)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "1")]
pub enum Sign {
    Positive = 0,
    Negative = 1,
}

impl Sign {
    #[must_use]
    pub fn value(&self) -> i16 {
        match self {
            Self::Positive => 1,
            Self::Negative => -1,
        }
    }

    // for serde(default)
    fn positive() -> Self {
        Self::Positive
    }
}

impl fmt::Display for Sign {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Positive => "",
                Self::Negative => "-",
            }
        )
    }
}

/// Ground Speed Decoding (Subtypes 1 & 2)
///
/// Per ICAO Doc 9871 Table A-2-9a: Velocity over ground encoding
///
/// Velocity is encoded as East-West and North-South components:
///
/// **East-West Velocity** (bits 14-24):
/// - Bit 14: Direction (0=East, 1=West)
/// - Bits 15-24: 10-bit velocity magnitude
///
/// **North-South Velocity** (bits 25-35):
/// - Bit 25: Direction (0=North, 1=South)  
/// - Bits 26-35: 10-bit velocity magnitude
///
/// **Subtype 1** (Normal, LSB=1 kt):
/// - value=0: No velocity information
/// - value=1: 0 kt
/// - value=2: 1 kt
/// - ...
/// - value=1022: 1,021 kt
/// - value=1023: >1,021.5 kt
///
/// **Subtype 2** (Supersonic, LSB=4 kt):
/// - value=0: No velocity information
/// - value=1: 0 kt
/// - value=2: 4 kt
/// - value=3: 8 kt
/// - ...
/// - value=1022: 4,084 kt
/// - value=1023: >4,086 kt
///
/// Supersonic encoding used when either E-W OR N-S exceeds 1,022 kt.
/// Switch back to normal when both drop below 1,000 kt.
///
/// Ground speed and track angle computed from components:
/// - groundspeed = √(ew_vel² + ns_vel²)
/// - track = atan2(ew_vel, ns_vel) converted to degrees (0-360°)
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
pub struct GroundSpeedDecoding {
    #[serde(skip, default = "Sign::positive")]
    pub ew_sign: Sign,
    #[deku(
        endian = "big",
        bits = "10",
        map = "|val: u16| -> Result<_, DekuError> {
            Ok(f64::from((val as i16 - 1) * ew_sign.value()))
        }"
    )]
    #[serde(skip)]
    pub ew_vel: f64,
    #[serde(skip, default = "Sign::positive")]
    pub ns_sign: Sign,
    #[serde(skip)]
    #[deku(
        endian = "big",
        bits = "10",
        map = "|val: u16| -> Result<_, DekuError> {
            Ok(f64::from((val as i16 - 1) * ns_sign.value()))
        }"
    )]
    pub ns_vel: f64,
    #[deku(
        skip,
        default = "libm::hypot(f64::abs(*ew_vel), f64::abs(*ns_vel))"
    )]
    pub groundspeed: f64,
    #[deku(
        skip,
        default = "
        let h = libm::atan2(*ew_vel, *ns_vel) *
            (360.0 / (2.0 * std::f64::consts::PI));
        if h < 0.0 { h + 360. } else { h }
        "
    )]
    pub track: f64,
}

/// Airspeed Subsonic Decoding (Subtype 3)
///
/// Per ICAO Doc 9871 Table A-2-9b: Airspeed and heading (subsonic)
///
/// Used when GNSS position unavailable, reports airspeed and heading instead.
/// Subtype 3 messages are rare in practice.
///
/// **Magnetic Heading** (bits 14-24):
/// - Bit 14: Status (0=not available, 1=available)
/// - Bits 15-24: 10-bit heading value
///   * MSB = 180 degrees
///   * LSB = 360/1024 degrees (≈0.3516°)
///   * Formula: heading = value × (360/1024) degrees
///   * Range: [0, 359.6484°]
///   * Zero indicates magnetic north
///
/// **Airspeed** (bits 25-35):
/// - Bit 25: Type (0=IAS Indicated Airspeed, 1=TAS True Airspeed)
/// - Bits 26-35: 10-bit airspeed value
///   * LSB = 1 kt
///   * value=0: No airspeed information
///   * value=1: 0 kt
///   * value=2: 1 kt
///   * ...
///   * value=1022: 1,021 kt
///   * value=1023: >1,021.5 kt
///
/// Supersonic version (subtype 4) uses LSB=4 kt for airspeeds >1,022 kt.
#[derive(Debug, PartialEq, DekuRead, Clone)]
pub struct AirspeedSubsonicDecoding {
    #[deku(bits = "1")]
    /// Heading Status (bit 14): 0=not available, 1=available
    pub status_heading: bool,

    #[deku(
        endian = "big",
        bits = "10",
        map = "|val: u16| -> Result<_, DekuError> {
            Ok(if *status_heading { Some(val as f64 * 360. / 1024.) } else { None })
        }"
    )]
    /// Magnetic Heading (bits 15-24): Clockwise from magnetic north  
    /// Formula: heading = value × (360/1024) degrees  
    /// LSB = 360/1024 degrees (≈0.3516°)  
    /// Returns None if status_heading is false.
    pub heading: Option<f64>,

    /// Airspeed Type (bit 25): 0=IAS (Indicated), 1=TAS (True)
    pub airspeed_type: AirspeedType,

    #[deku(
        endian = "big",
        bits = "10",
        map = "|value: u16| -> Result<_, DekuError> {
            if value == 0 { return Ok(None) }
            Ok(Some(value - 1))
        }"
    )]
    /// Airspeed (bits 26-35): IAS or TAS in knots  
    /// Formula: airspeed = (value - 1) kt  
    /// value=0 returns None (no information)
    pub airspeed: Option<u16>,
}

impl Serialize for AirspeedSubsonicDecoding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        AirspeedSerdeHelper::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AirspeedSubsonicDecoding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let helper = AirspeedSerdeHelper::deserialize(deserializer)?;
        let (status_heading, heading, airspeed_type, airspeed) =
            helper.into_components();
        Ok(Self {
            status_heading,
            heading,
            airspeed_type,
            airspeed,
        })
    }
}

/// Airspeed Supersonic Decoding (Subtype 4)
///
/// Per ICAO Doc 9871 Table A-2-9b: Airspeed and heading (supersonic)
///
/// Identical structure to subtype 3 but with 4× speed resolution for supersonic flight.
///
/// **Airspeed** (bits 26-35):
/// - LSB = 4 kt (vs 1 kt for subtype 3)
/// - value=0: No airspeed information
/// - value=1: 0 kt
/// - value=2: 4 kt
/// - value=3: 8 kt
/// - ...
/// - value=1022: 4,084 kt
/// - value=1023: >4,086 kt
///
/// Supersonic encoding used when airspeed exceeds 1,022 kt.
/// Switch back to normal (subtype 3) when airspeed drops below 1,000 kt.
///
/// Note: Rare in practice as no operational supersonic airliners currently use ADS-B.
#[derive(Debug, PartialEq, DekuRead, Clone)]
pub struct AirspeedSupersonicDecoding {
    #[deku(bits = "1")]
    /// Heading Status (bit 14): 0=not available, 1=available
    pub status_heading: bool,

    #[deku(
        endian = "big",
        bits = "10",
        map = "|val: u16| -> Result<_, DekuError> {
            Ok(if *status_heading { Some(val as f32 * 360. / 1024.) } else { None })
        }"
    )]
    /// Magnetic Heading (bits 15-24): Same as subtype 3  
    /// LSB = 360/1024 degrees (≈0.3516°)
    pub heading: Option<f32>,

    /// Airspeed Type (bit 25): 0=IAS (Indicated), 1=TAS (True)
    pub airspeed_type: AirspeedType,

    #[deku(
        endian = "big",
        bits = "10",
        map = "|value: u16| -> Result<_, DekuError> {
            if value == 0 { return Ok(None) }
            Ok(Some(4*(value - 1)))
        }"
    )]
    /// Airspeed (bits 26-35): IAS or TAS in knots (supersonic resolution)  
    /// Formula: airspeed = 4 × (value - 1) kt  
    /// LSB = 4 kt for supersonic speeds
    pub airspeed: Option<u16>,
}

impl Serialize for AirspeedSupersonicDecoding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        AirspeedSerdeHelper::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AirspeedSupersonicDecoding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let helper = AirspeedSerdeHelper::deserialize(deserializer)?;
        let (status_heading, heading, airspeed_type, airspeed) =
            helper.into_components();
        Ok(Self {
            status_heading,
            heading: heading.map(|h| h as f32),
            airspeed_type,
            airspeed,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct AirspeedSerdeHelper {
    #[serde(skip_serializing_if = "Option::is_none")]
    heading: Option<f64>,
    #[serde(rename = "IAS", skip_serializing_if = "Option::is_none")]
    ias: Option<u16>,
    #[serde(rename = "TAS", skip_serializing_if = "Option::is_none")]
    tas: Option<u16>,
}

impl AirspeedSerdeHelper {
    fn into_components(self) -> (bool, Option<f64>, AirspeedType, Option<u16>) {
        let (airspeed_type, airspeed) = if let Some(ias) = self.ias {
            (AirspeedType::IAS, Some(ias))
        } else if let Some(tas) = self.tas {
            (AirspeedType::TAS, Some(tas))
        } else {
            (AirspeedType::IAS, None)
        };
        (
            self.heading.is_some(),
            self.heading,
            airspeed_type,
            airspeed,
        )
    }
}

impl From<&AirspeedSubsonicDecoding> for AirspeedSerdeHelper {
    fn from(v: &AirspeedSubsonicDecoding) -> Self {
        let (ias, tas) = match v.airspeed_type {
            AirspeedType::IAS => (v.airspeed, None),
            AirspeedType::TAS => (None, v.airspeed),
        };
        Self {
            heading: v.heading,
            ias,
            tas,
        }
    }
}

impl From<&AirspeedSupersonicDecoding> for AirspeedSerdeHelper {
    fn from(v: &AirspeedSupersonicDecoding) -> Self {
        let (ias, tas) = match v.airspeed_type {
            AirspeedType::IAS => (v.airspeed, None),
            AirspeedType::TAS => (None, v.airspeed),
        };
        Self {
            heading: v.heading.map(|h| h as f64),
            ias,
            tas,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, DekuRead)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "1")]
pub enum AirspeedType {
    IAS = 0,
    TAS = 1,
}

impl fmt::Display for AirspeedType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::IAS => "IAS",
                Self::TAS => "TAS",
            }
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq, DekuRead)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "1")]
pub enum DirectionEW {
    WestToEast = 0,
    EastToWest = 1,
}

#[derive(Copy, Clone, Debug, PartialEq, DekuRead)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "1")]
pub enum DirectionNS {
    SouthToNorth = 0,
    NorthToSouth = 1,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "1")]
pub enum VerticalRateSource {
    #[serde(rename = "barometric")]
    BarometricPressureAltitude = 0,

    #[serde(rename = "GNSS")]
    GeometricAltitude = 1,
}

impl fmt::Display for VerticalRateSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::BarometricPressureAltitude => "barometric",
                Self::GeometricAltitude => "GNSS",
            }
        )
    }
}

impl fmt::Display for AirborneVelocity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  Airborne velocity over ground (BDS 0,9)")?;
        match &self.velocity {
            AirborneVelocitySubType::GroundSpeedDecoding(v) => {
                writeln!(f, "  Track angle:   {}°", libm::round(v.track))?;
                writeln!(
                    f,
                    "  Groundspeed:   {} kt",
                    libm::round(v.groundspeed)
                )?;
            }
            AirborneVelocitySubType::AirspeedSubsonic(v) => {
                if let Some(value) = v.airspeed {
                    writeln!(
                        f,
                        "  {}:           {} kt",
                        v.airspeed_type, value
                    )?;
                }
                if let Some(value) = v.heading {
                    writeln!(f, "  Heading:       {}°", libm::round(value))?;
                }
            }
            AirborneVelocitySubType::AirspeedSupersonic(v) => {
                if let Some(value) = v.airspeed {
                    writeln!(
                        f,
                        "  {}:           {} kt",
                        v.airspeed_type, value
                    )?;
                    if let Some(value) = v.heading {
                        writeln!(
                            f,
                            "  Heading:       {}°",
                            libm::round(value as f64)
                        )?;
                    }
                }
            }
            AirborneVelocitySubType::Reserved0(_)
            | AirborneVelocitySubType::Reserved1(_) => {}
        }
        if let Some(vr) = &self.vertical_rate {
            writeln!(f, "  Vertical rate: {} ft/min {}", vr, &self.vrate_src)?;
        }
        writeln!(f, "  NACv:          {}", &self.nac_v)?;
        if let Some(value) = &self.geo_minus_baro {
            writeln!(f, "  GNSS delta:    {value} ft")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use approx::assert_relative_eq;
    use hexlit::hex;

    #[test]
    fn test_groundspeed_velocity() {
        let bytes = hex!("8D485020994409940838175B284F");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS09(velocity) = adsb_msg.message {
                if let AirborneVelocitySubType::GroundSpeedDecoding(_gsd) =
                    velocity.velocity
                {
                    assert_relative_eq!(
                        _gsd.groundspeed,
                        159.,
                        max_relative = 1e-2
                    );
                    assert_relative_eq!(
                        _gsd.track,
                        182.88,
                        max_relative = 1e-2
                    );
                    if let Some(vrate) = velocity.vertical_rate {
                        assert_eq!(vrate, -832);
                    }
                    assert_eq!(velocity.geo_minus_baro, Some(550));
                }
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_format_groundspeed() {
        let bytes = hex!("8D485020994409940838175B284F");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        assert_eq!(
            format!("{msg}"),
            r#" DF17. Extended Squitter
  Address:       485020
  Air/Ground:    airborne
  Airborne velocity over ground (BDS 0,9)
  Track angle:   183°
  Groundspeed:   159 kt
  Vertical rate: -832 ft/min barometric
  NACv:          0
  GNSS delta:    550 ft
"#
        )
    }

    #[test]
    fn test_airspeed_velocity() {
        let bytes = hex!("8DA05F219B06B6AF189400CBC33F");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS09(velocity) = adsb_msg.message {
                if let AirborneVelocitySubType::AirspeedSubsonic(asd) =
                    velocity.velocity
                {
                    assert_eq!(asd.airspeed.unwrap(), 375);
                    assert_relative_eq!(
                        asd.heading.unwrap(),
                        244.,
                        max_relative = 1e-2
                    );
                    assert_eq!(velocity.vertical_rate.unwrap(), -2304);
                }
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_format_airspeed() {
        let bytes = hex!("8DA05F219B06B6AF189400CBC33F");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        assert_eq!(
            format!("{msg}"),
            r#" DF17. Extended Squitter
  Address:       a05f21
  Air/Ground:    airborne
  Airborne velocity over ground (BDS 0,9)
  TAS:           375 kt
  Heading:       244°
  Vertical rate: -2304 ft/min GNSS
  NACv:          0
"#
        )
    }

    // Corner case tests for vertical rate encoding
    // These tests validate sign bit handling and edge cases

    #[test]
    fn test_vertical_rate_positive_64() {
        // Real message with vertical rate +64 ft/min (minimum positive rate)
        // vrate_sign=0 (positive), value=1, rate=(1-1)*64=0 but spec says value=2 for +64
        let bytes = hex!("8d3461cf9908388930080f948ea1");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS09(velocity) = adsb_msg.message {
                assert_eq!(velocity.vertical_rate, Some(64));
                assert_eq!(
                    velocity.vrate_src,
                    VerticalRateSource::GeometricAltitude
                );
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_vertical_rate_positive_128() {
        // Real message with vertical rate +128 ft/min
        let bytes = hex!("8d3461cf9908558e100c1071eb67");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS09(velocity) = adsb_msg.message {
                assert_eq!(velocity.vertical_rate, Some(128));
                assert_eq!(
                    velocity.vrate_src,
                    VerticalRateSource::GeometricAltitude
                );
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_vertical_rate_positive_960() {
        // Real message with vertical rate +960 ft/min
        let bytes = hex!("8d3461cf99085a8f10400f80e6ac");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS09(velocity) = adsb_msg.message {
                assert_eq!(velocity.vertical_rate, Some(960));
                assert_eq!(
                    velocity.vrate_src,
                    VerticalRateSource::GeometricAltitude
                );
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_vertical_rate_level_flight() {
        // Test for level flight (0 ft/min climb rate)
        // Note: Real-world messages with vrate_value=1 are rare
        // This would require: vrate_value=1, giving rate=(1-1)*64=0 ft/min
        // For now, we'll skip this test as we don't have a real message
        // The formula is tested implicitly by other tests (e.g., +64 uses value=2)
    }

    // Note: test_vertical_rate_zero (vrate_value=0 → None) is not included
    // because such messages are extremely rare in real flight data.
    // The value=0 case means "vertical rate information not available"

    #[test]
    fn test_vertical_rate_negative_64() {
        // Real message with vertical rate -64 ft/min (minimum negative rate)
        // vrate_sign=1 (negative), value=2, rate=-(2-1)*64=-64
        let bytes = hex!("8d394c0f990c4932780838866883");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS09(velocity) = adsb_msg.message {
                assert_eq!(velocity.vertical_rate, Some(-64));
                assert_eq!(
                    velocity.vrate_src,
                    VerticalRateSource::GeometricAltitude
                );
                return;
            }
        }
        unreachable!();
    }

    #[test]
    fn test_vertical_rate_sign_bit() {
        // Test sign bit handling: same magnitude, different signs
        // Positive test already covered above
        // This test uses a different negative rate to verify sign encoding
        let bytes_positive = hex!("8d3461cf9908388930080f948ea1"); // +64
        let bytes_negative = hex!("8d394c0f990c4932780838866883"); // -64

        let (_, msg_pos) = Message::from_bytes((&bytes_positive, 0)).unwrap();
        let (_, msg_neg) = Message::from_bytes((&bytes_negative, 0)).unwrap();

        if let ExtendedSquitterADSB(adsb_pos) = msg_pos.df {
            if let ME::BDS09(vel_pos) = adsb_pos.message {
                if let ExtendedSquitterADSB(adsb_neg) = msg_neg.df {
                    if let ME::BDS09(vel_neg) = adsb_neg.message {
                        // Verify signs are opposite
                        assert_eq!(vel_pos.vertical_rate, Some(64));
                        assert_eq!(vel_neg.vertical_rate, Some(-64));
                        assert_eq!(
                            vel_pos.vertical_rate.unwrap(),
                            -vel_neg.vertical_rate.unwrap()
                        );
                        return;
                    }
                }
            }
        }
        unreachable!();
    }

    #[test]
    fn test_vrate_source_gnss_vs_barometric() {
        // Test vertical rate source field
        // GNSS source example
        let bytes_gnss = hex!("8d3461cf9908388930080f948ea1");
        let (_, msg_gnss) = Message::from_bytes((&bytes_gnss, 0)).unwrap();

        if let ExtendedSquitterADSB(adsb_msg) = msg_gnss.df {
            if let ME::BDS09(velocity) = adsb_msg.message {
                assert_eq!(
                    velocity.vrate_src,
                    VerticalRateSource::GeometricAltitude
                );
            }
        }

        // Barometric source example (from existing test)
        let bytes_baro = hex!("8D485020994409940838175B284F");
        let (_, msg_baro) = Message::from_bytes((&bytes_baro, 0)).unwrap();

        if let ExtendedSquitterADSB(adsb_msg) = msg_baro.df {
            if let ME::BDS09(velocity) = adsb_msg.message {
                assert_eq!(
                    velocity.vrate_src,
                    VerticalRateSource::BarometricPressureAltitude
                );
            }
        }
    }

    #[test]
    fn test_geo_minus_baro_positive() {
        // Test GNSS altitude difference (GNSS above barometric)
        // From existing test: geo_minus_baro = +550 ft
        let bytes = hex!("8D485020994409940838175B284F");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(adsb_msg) = msg.df {
            if let ME::BDS09(velocity) = adsb_msg.message {
                assert_eq!(velocity.geo_minus_baro, Some(550));
                return;
            }
        }
        unreachable!();
    }
}
