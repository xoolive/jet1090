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
use super::bds::{validate_score_gate, DecodedBds};
use super::cpr::AircraftState;
use super::AC13Field;
use super::ICAO;
use deku::{ctx::Order, prelude::*};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use tracing::debug;

macro_rules! keep_scored_candidate {
    ($result:ident.$field:ident = $value:ident, $variant:ident, $label:literal) => {
        match validate_score_gate(&DecodedBds::$variant($value.clone())) {
            Ok(()) => $result.$field = Some($value),
            Err(e) => debug!("Hypothesis {}: {}", $label, e.to_string()),
        }
    };
}

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
    /// Sanitize decoded Comm-B candidates using optional aircraft state context.
    ///
    /// Applied after all BDS hypotheses have been decoded from the same 56-bit
    /// payload. The following checks run in order under `bds-infer`:
    ///
    /// 1. Reject a BDS 0,5 candidate whose decoded altitude differs by more
    ///    than 100 ft from [`CommBContext::last_altitude`]. For a genuine
    ///    BDS 0,5, both values come from the same on-board barometric sensor
    ///    and must agree within Mode-C quantisation (25 ft). For a phantom
    ///    they are uncorrelated.
    ///
    /// 2. Reject a BDS 2,1 candidate whose registration string does not match
    ///    any of the four structural buckets for the country inferred from
    ///    [`CommBContext::icao24`].
    ///
    /// 3. Winner-take-all: when a high-confidence candidate (BDS 1,0, 2,0,
    ///    or 3,0 by byte-header) is present, evict all other candidates.
    ///    BDS 2,1 is validated but is not used as a winner because short
    ///    registration-looking phantoms are too common.
    ///
    /// 4. Drop a GICB candidate (BDS 1,7 / 1,8 / 1,9) whose capability
    ///    bitmap contradicts [`CommBContext::seen_bds`], but only as a
    ///    tie-breaker (at least one non-GICB candidate must also survive).
    ///
    /// 5. Reject a BDS 0,5 candidate whose locally-decoded CPR position
    ///    differs by more than 5 NM from [`CommBContext::last_position`].
    ///
    /// Without `bds-infer` only the legacy BDS 5,0 × BDS 6,0 mutual eviction
    /// is applied.
    pub fn sanitize(&mut self, context: Option<&CommBContext<'_>>) {
        #[cfg(feature = "bds-infer")]
        if let Some(ctx) = context {
            sanitize_candidates(self, ctx);
        }
        #[cfg(not(feature = "bds-infer"))]
        {
            let _ = context;
            if self.bds50.is_some() && self.bds60.is_some() {
                self.bds50 = None;
                self.bds60 = None;
            }
        }
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
    /// Sanitize decoded Comm-B candidates.
    ///
    /// See [`DF20DataSelector::sanitize`] for the full rule description.
    pub fn sanitize(&mut self, context: Option<&CommBContext<'_>>) {
        #[cfg(feature = "bds-infer")]
        if let Some(ctx) = context {
            sanitize_candidates(self, ctx);
        }
        #[cfg(not(feature = "bds-infer"))]
        {
            let _ = context;
            if self.bds50.is_some() && self.bds60.is_some() {
                self.bds50 = None;
                self.bds60 = None;
            }
        }
    }
}

/// Context for Comm-B message validation and sanitization.
///
/// Passed to [`DF20DataSelector::sanitize`] and [`DF21DataSelector::sanitize`].
/// All fields are optional so callers can populate only what they have.
#[derive(Debug, Default, Clone)]
pub struct CommBContext<'a> {
    /// Most recent barometric altitude for this aircraft, in feet.
    ///
    /// For DF 20, use the outer AC13 altitude from the same message (same
    /// barometric sensor, should agree within Mode-C quantization with any
    /// genuine BDS 0,5 candidate). For DF 21, or when no DF 20 altitude is
    /// available, use the most recent ADS-B barometric altitude from this
    /// aircraft. Either way, a BDS 0,5 candidate whose decoded altitude
    /// differs by more than 100 ft is almost certainly a phantom.
    pub last_altitude: Option<i32>,

    /// ICAO 24-bit aircraft address.
    ///
    /// Used to validate a surviving BDS 2,1 candidate's registration
    /// string against the country-prefix patterns inferred from this address.
    pub icao24: Option<u32>,

    /// Set of BDS register codes the aircraft has been observed transmitting.
    ///
    /// Used for the GICB bitmap tie-breaker: drop a GICB candidate (BDS 1,7 / 1,8 / 1,9)
    /// whose capability bitmap contradicts this evidence set, but only when at
    /// least one non-GICB candidate also survives.
    pub seen_bds: Option<&'a BTreeSet<u8>>,

    /// Most recent decoded position for this aircraft.
    ///
    /// Used for CPR position cross-validation: locally CPR-decode the BDS 0,5
    /// candidate using this as the reference; reject when the decoded position
    /// differs by more than 5 NM. Phantom CPR bits scatter uniformly over a
    /// ~360 × 360 NM cell; genuine positions cluster within 1–2 NM of the
    /// reference.
    pub last_position: Option<super::cpr::Position>,

    /// BDS codes the aircraft's capability report declares as supported,
    /// once enough consistent reports have been seen.
    ///
    /// When present, any surviving candidate whose BDS code appears in the
    /// BDS 1,7 schema but is absent from this set is rejected — the aircraft
    /// has declared it does not support that register.
    pub stable_supported_bds: Option<&'a BTreeSet<u8>>,
}

/// Trait abstracting the candidate fields common to both [`DF20DataSelector`]
/// and [`DF21DataSelector`], so the sanitize logic can be written once.
#[cfg(feature = "bds-infer")]
#[allow(dead_code)]
trait CommBCandidates {
    fn bds05_alt(&self) -> Option<i32>;
    fn bds05_msg(&self) -> Option<&AirbornePosition>;
    fn clear_bds05(&mut self);
    fn bds21_reg(&self) -> Option<&str>;
    fn clear_bds21(&mut self);
    fn has_bds10(&self) -> bool;
    fn has_bds20(&self) -> bool;
    fn has_bds21(&self) -> bool;
    fn has_bds30(&self) -> bool;
    fn has_bds17(&self) -> bool;
    fn has_bds18(&self) -> bool;
    fn has_bds19(&self) -> bool;
    fn has_non_gicb(&self) -> bool;
    fn clear_bds17(&mut self);
    fn clear_bds18(&mut self);
    fn clear_bds19(&mut self);
    fn clear_non_winner(&mut self);
    fn gicb_bitmap_consistent(&self, reg: &BTreeSet<u8>) -> (bool, bool, bool);
    /// Reject candidates for BDS codes in the BDS 1,7 schema that the
    /// aircraft has declared unsupported (i.e. absent from `supported`).
    fn reject_unsupported(&mut self, supported: &BTreeSet<u8>);
}

#[cfg(feature = "bds-infer")]
macro_rules! impl_commb_candidates {
    ($t:ty) => {
        impl CommBCandidates for $t {
            fn bds05_alt(&self) -> Option<i32> {
                self.bds05.as_ref().and_then(|b| b.alt)
            }
            fn bds05_msg(&self) -> Option<&AirbornePosition> {
                self.bds05.as_ref()
            }
            fn clear_bds05(&mut self) {
                self.bds05 = None;
            }
            fn bds21_reg(&self) -> Option<&str> {
                self.bds21
                    .as_ref()
                    .and_then(|b| b.aircraft_registration.as_deref())
            }
            fn clear_bds21(&mut self) {
                self.bds21 = None;
            }
            fn has_bds10(&self) -> bool {
                self.bds10.is_some()
            }
            fn has_bds20(&self) -> bool {
                self.bds20.is_some()
            }
            fn has_bds21(&self) -> bool {
                self.bds21.is_some()
            }
            fn has_bds30(&self) -> bool {
                self.bds30.is_some()
            }
            fn has_bds17(&self) -> bool {
                self.bds17.is_some()
            }
            fn has_bds18(&self) -> bool {
                self.bds18.is_some()
            }
            fn has_bds19(&self) -> bool {
                self.bds19.is_some()
            }
            fn has_non_gicb(&self) -> bool {
                // any surviving candidate that is NOT a GICB register
                self.bds05.is_some()
                    || self.bds10.is_some()
                    || self.bds20.is_some()
                    || self.bds21.is_some()
                    || self.bds30.is_some()
                    || self.bds40.is_some()
                    || self.bds44.is_some()
                    || self.bds45.is_some()
                    || self.bds50.is_some()
                    || self.bds60.is_some()
                    || self.bds65.is_some()
            }
            fn clear_bds17(&mut self) {
                self.bds17 = None;
            }
            fn clear_bds18(&mut self) {
                self.bds18 = None;
            }
            fn clear_bds19(&mut self) {
                self.bds19 = None;
            }
            /// Evict every candidate that is NOT in the high-confidence tier.
            fn clear_non_winner(&mut self) {
                self.bds05 = None;
                self.bds17 = None;
                self.bds18 = None;
                self.bds19 = None;
                self.bds40 = None;
                self.bds44 = None;
                self.bds45 = None;
                self.bds50 = None;
                self.bds60 = None;
                self.bds65 = None;
            }
            /// Returns (bds17_ok, bds18_ok, bds19_ok): whether each GICB
            /// candidate's bitmap is consistent with the evidence set.
            ///
            /// For each code in `seen` (registers this aircraft has been
            /// observed transmitting) that appears in the GICB schema, the
            /// corresponding capability bit in the decoded struct must be
            /// `true`. A phantom candidate that happens to pass the GICB
            /// decoder will satisfy this by chance only.
            fn gicb_bitmap_consistent(
                &self,
                seen: &BTreeSet<u8>,
            ) -> (bool, bool, bool) {
                // BDS 1,7 schema: the capability bits it can represent
                // as `true`/`false`.
                let b17 = self.bds17.as_ref().map_or(true, |b| {
                    // For every code in `seen` that BDS 1,7 tracks,
                    // the bit must be set to true.
                    if seen.contains(&0x05u8) && !b.bds05 {
                        return false;
                    }
                    if seen.contains(&0x06u8) && !b.bds06 {
                        return false;
                    }
                    if seen.contains(&0x08u8) && !b.bds08 {
                        return false;
                    }
                    if seen.contains(&0x09u8) && !b.bds09 {
                        return false;
                    }
                    if seen.contains(&0x20u8) && !b.bds20 {
                        return false;
                    }
                    if seen.contains(&0x40u8) && !b.bds40 {
                        return false;
                    }
                    if seen.contains(&0x50u8) && !b.bds50 {
                        return false;
                    }
                    if seen.contains(&0x60u8) && !b.bds60 {
                        return false;
                    }
                    true
                });
                let b18 = self.bds18.as_ref().map_or(true, |b| {
                    // BDS 1,8 schema covers the lower register range.
                    if seen.contains(&0x05u8) && !b.bds05 {
                        return false;
                    }
                    if seen.contains(&0x06u8) && !b.bds06 {
                        return false;
                    }
                    if seen.contains(&0x08u8) && !b.bds08 {
                        return false;
                    }
                    if seen.contains(&0x09u8) && !b.bds09 {
                        return false;
                    }
                    if seen.contains(&0x10u8) && !b.bds10 {
                        return false;
                    }
                    if seen.contains(&0x17u8) && !b.bds17 {
                        return false;
                    }
                    if seen.contains(&0x18u8) && !b.bds18 {
                        return false;
                    }
                    if seen.contains(&0x19u8) && !b.bds19 {
                        return false;
                    }
                    if seen.contains(&0x20u8) && !b.bds20 {
                        return false;
                    }
                    if seen.contains(&0x21u8) && !b.bds21 {
                        return false;
                    }
                    if seen.contains(&0x30u8) && !b.bds30 {
                        return false;
                    }
                    true
                });
                let b19 = self.bds19.as_ref().map_or(true, |_b| {
                    // BDS 1,9 is always treated as consistent — very few
                    // false positives survive to this point anyway.
                    true
                });
                (b17, b18, b19)
            }
            fn reject_unsupported(&mut self, supported: &BTreeSet<u8>) {
                // BDS 1,7 schema: codes it tracks. For each code absent from
                // `supported`, clear the corresponding candidate if present.
                if !supported.contains(&0x05u8) {
                    self.bds05 = None;
                }
                // 0x06 surface position: not typically in EHS inference scope
                if !supported.contains(&0x20u8) {
                    self.bds20 = None;
                }
                if !supported.contains(&0x21u8) {
                    self.bds21 = None;
                }
                if !supported.contains(&0x40u8) {
                    self.bds40 = None;
                }
                if !supported.contains(&0x44u8) {
                    self.bds44 = None;
                }
                if !supported.contains(&0x45u8) {
                    self.bds45 = None;
                }
                if !supported.contains(&0x50u8) {
                    self.bds50 = None;
                }
                if !supported.contains(&0x60u8) {
                    self.bds60 = None;
                }
            }
        }
    };
}

#[cfg(feature = "bds-infer")]
impl_commb_candidates!(DF20DataSelector);
#[cfg(feature = "bds-infer")]
impl_commb_candidates!(DF21DataSelector);

/// Apply contextual checks to any set of surviving BDS candidates.
#[cfg(feature = "bds-infer")]
fn sanitize_candidates<C: CommBCandidates>(
    sel: &mut C,
    ctx: &CommBContext<'_>,
) {
    // Altitude cross-check: reject a BDS 0,5 candidate whose decoded altitude
    // differs > 100 ft from the last known barometric altitude.
    // For a genuine BDS 0,5 both values come from the same on-board sensor
    // and must agree within Mode-C quantisation (25 ft); for a phantom they
    // are uncorrelated.
    if let (Some(cand_alt), Some(ref_alt)) =
        (sel.bds05_alt(), ctx.last_altitude)
    {
        if (cand_alt - ref_alt).abs() > 100 {
            sel.clear_bds05();
        }
    }

    // Capability-based rejection: once the aircraft's BDS 1,7 capability
    // set is stable (seen in at least 3 consistent records), reject any
    // candidate for a BDS code the aircraft has declared unsupported.
    if let Some(supported) = ctx.stable_supported_bds {
        sel.reject_unsupported(supported);
    }

    // Registration validation: reject a BDS 2,1 candidate whose registration
    // string does not match any of the four structural buckets for the country
    // inferred from the aircraft's ICAO address range.
    if let (Some(reg), Some(addr)) =
        (sel.bds21_reg().map(str::to_owned), ctx.icao24)
    {
        if !super::bds::bds21::validate_registration(&reg, addr) {
            sel.clear_bds21();
        }
    }

    // Winner-take-all: BDS 1,0 / 2,0 / 3,0 are identified by a mandatory
    // byte-header prefix that no other register produces. BDS 2,1 is not a
    // winner: even after registration validation, short registration-looking
    // phantoms are common enough that they must not evict other candidates.
    let has_winner = sel.has_bds10() || sel.has_bds20() || sel.has_bds30();
    if has_winner {
        sel.clear_non_winner();
        return; // GICB and CPR checks are irrelevant once only high-confidence candidates remain
    }

    // GICB bitmap tie-breaker: drop a GICB candidate (BDS 1,7 / 1,8 / 1,9)
    // whose capability bitmap contradicts the aircraft's known transmission
    // history, but only when at least one non-GICB candidate also survives
    // (singleton GICB records are kept — some legitimate records have
    // intermittent zero bits due to sub-format / equipment-cycle effects).
    if let Some(seen) = ctx.seen_bds {
        if sel.has_non_gicb() {
            let (b17_ok, b18_ok, _b19_ok) = sel.gicb_bitmap_consistent(seen);
            if !b17_ok {
                sel.clear_bds17();
            }
            if !b18_ok {
                sel.clear_bds18();
            }
            // BDS 1,9 is always kept (treated as consistent — see above).
        }
    }

    // CPR position cross-check: reject a BDS 0,5 candidate whose
    // locally-decoded position differs by more than 5 NM from the aircraft's
    // last known position. Phantom CPR bits scatter uniformly over a
    // ~360 × 360 NM local CPR cell (phantom acceptance ≈ 0.06 % at 5 NM).
    // Genuine candidates cluster within 1–2 NM of the reference (q99 ≈ 1.6 NM).
    if let (Some(msg), Some(ref_pos)) =
        (sel.bds05_msg(), ctx.last_position.as_ref())
    {
        use super::cpr::{airborne_position_with_reference, distance_nm};
        let decoded = airborne_position_with_reference(
            msg,
            ref_pos.latitude,
            ref_pos.longitude,
        );
        match decoded {
            Some(cand_pos) if distance_nm(&cand_pos, ref_pos) <= 5.0 => {}
            _ => {
                sel.clear_bds05();
            }
        }
    }
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
            reader.skip_bits(5, Order::Msb0)?;
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
            Ok(bds40) => {
                keep_scored_candidate!(result.bds40 = bds40, Bds40, "BDS40")
            }
            Err(e) => debug!("Hypothesis BDS40: {}", e.to_string()),
        }
        match MeteorologicalRoutineAirReport::try_from(buf.as_slice()) {
            Ok(bds44) => {
                keep_scored_candidate!(result.bds44 = bds44, Bds44, "BDS44")
            }
            Err(e) => debug!("Hypothesis BDS44: {}", e.to_string()),
        }
        match MeteorologicalHazardReport::try_from(buf.as_slice()) {
            Ok(bds45) => {
                keep_scored_candidate!(result.bds45 = bds45, Bds45, "BDS45")
            }
            Err(e) => debug!("Hypothesis BDS45: {}", e.to_string()),
        }
        match TrackAndTurnReport::try_from(buf.as_slice()) {
            Ok(bds50) => {
                keep_scored_candidate!(result.bds50 = bds50, Bds50, "BDS50")
            }
            Err(e) => debug!("Hypothesis BDS50: {}", e.to_string()),
        }
        match HeadingAndSpeedReport::try_from(buf.as_slice()) {
            Ok(bds60) => {
                keep_scored_candidate!(result.bds60 = bds60, Bds60, "BDS60")
            }
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
            Ok(bds40) => {
                keep_scored_candidate!(result.bds40 = bds40, Bds40, "BDS40")
            }
            Err(e) => debug!("Hypothesis BDS40: {}", e.to_string()),
        }
        match MeteorologicalRoutineAirReport::try_from(buf.as_slice()) {
            Ok(bds44) => {
                keep_scored_candidate!(result.bds44 = bds44, Bds44, "BDS44")
            }
            Err(e) => debug!("Hypothesis BDS44: {}", e.to_string()),
        }
        match MeteorologicalHazardReport::try_from(buf.as_slice()) {
            Ok(bds45) => {
                keep_scored_candidate!(result.bds45 = bds45, Bds45, "BDS45")
            }
            Err(e) => debug!("Hypothesis BDS45: {}", e.to_string()),
        }
        match TrackAndTurnReport::try_from(buf.as_slice()) {
            Ok(bds50) => {
                keep_scored_candidate!(result.bds50 = bds50, Bds50, "BDS50")
            }
            Err(e) => debug!("Hypothesis BDS50: {}", e.to_string()),
        }
        match HeadingAndSpeedReport::try_from(buf.as_slice()) {
            Ok(bds60) => {
                keep_scored_candidate!(result.bds60 = bds60, Bds60, "BDS60")
            }
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
/// MessageProcessor::new(&mut message, &mut aircraft)
///     .sanitize_commb()
///     .finish();
/// ```
pub struct MessageProcessor<'a> {
    message: &'a mut super::Message,
    aircraft: &'a mut BTreeMap<ICAO, AircraftState>,
}

impl<'a> MessageProcessor<'a> {
    /// Create a new message processor
    pub fn new(
        message: &'a mut super::Message,
        aircraft: &'a mut BTreeMap<ICAO, AircraftState>,
    ) -> Self {
        Self { message, aircraft }
    }

    /// Sanitize Comm-B data using aircraft state context
    pub fn sanitize_commb(self) -> Self {
        use super::DF::*;

        // Resolve per-aircraft state for the icao24 in this message.
        let icao24_addr: Option<ICAO> = match &self.message.df {
            CommBAltitudeReply { ap, .. } => Some(ICAO::from(*ap)),
            CommBIdentityReply { ap, .. } => Some(ICAO::from(*ap)),
            _ => None,
        };
        let state = icao24_addr.as_ref().and_then(|ap| self.aircraft.get(ap));

        match &mut self.message.df {
            CommBAltitudeReply { bds, ac, ap, .. } => {
                let context = CommBContext {
                    last_altitude: ac
                        .0
                        .or_else(|| state.and_then(|s| s.last_altitude())),
                    icao24: Some(ap.0),
                    seen_bds: state.map(|s| s.seen_bds()),
                    last_position: state.and_then(|s| s.last_position()),
                    stable_supported_bds: state
                        .and_then(|s| s.stable_supported_bds()),
                };
                bds.sanitize(Some(&context));
            }
            CommBIdentityReply { bds, ap, .. } => {
                let context = CommBContext {
                    last_altitude: state.and_then(|s| s.last_altitude()),
                    icao24: Some(ap.0),
                    seen_bds: state.map(|s| s.seen_bds()),
                    last_position: state.and_then(|s| s.last_position()),
                    stable_supported_bds: state
                        .and_then(|s| s.stable_supported_bds()),
                };
                bds.sanitize(Some(&context));
            }
            _ => {}
        }
        self
    }

    /// Record observed BDS registers into the per-aircraft evidence set.
    ///
    /// Must be called **after** [`sanitize_commb`](Self::sanitize_commb) so
    /// that only unambiguous (surviving) candidates are recorded. Each
    /// surviving data register (BDS 0,5 / 1,0 / 2,0 / 2,1 / 3,0 / 4,0 /
    /// 4,4 / 4,5 / 5,0 / 6,0 / 6,5) updates the aircraft's `seen_bds` set.
    ///
    /// GICB capability registers (BDS 1,7 / 1,8 / 1,9) are intentionally
    /// not recorded: the bitmap tie-breaker checks whether the registers
    /// *declared* in the capability bitmap have been observed, not whether
    /// the GICB register itself has been seen.
    pub fn record_observed_bds(self) -> Self {
        use super::DF::*;
        let icao = match &self.message.df {
            CommBAltitudeReply { ap, .. } => Some(ICAO::from(*ap)),
            CommBIdentityReply { ap, .. } => Some(ICAO::from(*ap)),
            _ => None,
        };
        if let Some(icao) = icao {
            if let Some(state) = self.aircraft.get_mut(&icao) {
                let record = |state: &mut AircraftState, code: u8| {
                    state.record_bds(code);
                };
                // Record only data registers, not GICB capability registers
                // (0x17 / 0x18 / 0x19), which are never consulted by the
                // bitmap consistency check.
                let record_data =
                    |state: &mut AircraftState, bds: &DF20DataSelector| {
                        if bds.bds05.is_some() {
                            record(state, 0x05);
                        }
                        if bds.bds10.is_some() {
                            record(state, 0x10);
                        }
                        if bds.bds20.is_some() {
                            record(state, 0x20);
                        }
                        if bds.bds21.is_some() {
                            record(state, 0x21);
                        }
                        if bds.bds30.is_some() {
                            record(state, 0x30);
                        }
                        if bds.bds40.is_some() {
                            record(state, 0x40);
                        }
                        if bds.bds44.is_some() {
                            record(state, 0x44);
                        }
                        if bds.bds45.is_some() {
                            record(state, 0x45);
                        }
                        if bds.bds50.is_some() {
                            record(state, 0x50);
                        }
                        if bds.bds60.is_some() {
                            record(state, 0x60);
                        }
                        if bds.bds65.is_some() {
                            record(state, 0x65);
                        }
                    };
                match &self.message.df {
                    CommBAltitudeReply { bds, .. } => {
                        record_data(state, bds);
                        if let Some(b17) = &bds.bds17 {
                            state.update_bds17(b17);
                        }
                    }
                    CommBIdentityReply { bds, .. } => {
                        // DF21DataSelector has the same fields; use a
                        // separate arm rather than trying to unify types.
                        if bds.bds05.is_some() {
                            record(state, 0x05);
                        }
                        if bds.bds10.is_some() {
                            record(state, 0x10);
                        }
                        if bds.bds20.is_some() {
                            record(state, 0x20);
                        }
                        if bds.bds21.is_some() {
                            record(state, 0x21);
                        }
                        if bds.bds30.is_some() {
                            record(state, 0x30);
                        }
                        if bds.bds40.is_some() {
                            record(state, 0x40);
                        }
                        if bds.bds44.is_some() {
                            record(state, 0x44);
                        }
                        if bds.bds45.is_some() {
                            record(state, 0x45);
                        }
                        if bds.bds50.is_some() {
                            record(state, 0x50);
                        }
                        if bds.bds60.is_some() {
                            record(state, 0x60);
                        }
                        if bds.bds65.is_some() {
                            record(state, 0x65);
                        }
                        if let Some(b17) = &bds.bds17 {
                            state.update_bds17(b17);
                        }
                    }
                    _ => {}
                }
            }
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
