//! BDS (Comm-B Data Selector) decoders for Mode S DF 20/21 replies.
//!
//! ## Inference strategy (`bds-infer` Cargo feature, default = on)
//!
//! Because passive receivers do not see the uplink interrogation, the BDS
//! code of a Comm-B reply must be guessed from the 56-bit MB payload alone.
//! Two strategies are available, switched at compile time:
//!
//! * **Default (`bds-infer` feature on)** — full inference strategy.
//!   In addition to the structural / status-bit checks of the minimal
//!   strategy, the following families of validation rules are active:
//!
//!   - **Hard physical-limit rejection** in the per-field readers:
//!     BDS 05 altitude ≤ 50 000 ft;
//!     BDS 40 selected MCP / FMS altitude ≤ 50 000 ft and barometric setting
//!     in [800, 1200] mb;
//!     BDS 50 groundspeed ≤ 700 kt, TAS ≤ 600 kt, |roll| ≤ 45°;
//!     BDS 60 IAS ≤ 500 kt, Mach ≤ 1, |vrate_inertial| ≤ 6 000 ft/min
//!     (the *barometric* altitude rate is intentionally not bounded
//!     because of known instrument inertia / barometric noise);
//!     BDS 44 temperature in [-80, +60] °C and wind speed ≤ 250 kt;
//!     BDS 45 static temperature in [-80, +60] °C;
//!     BDS 21 registration with at most 2 `#` placeholders.
//!   - **Density-based scoring** combined with **within-record cross-field
//!     penalties**: for the candidates in BDS 05 / 40 / 44 / 45 / 50 / 60,
//!     a mean log-density across decoded fields ([`density`]) is summed
//!     with the cross-field penalty ([`penalty`], which currently covers
//!     BDS 50 |TAS−GS| and roll/track-rate sign, and BDS 60 IAS/Mach
//!     ratio). The candidate is rejected when the combined score falls
//!     below [`density::DENSITY_THRESHOLD`] (default `−3.0`).
//!   - Strict capability-bit constraints from ICAO Doc 9871 on BDS 17 / 18.
//!   - The candidate set in [`infer_bds`] also includes BDS 06 / 08 / 09 /
//!     61 / 62.
//!
//! * **Minimal (`--no-default-features`)** — baseline strategy. Only
//!   reserved bits, status-bit consistency and spec-mandated value ranges
//!   are enforced; the relaxed capability bits on BDS 17 / 18 are
//!   accepted; BDS 06 / 08 / 09 / 61 / 62 are excluded from [`infer_bds`].
//!
//! Decoder bug-fixes (BDS 05 / 06 / 08 5-bit offset, BDS 10 prepend, BDS 21
//! dash-placeholder mapping, BDS 17 `bds20` accepting 0, BDS 44 temperature
//! sign-bit wrap-around fallback) are applied unconditionally in both
//! strategies.

pub mod bds05;
pub mod bds06;
pub mod bds08;
pub mod bds09;
pub mod bds10;
pub mod bds17;
pub mod bds18;
pub mod bds19;
pub mod bds20;
pub mod bds21;
pub mod bds30;
pub mod bds40;
pub mod bds44;
pub mod bds45;
pub mod bds50;
pub mod bds60;
pub mod bds61;
pub mod bds62;
pub mod bds65;
#[cfg(feature = "bds-infer")]
pub mod density;
#[cfg(feature = "bds-infer")]
pub mod penalty;

use self::bds05::AirbornePosition;
use self::bds06::SurfacePosition;
use self::bds08::AircraftIdentification as Bds08AircraftIdentification;
use self::bds09::AirborneVelocity;
use self::bds10::DataLinkCapability;
use self::bds17::CommonUsageGICBCapabilityReport;
use self::bds18::GICBCapabilityReportPart1;
use self::bds19::GICBCapabilityReportPart2;
use self::bds20::AircraftIdentification as Bds20AircraftIdentification;
use self::bds21::AircraftAndAirlineRegistrationMarkings;
use self::bds30::ACASResolutionAdvisory;
use self::bds40::SelectedVerticalIntention;
use self::bds44::MeteorologicalRoutineAirReport;
use self::bds45::MeteorologicalHazardReport;
use self::bds50::TrackAndTurnReport;
use self::bds60::HeadingAndSpeedReport;
use self::bds61::AircraftStatus;
use self::bds62::TargetStateAndStatusInformation;
use self::bds65::AircraftOperationStatus;
use deku::{DekuContainerRead, DekuError, DekuReader};
use serde::Serialize;
use std::fmt;

/// Error type for BDS decoding operations
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum DecodingError {
    /// Invalid BDS code (0xFF, reserved, or unsupported)
    InvalidBdsCode(u8),
    /// Type Code validation failed for this BDS code
    TypeCodeMismatch {
        /// The BDS code being decoded
        bds_code: u8,
        /// The Type Code extracted from payload
        received_tc: u8,
        /// Expected Type Code ranges for this BDS
        expected_tc: String,
    },
    /// Payload too short for this BDS code
    PayloadTooShort {
        /// Expected minimum bytes
        expected: usize,
        /// Actual bytes received
        received: usize,
    },
    /// Deku decoding error
    DecodingFailed(String),
}

impl fmt::Display for DecodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodingError::InvalidBdsCode(code) => {
                write!(f, "Invalid BDS code 0x{:02x}", code)
            }
            DecodingError::TypeCodeMismatch {
                bds_code,
                received_tc,
                expected_tc,
            } => {
                write!(
                    f,
                    "Type Code mismatch for BDS 0x{:02x}: received TC={}, expected {}",
                    bds_code, received_tc, expected_tc
                )
            }
            DecodingError::PayloadTooShort { expected, received } => {
                write!(
                    f,
                    "Payload too short: expected {} bytes, got {}",
                    expected, received
                )
            }
            DecodingError::DecodingFailed(msg) => {
                write!(f, "Decoding failed: {}", msg)
            }
        }
    }
}

impl std::error::Error for DecodingError {}

impl From<DekuError> for DecodingError {
    fn from(err: DekuError) -> Self {
        DecodingError::DecodingFailed(err.to_string())
    }
}

/// Decoded BDS payload content
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum DecodedBds {
    /// BDS 0,5: Airborne Position
    Bds05(AirbornePosition),
    /// BDS 0,6: Surface Position
    Bds06(SurfacePosition),
    /// BDS 0,8: Aircraft Identification and Category
    Bds08(Bds08AircraftIdentification),
    /// BDS 0,9: Airborne Velocity
    Bds09(AirborneVelocity),
    /// BDS 1,0: Data Link Capability
    Bds10(DataLinkCapability),
    /// BDS 1,7: Common Usage GICB Capability Report
    Bds17(CommonUsageGICBCapabilityReport),
    /// BDS 1,8: GICB Capability Report Part 1
    Bds18(GICBCapabilityReportPart1),
    /// BDS 1,9: GICB Capability Report Part 2
    Bds19(GICBCapabilityReportPart2),
    /// BDS 2,0: Aircraft Identification
    Bds20(Bds20AircraftIdentification),
    /// BDS 2,1: Aircraft and Airline Registration Markings
    Bds21(AircraftAndAirlineRegistrationMarkings),
    /// BDS 3,0: ACAS Resolution Advisory
    Bds30(ACASResolutionAdvisory),
    /// BDS 4,0: Selected Vertical Intention
    Bds40(SelectedVerticalIntention),
    /// BDS 4,4: Meteorological Routine Air Report
    Bds44(MeteorologicalRoutineAirReport),
    /// BDS 4,5: Meteorological Hazard Report
    Bds45(MeteorologicalHazardReport),
    /// BDS 5,0: Track and Turn Report
    Bds50(TrackAndTurnReport),
    /// BDS 6,0: Heading and Speed Report
    Bds60(HeadingAndSpeedReport),
    /// BDS 6,1: Aircraft Status (Emergency/Priority)
    Bds61(AircraftStatus),
    /// BDS 6,2: Target State and Status Information
    Bds62(TargetStateAndStatusInformation),
    /// BDS 6,5: Aircraft Operational Status
    Bds65(AircraftOperationStatus),
}

/// Extract the Type Code (TC) from the first 5 bits of payload
fn extract_tc(payload: &[u8]) -> u8 {
    if payload.is_empty() {
        return 0;
    }
    payload[0] >> 3
}

/// Apply the combined density + cross-field score gate to a successfully
/// decoded candidate.
///
/// With `bds-infer` enabled (default), the candidate's combined score is
///
/// ```text
///     score = mean(field_log_densities)   // density.rs
///           + cross_field_penalty         // penalty.rs
/// ```
///
/// and the candidate is rejected with [`DecodingError::DecodingFailed`]
/// when the combined score falls below [`density::DENSITY_THRESHOLD`]
/// (default `−3.0`). Candidates with no density-scored field and a zero
/// cross-field penalty are accepted unconditionally (no signal). Without
/// `bds-infer`, this function is the identity.
#[inline]
fn apply_score_gate(
    result: Result<DecodedBds, DecodingError>,
) -> Result<DecodedBds, DecodingError> {
    #[cfg(feature = "bds-infer")]
    {
        match result {
            Ok(decoded) => {
                let mean = density::density_score(&decoded);
                let pen = penalty::penalty(&decoded);
                if mean.is_none() && pen == 0.0 {
                    return Ok(decoded);
                }
                let score = mean.unwrap_or(0.0) + pen;
                if score < density::DENSITY_THRESHOLD {
                    Err(DecodingError::DecodingFailed(format!(
                        "combined score {score:.2} below threshold {:.1}",
                        density::DENSITY_THRESHOLD
                    )))
                } else {
                    Ok(decoded)
                }
            }
            Err(e) => Err(e),
        }
    }
    #[cfg(not(feature = "bds-infer"))]
    {
        result
    }
}

/// Attempt to decode the BDS payload based on the BDS code
/// Returns None if decoding fails or BDS type is not supported
pub fn decode_payload(
    payload: &[u8],
    bds_code: u8,
) -> Result<DecodedBds, DecodingError> {
    let result = match bds_code {
        0x05 => {
            let tc = extract_tc(payload);
            if !((9..=18).contains(&tc) || (20..=22).contains(&tc)) {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "9-18, 20-22".to_string(),
                });
            }
            let mut reader =
                deku::reader::Reader::new(std::io::Cursor::new(payload));
            reader.skip_bits(5, deku::ctx::Order::Msb0)?;
            AirbornePosition::from_reader_with_ctx(&mut reader, tc)
                .map(DecodedBds::Bds05)
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x06 => {
            let tc = extract_tc(payload);
            if !(5..=8).contains(&tc) {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "5-8".to_string(),
                });
            }
            let mut reader =
                deku::reader::Reader::new(std::io::Cursor::new(payload));
            reader.skip_bits(5, deku::ctx::Order::Msb0)?;
            SurfacePosition::from_reader_with_ctx(&mut reader, tc)
                .map(DecodedBds::Bds06)
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x08 => {
            let tc = extract_tc(payload);
            if !(1..=4).contains(&tc) {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "1-4".to_string(),
                });
            }
            let mut reader =
                deku::reader::Reader::new(std::io::Cursor::new(payload));
            reader.skip_bits(5, deku::ctx::Order::Msb0)?;
            Bds08AircraftIdentification::from_reader_with_ctx(&mut reader, tc)
                .map(DecodedBds::Bds08)
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x09 => {
            let tc = extract_tc(payload);
            if tc != 19 {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "19".to_string(),
                });
            }
            AirborneVelocity::from_bytes((payload, 5))
                .map(|(_, decoded)| DecodedBds::Bds09(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x10 => DataLinkCapability::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds10(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x17 => CommonUsageGICBCapabilityReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds17(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x18 => GICBCapabilityReportPart1::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds18(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x19 => GICBCapabilityReportPart2::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds19(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x20 => Bds20AircraftIdentification::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds20(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x21 => {
            AircraftAndAirlineRegistrationMarkings::from_bytes((payload, 0))
                .map(|(_, decoded)| DecodedBds::Bds21(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x30 => ACASResolutionAdvisory::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds30(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x40 => SelectedVerticalIntention::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds40(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x44 => MeteorologicalRoutineAirReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds44(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x45 => MeteorologicalHazardReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds45(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x50 => TrackAndTurnReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds50(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x60 => HeadingAndSpeedReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds60(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x61 => {
            let tc = extract_tc(payload);
            if tc != 28 {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "28".to_string(),
                });
            }
            AircraftStatus::from_bytes((payload, 5))
                .map(|(_, decoded)| DecodedBds::Bds61(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x62 => {
            let tc = extract_tc(payload);
            if tc != 29 {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "29".to_string(),
                });
            }
            TargetStateAndStatusInformation::from_bytes((payload, 5))
                .map(|(_, decoded)| DecodedBds::Bds62(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x65 => {
            let tc = extract_tc(payload);
            if tc != 31 {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "31".to_string(),
                });
            }
            AircraftOperationStatus::from_bytes((payload, 5))
                .map(|(_, decoded)| DecodedBds::Bds65(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        _ => Err(DecodingError::InvalidBdsCode(bds_code)),
    };
    apply_score_gate(result)
}

/// Decode a BDS payload with a required BDS code (strict decoding)
///
/// This function requires the caller to specify which BDS code to decode.
/// It validates that:
/// 1. The BDS code is supported
/// 2. The payload's Type Code (TC) matches the expected range for that BDS
/// 3. The decoding succeeds with proper error reporting
///
/// # Arguments
/// * `payload` - The BDS data field (typically 7 bytes from Comm-B message)
/// * `bds_code` - The specific BDS code to decode (0x05, 0x06, etc.)
///
/// # Returns
/// * `Ok(DecodedBds)` - Successfully decoded payload
/// * `Err(DecodingError)` - Decoding failed with specific error reason
///
/// # Example
/// ```ignore
/// let payload = vec![0x80, 0x02, 0x04, 0x08, 0x0E, 0x20, 0x47];
/// let result = decode_bds(&payload, 0x60); // BDS 6,0 (Heading and Speed Report)
/// ```
pub fn decode_bds(
    payload: &[u8],
    bds_code: u8,
) -> Result<DecodedBds, DecodingError> {
    // Validate payload is not empty
    if payload.is_empty() {
        return Err(DecodingError::PayloadTooShort {
            expected: 1,
            received: 0,
        });
    }

    let result = match bds_code {
        0x05 => {
            let tc = extract_tc(payload);
            if !((9..=18).contains(&tc) || (20..=22).contains(&tc)) {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "9-18, 20-22".to_string(),
                });
            }
            let mut reader =
                deku::reader::Reader::new(std::io::Cursor::new(payload));
            reader.skip_bits(5, deku::ctx::Order::Msb0)?;
            AirbornePosition::from_reader_with_ctx(&mut reader, tc)
                .map(DecodedBds::Bds05)
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x06 => {
            let tc = extract_tc(payload);
            if !(5..=8).contains(&tc) {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "5-8".to_string(),
                });
            }
            let mut reader =
                deku::reader::Reader::new(std::io::Cursor::new(payload));
            reader.skip_bits(5, deku::ctx::Order::Msb0)?;
            SurfacePosition::from_reader_with_ctx(&mut reader, tc)
                .map(DecodedBds::Bds06)
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x08 => {
            let tc = extract_tc(payload);
            if !(1..=4).contains(&tc) {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "1-4".to_string(),
                });
            }
            let mut reader =
                deku::reader::Reader::new(std::io::Cursor::new(payload));
            reader.skip_bits(5, deku::ctx::Order::Msb0)?;
            Bds08AircraftIdentification::from_reader_with_ctx(&mut reader, tc)
                .map(DecodedBds::Bds08)
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x09 => {
            let tc = extract_tc(payload);
            if tc != 19 {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "19".to_string(),
                });
            }
            AirborneVelocity::from_bytes((payload, 5))
                .map(|(_, decoded)| DecodedBds::Bds09(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x10 => DataLinkCapability::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds10(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x17 => CommonUsageGICBCapabilityReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds17(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x18 => GICBCapabilityReportPart1::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds18(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x19 => GICBCapabilityReportPart2::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds19(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x20 => Bds20AircraftIdentification::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds20(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x21 => {
            AircraftAndAirlineRegistrationMarkings::from_bytes((payload, 0))
                .map(|(_, decoded)| DecodedBds::Bds21(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x30 => ACASResolutionAdvisory::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds30(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x40 => SelectedVerticalIntention::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds40(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x44 => MeteorologicalRoutineAirReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds44(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x45 => MeteorologicalHazardReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds45(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x50 => TrackAndTurnReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds50(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x60 => HeadingAndSpeedReport::from_bytes((payload, 0))
            .map(|(_, decoded)| DecodedBds::Bds60(decoded))
            .map_err(|e| DecodingError::DecodingFailed(e.to_string())),
        0x61 => {
            let tc = extract_tc(payload);
            if tc != 28 {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "28".to_string(),
                });
            }
            AircraftStatus::from_bytes((payload, 5))
                .map(|(_, decoded)| DecodedBds::Bds61(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x62 => {
            let tc = extract_tc(payload);
            if tc != 29 {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "29".to_string(),
                });
            }
            TargetStateAndStatusInformation::from_bytes((payload, 5))
                .map(|(_, decoded)| DecodedBds::Bds62(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        0x65 => {
            let tc = extract_tc(payload);
            if tc != 31 {
                return Err(DecodingError::TypeCodeMismatch {
                    bds_code,
                    received_tc: tc,
                    expected_tc: "31".to_string(),
                });
            }
            // Restrict BDS 65 inference to the Airborne Status Message
            // (subtype 0). Surface (subtype 1) and Reserved (subtypes 2-7)
            // are rejected here because, in the absence of an interrogation
            // context, accepting them inflates the false-positive rate when
            // BDS 65 is tried as a hypothesis on arbitrary Comm-B payloads.
            // Subtype is the 3-bit field immediately following the 5-bit TC,
            // i.e. the lower 3 bits of payload[0].
            let subtype = payload[0] & 0b111;
            if subtype != 0 {
                return Err(DecodingError::DecodingFailed(format!(
                    "BDS 65 subtype {subtype} rejected (only Airborne / subtype 0 accepted)"
                )));
            }
            AircraftOperationStatus::from_bytes((payload, 5))
                .map(|(_, decoded)| DecodedBds::Bds65(decoded))
                .map_err(|e| DecodingError::DecodingFailed(e.to_string()))
        }
        _ => Err(DecodingError::InvalidBdsCode(bds_code)),
    };
    apply_score_gate(result)
}

/// Infer possible BDS codes from a Comm-B payload (inference mode)
///
/// This function attempts to decode the payload as multiple BDS codes and returns
/// only those that succeed. This is used in Comm-B contexts where the BDS code
/// is not directly available, following the DF20/DF21 inference pattern.
///
/// # Arguments
/// * `payload` - The Comm-B data field (typically 7 bytes)
/// * `icao24` - Optional ICAO 24-bit aircraft address, used to validate BDS 2,1
///   registration strings against the country-prefix pattern inferred from the
///   address range. Pass `None` when the address is not available.
///
/// # Returns
/// * `Vec<DecodedBds>` - All successfully decoded BDS variants, ordered by BDS code
///   Returns empty vec if no BDS codes can decode the payload.
///
/// # Example
/// ```ignore
/// let payload = vec![0x80, 0x02, 0x04, 0x08, 0x0E, 0x20, 0x47];
/// let results = infer_bds(&payload, None);
/// // Returns all BDS codes that successfully decode this payload
/// ```
pub fn infer_bds(payload: &[u8], icao24: Option<u32>) -> Vec<DecodedBds> {
    if payload.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();

    // Try all supported BDS codes in order
    let candidates: &[u8] = &[
        0x05, 0x10, 0x17, 0x18, 0x19, 0x20, 0x21, 0x30, 0x40, 0x44, 0x45, 0x50,
        0x60, 0x65,
    ];
    for bds_code in candidates {
        if let Ok(decoded) = decode_bds(payload, *bds_code) {
            // P1: icao24-aware registration validation for BDS 2,1.
            // Rejects phantom BDS 2,1 candidates whose decoded registration
            // does not match any of the four structural buckets for the
            // country inferred from the aircraft's ICAO address range.
            #[cfg(feature = "bds-infer")]
            if let (DecodedBds::Bds21(ref bds21), Some(addr)) =
                (&decoded, icao24)
            {
                if let Some(reg) = &bds21.aircraft_registration {
                    if !bds21::validate_registration(reg, addr) {
                        continue;
                    }
                }
            }
            results.push(decoded);
        }
    }

    // P2: winner-take-all.
    // BDS 1,0 / 2,0 / 3,0 are identified by a mandatory byte-header prefix
    // (0x10 / 0x20 / 0x30) that no other register produces, making them
    // high-confidence identifications. BDS 2,1 after passing P1 joins this
    // tier. When any such candidate is present, it evicts every other
    // surviving candidate — phantom co-survivors on the same payload cannot
    // be genuine.
    #[cfg(feature = "bds-infer")]
    {
        let has_winner = results.iter().any(|d| {
            matches!(
                d,
                DecodedBds::Bds10(_)
                    | DecodedBds::Bds20(_)
                    | DecodedBds::Bds21(_)
                    | DecodedBds::Bds30(_)
            )
        });
        if has_winner {
            results.retain(|d| {
                matches!(
                    d,
                    DecodedBds::Bds10(_)
                        | DecodedBds::Bds20(_)
                        | DecodedBds::Bds21(_)
                        | DecodedBds::Bds30(_)
                )
            });
        }
    }

    results
}
