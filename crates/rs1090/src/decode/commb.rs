use super::bds::bds05::AirbornePosition;
use super::bds::bds10::DataLinkCapability;
use super::bds::bds17::CommonUsageGICBCapabilityReport;
use super::bds::bds18::GICBCapabilityReportPart1;
use super::bds::bds19::GICBCapabilityReportPart2;
use super::bds::bds20::AircraftIdentification;
use super::bds::bds21::AircraftAndAirlineRegistrationMarkings;
use super::bds::bds30::ACASResolutionAdvisory;
use super::bds::bds40::SelectedVerticalIntention;
use super::bds::bds44::MeteorologicalRoutineAirReport;
use super::bds::bds45::MeteorologicalHazardReport;
use super::bds::bds50::TrackAndTurnReport;
use super::bds::bds60::HeadingAndSpeedReport;
use super::bds::bds65::AircraftOperationStatus;
use super::cpr::AircraftState;
use super::AC13Field;
use super::ICAO;
use deku::{ctx::Order, prelude::*};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use tracing::debug;

/**
 * ## Comm-B Data Selector (BDS)
 *
 * The first four BDS codes (1,0, 1,7, 2,0, 3,0) belong to the ELS service,
 * the next three ones (4,0, 5,0, 6,0) belong to the EHS services,
 * and the last two codes (4,4, 4,5) report meteorological information.
 */

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Default)]
pub struct DF20DataSelector {
    #[serde(skip)]
    /// Set to true if all zeros, then there is no need to parse
    pub is_empty: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds05: Option<AirbornePosition>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds10: Option<DataLinkCapability>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds17: Option<CommonUsageGICBCapabilityReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds18: Option<GICBCapabilityReportPart1>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds19: Option<GICBCapabilityReportPart2>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds20: Option<AircraftIdentification>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds21: Option<AircraftAndAirlineRegistrationMarkings>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds30: Option<ACASResolutionAdvisory>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds40: Option<SelectedVerticalIntention>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds44: Option<MeteorologicalRoutineAirReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds45: Option<MeteorologicalHazardReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds50: Option<TrackAndTurnReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds60: Option<HeadingAndSpeedReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds65: Option<AircraftOperationStatus>,
}

impl DF20DataSelector {
    /// Sanitize Comm-B data using optional aircraft state context
    ///
    /// Current implementation invalidates BDS 5,0 and BDS 6,0 if both are present,
    /// as this typically indicates unreliable data.
    ///
    /// Future implementations will use the context for more sophisticated validation:
    /// - Check if groundspeed/track changes are physically possible
    /// - Validate against reference altitude from DF20 header
    /// - Temporal consistency checks
    pub fn sanitize(&mut self, _context: Option<&CommBContext>) {
        // Basic validation: if both BDS50 and BDS60 are present, invalidate both
        // This is a common pattern indicating unreliable Comm-B data
        if self.bds50.is_some() && self.bds60.is_some() {
            self.bds50 = None;
            self.bds60 = None;
        }

        // Future: context-based validation
        // if let Some(ctx) = context {
        //     // Validate BDS 5,0 groundspeed against last known value
        //     if let Some(bds50) = &self.bds50 {
        //         if let (Some(gs), Some(last_gs)) = (bds50.groundspeed, ctx.last_groundspeed) {
        //             // Check if change is physically possible (e.g., < 100 kt/s)
        //             if (gs as f64 - last_gs).abs() > 100.0 {
        //                 self.bds50 = None;
        //             }
        //         }
        //     }
        //
        //     // Validate against reference altitude from DF20 AC13 field
        //     if let (Some(ref_alt), Some(bds40)) = (ctx.reference_altitude, &self.bds40) {
        //         if let Some(sel_alt) = bds40.selected_altitude_mcp {
        //             // Selected altitude should be reasonably close to current altitude
        //             if (sel_alt as i32 - ref_alt).abs() > 10000 {
        //                 self.bds40 = None;
        //             }
        //         }
        //     }
        // }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Default)]
pub struct DF21DataSelector {
    #[serde(skip)]
    /// Set to true if all zeros, then there is no need to parse
    pub is_empty: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds05: Option<AirbornePosition>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds10: Option<DataLinkCapability>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds17: Option<CommonUsageGICBCapabilityReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds18: Option<GICBCapabilityReportPart1>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds19: Option<GICBCapabilityReportPart2>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds20: Option<AircraftIdentification>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds21: Option<AircraftAndAirlineRegistrationMarkings>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds30: Option<ACASResolutionAdvisory>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds40: Option<SelectedVerticalIntention>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds44: Option<MeteorologicalRoutineAirReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds45: Option<MeteorologicalHazardReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds50: Option<TrackAndTurnReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds60: Option<HeadingAndSpeedReport>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bds65: Option<AircraftOperationStatus>,
}

impl DF21DataSelector {
    /// Sanitize Comm-B data using optional aircraft state context
    ///
    /// See [`DF20DataSelector::sanitize`] for details.
    pub fn sanitize(&mut self, _context: Option<&CommBContext>) {
        if self.bds50.is_some() && self.bds60.is_some() {
            self.bds50 = None;
            self.bds60 = None;
        }
    }
}

/// Context for Comm-B message validation and sanitization
///
/// This context is used to validate Comm-B data against known aircraft state.
/// Currently used for basic validation (e.g., invalidating BDS 5,0 and 6,0 if both present).
/// Future implementations will use this for more sophisticated validation based on
/// temporal consistency, physical constraints, and cross-field validation.
#[derive(Debug, Default, Clone)]
pub struct CommBContext {
    /// Reference altitude from DF20 AC13 field (for cross-validation)
    pub reference_altitude: Option<i32>,
    // Future: Add more fields when we have a proper aircraft tracking structure
    // pub last_altitude: Option<i32>,
    // pub last_groundspeed: Option<f64>,
    // pub last_track: Option<f64>,
    // pub last_timestamp: Option<f64>,
}

impl fmt::Display for DF21DataSelector {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl fmt::Display for DF20DataSelector {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl DekuReader<'_, AC13Field> for DF20DataSelector {
    fn from_reader_with_ctx<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
        reader: &mut Reader<R>,
        ac: AC13Field, // altitude helps a lot in the validation
    ) -> Result<Self, DekuError>
    where
        Self: Sized,
    {
        let mut result = Self::default();
        let res = reader.read_bits(56, Order::Msb0)?;
        let bits = res.unwrap();
        let buf = bits.into_vec();
        debug!(
            "Decoding {:?} according to various hypotheses",
            buf.as_slice()
        );

        if buf.iter().all(|&x| x == 0) {
            result.is_empty = true;
            return Ok(result);
        }

        // Read the first 5 bits as a u8 and get the typecode
        let tc = &buf[0] >> 3;
        if (9..22).contains(&tc) && tc != 19 {
            let mut input = std::io::Cursor::new(&buf);
            let mut reader = Reader::new(&mut input);
            match AirbornePosition::from_reader_with_ctx(&mut reader, tc) {
                Ok(bds05) if bds05.alt == ac.0 => result.bds05 = Some(bds05),
                Ok(_) => (),
                Err(e) => debug!("Hypothesis BDS05: {}", e.to_string()),
            }
        } else {
            debug!(
                "Hypothesis BDS05: Typecode inconsistency {} should be in [9, 18] or [20, 22]",
                tc
            )
        }
        match DataLinkCapability::try_from(buf.as_slice()) {
            Ok(bds10) => result.bds10 = Some(bds10),
            Err(e) => debug!("Hypothesis BDS10: {}", e.to_string()),
        }
        match CommonUsageGICBCapabilityReport::try_from(buf.as_slice()) {
            Ok(bds17) => result.bds17 = Some(bds17),
            Err(e) => debug!("Hypothesis BDS17: {}", e.to_string()),
        }
        match GICBCapabilityReportPart1::try_from(buf.as_slice()) {
            Ok(bds18) => result.bds18 = Some(bds18),
            Err(e) => debug!("Hypothesis BDS18: {}", e.to_string()),
        }
        match GICBCapabilityReportPart2::try_from(buf.as_slice()) {
            Ok(bds19) => result.bds19 = Some(bds19),
            Err(e) => debug!("Hypothesis BDS19: {}", e.to_string()),
        }
        match AircraftIdentification::try_from(buf.as_slice()) {
            Ok(bds20) => result.bds20 = Some(bds20),
            Err(e) => debug!("Hypothesis BDS20: {}", e.to_string()),
        }
        match AircraftAndAirlineRegistrationMarkings::try_from(buf.as_slice()) {
            Ok(bds21) => result.bds21 = Some(bds21),
            Err(e) => debug!("Hypothesis BDS21: {}", e.to_string()),
        }
        match ACASResolutionAdvisory::try_from(buf.as_slice()) {
            Ok(bds30) => result.bds30 = Some(bds30),
            Err(e) => debug!("Hypothesis BDS30: {}", e.to_string()),
        }
        match SelectedVerticalIntention::try_from(buf.as_slice()) {
            Ok(bds40) => result.bds40 = Some(bds40),
            Err(e) => debug!("Hypothesis BDS40: {}", e.to_string()),
        }
        match MeteorologicalRoutineAirReport::try_from(buf.as_slice()) {
            Ok(bds44) => result.bds44 = Some(bds44),
            Err(e) => debug!("Hypothesis BDS44: {}", e.to_string()),
        }
        match MeteorologicalHazardReport::try_from(buf.as_slice()) {
            Ok(bds45) => result.bds45 = Some(bds45),
            Err(e) => debug!("Hypothesis BDS45: {}", e.to_string()),
        }
        match TrackAndTurnReport::try_from(buf.as_slice()) {
            Ok(bds50) => result.bds50 = Some(bds50),
            Err(e) => debug!("Hypothesis BDS50: {}", e.to_string()),
        }
        match HeadingAndSpeedReport::try_from(buf.as_slice()) {
            Ok(bds60) => result.bds60 = Some(bds60),
            Err(e) => debug!("Hypothesis BDS60: {}", e.to_string()),
        }

        let enum_id = &buf[0] & 0b111;
        match (tc, enum_id) {
            (31, id) if id < 2 => {
                match  AircraftOperationStatus::try_from(buf.as_slice()) {
                    Ok(bds65) => {
                        result.bds65 = Some(bds65)
                    }
                    Err(e) => debug!("Hypothesis BDS65: {}", e.to_string())
                }
            }
            _ => debug!(
                "Hypothesis BDS 6,5: invalid typecode {} (31) or category {} (0 or 1)",
                tc, enum_id
            )
        }

        Ok(result)
    }
}

impl DekuReader<'_> for DF21DataSelector {
    fn from_reader_with_ctx<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
        reader: &mut Reader<R>,
        _: (),
    ) -> Result<Self, DekuError>
    where
        Self: Sized,
    {
        let mut result = Self::default();
        let res = reader.read_bits(56, Order::Msb0)?;
        let buf = res.unwrap().into_vec();
        debug!(
            "Decoding {:?} according to various hypotheses",
            buf.as_slice()
        );

        if buf.iter().all(|&x| x == 0) {
            result.is_empty = true;
            return Ok(result);
        }

        let tc = &buf[0] >> 3;

        // On purpose: do not try bds05 here.
        // The reason for that is that there is no way to validate the altitude
        // Read the first 5 bits as a u8 and get the typecode
        /*if (9..22).contains(&tc) && tc != 19 {
            match AirbornePosition::try_from(buf.as_slice()) {
                Ok(bds05) => result.bds05 = Some(bds05),
                Err(e) => debug!("Hypothesis BDS05: {}", e.to_string()),
            }
        } else {
            debug!(
                "Hypothesis BDS05: Typecode {} should be in [9, 18] or [20, 22]",
                tc
            )
        }*/

        match DataLinkCapability::try_from(buf.as_slice()) {
            Ok(bds10) => result.bds10 = Some(bds10),
            Err(e) => debug!("Hypothesis BDS10: {}", e.to_string()),
        }
        match CommonUsageGICBCapabilityReport::try_from(buf.as_slice()) {
            Ok(bds17) => result.bds17 = Some(bds17),
            Err(e) => debug!("Hypothesis BDS17: {}", e.to_string()),
        }
        match GICBCapabilityReportPart1::try_from(buf.as_slice()) {
            Ok(bds18) => result.bds18 = Some(bds18),
            Err(e) => debug!("Hypothesis BDS18: {}", e.to_string()),
        }
        match GICBCapabilityReportPart2::try_from(buf.as_slice()) {
            Ok(bds19) => result.bds19 = Some(bds19),
            Err(e) => debug!("Hypothesis BDS19: {}", e.to_string()),
        }
        match AircraftIdentification::try_from(buf.as_slice()) {
            Ok(bds20) => result.bds20 = Some(bds20),
            Err(e) => debug!("Hypothesis BDS20: {}", e.to_string()),
        }
        match AircraftAndAirlineRegistrationMarkings::try_from(buf.as_slice()) {
            Ok(bds21) => result.bds21 = Some(bds21),
            Err(e) => debug!("Hypothesis BDS21: {}", e.to_string()),
        }
        match ACASResolutionAdvisory::try_from(buf.as_slice()) {
            Ok(bds30) => result.bds30 = Some(bds30),
            Err(e) => debug!("Hypothesis BDS30: {}", e.to_string()),
        }
        match SelectedVerticalIntention::try_from(buf.as_slice()) {
            Ok(bds40) => result.bds40 = Some(bds40),
            Err(e) => debug!("Hypothesis BDS40: {}", e.to_string()),
        }
        match MeteorologicalRoutineAirReport::try_from(buf.as_slice()) {
            Ok(bds44) => result.bds44 = Some(bds44),
            Err(e) => debug!("Hypothesis BDS44: {}", e.to_string()),
        }
        match MeteorologicalHazardReport::try_from(buf.as_slice()) {
            Ok(bds45) => result.bds45 = Some(bds45),
            Err(e) => debug!("Hypothesis BDS45: {}", e.to_string()),
        }
        match TrackAndTurnReport::try_from(buf.as_slice()) {
            Ok(bds50) => result.bds50 = Some(bds50),
            Err(e) => debug!("Hypothesis BDS50: {}", e.to_string()),
        }
        match HeadingAndSpeedReport::try_from(buf.as_slice()) {
            Ok(bds60) => result.bds60 = Some(bds60),
            Err(e) => debug!("Hypothesis BDS60: {}", e.to_string()),
        }

        let enum_id = &buf[0] & 0b111;
        match (tc, enum_id) {
            (31, id) if id < 2 => {
                match  AircraftOperationStatus::try_from(buf.as_slice()) {
                    Ok(bds65) => {
                        result.bds65 = Some(bds65)
                    }
                    Err(e) => debug!("Hypothesis BDS65: {}", e.to_string())
                }
            }
            _ => debug!(
                "Hypothesis BDS 6,5: invalid typecode {} (31) or category {} (0 or 1)",
                tc, enum_id
            )
        }

        Ok(result)
    }
}

/// Message processor for Comm-B sanitization
///
/// This provides a simple builder-pattern API for sanitizing Comm-B messages
/// based on aircraft state context. It can be chained with other message
/// processing operations.
///
/// # Example
///
/// ```no_run
/// use rs1090::decode::commb::MessageProcessor;
/// use rs1090::prelude::*;
/// use std::collections::BTreeMap;
///
/// # let mut message = todo!();
/// # let aircraft = todo!();
/// MessageProcessor::new(&mut message, &aircraft)
///     .sanitize_commb()
///     .finish();
/// ```
pub struct MessageProcessor<'a> {
    message: &'a mut super::Message,
    #[allow(dead_code)] // Reserved for future context-based validation
    aircraft: &'a BTreeMap<ICAO, AircraftState>,
}

impl<'a> MessageProcessor<'a> {
    /// Create a new message processor
    pub fn new(
        message: &'a mut super::Message,
        aircraft: &'a BTreeMap<ICAO, AircraftState>,
    ) -> Self {
        Self { message, aircraft }
    }

    /// Sanitize Comm-B data using aircraft state context
    ///
    /// This will invalidate BDS 5,0 and BDS 6,0 if both are present.
    /// Future implementations will perform more sophisticated validation
    /// using the aircraft state context.
    pub fn sanitize_commb(self) -> Self {
        use super::DF::*;

        match &mut self.message.df {
            CommBAltitudeReply { bds, ac, .. } => {
                let context = CommBContext {
                    reference_altitude: ac.0,
                };
                bds.sanitize(Some(&context));
            }
            CommBIdentityReply { bds, .. } => {
                let context = CommBContext {
                    reference_altitude: None,
                };
                bds.sanitize(Some(&context));
            }
            _ => {}
        }
        self
    }

    /// Finish processing and consume the processor
    pub fn finish(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_bds5060_no65() {
        let bytes = hex!("A8001EBCFFFB23286004A73F6A5B");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        match msg.df {
            CommBIdentityReply { bds, .. } => {
                assert!(bds.bds50.is_some());
                assert!(bds.bds60.is_some());
                assert!(bds.bds65.is_none());
            }
            _ => unreachable!(),
        }
    }
}
