//! Within-record cross-field penalties for decoded BDS candidates.
//!
//! For each candidate, [`penalty`] returns a **soft** value (always ≤ 0)
//! that captures aerodynamically or kinematically inconsistent
//! combinations of fields jointly decoded from the same Comm-B payload:
//!
//! * **BDS 50** — `−|TAS − groundspeed| / 100` (continuous; the airframe
//!   limit on |wind| ≈ 200 kt sets a soft anchor at penalty ≈ −2.0).
//! * **BDS 50** — `roll` × `track_rate` opposite sign during a turn:
//!   when `|roll| > 5°` and `|track_rate| > 0.3°/s`, mismatched signs
//!   contribute a flat `−2.0` penalty.
//! * **BDS 60** — `IAS / Mach` ratio outside `[250, 800]` kt: flat `−3.0`
//!   penalty (atmospheric physics constrains the ratio to this range).
//!
//! The penalty is summed with the [density score](super::density) and the
//! combined value is gated against [`super::density::DENSITY_THRESHOLD`].
//! It is invoked from the same post-decode hook as the density rule,
//! replacing what used to be field-reader hard rejects on the same
//! relations.
//!
//! This module is only compiled when the `bds-infer` feature is enabled.

use super::DecodedBds;

/// Cross-field penalty contributed by BDS-50 / BDS-60 relations
/// (always ≤ 0).
#[must_use]
pub fn penalty(decoded: &DecodedBds) -> f64 {
    let mut p = 0.0_f64;
    match decoded {
        DecodedBds::Bds50(t) => {
            // |TAS − GS| / 100, continuous proxy for headwind/tailwind.
            if let (Some(tas), Some(gs)) = (t.true_airspeed, t.groundspeed) {
                p -= (f64::from(tas) - f64::from(gs)).abs() / 100.0;
            }
            // Roll vs track_rate: signs must agree during a real turn.
            if let (Some(roll), Some(rate)) = (t.roll_angle, t.track_rate) {
                if roll.abs() > 5.0
                    && rate.abs() > 0.3
                    && (roll > 0.0) != (rate > 0.0)
                {
                    p -= 2.0;
                }
            }
        }
        DecodedBds::Bds60(h) => {
            // IAS / Mach ratio bounded by atmospheric physics.
            if let (Some(ias), Some(mach)) =
                (h.indicated_airspeed, h.mach_number)
            {
                if mach > 0.0 {
                    let ratio = f64::from(ias) / mach;
                    if !(250.0..=800.0).contains(&ratio) {
                        p -= 3.0;
                    }
                }
            }
        }
        _ => {}
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::bds::{decode_bds, DecodedBds};
    use hexlit::hex;

    /// Cruise-typical BDS 50 record: small |TAS-GS|, low roll/track-rate,
    /// signs agree. Penalty stays close to zero.
    #[test]
    fn cruise_bds50_small_penalty() {
        let payload = hex!("A0001838CA3E51B250C38D000000");
        let mb = &payload[4..11];
        if let Ok(d @ DecodedBds::Bds50(_)) = decode_bds(mb, 0x50) {
            let p = penalty(&d);
            assert!(p > -1.0, "penalty for cruise BDS 50 = {p}");
        }
    }

    /// A BDS 30 candidate exposes no fields touched by the cross-field
    /// rules and therefore receives a zero penalty.
    #[test]
    fn empty_decoded_bds_no_penalty() {
        let mb = hex!("30000000000000");
        if let Ok(d) = decode_bds(&mb, 0x30) {
            assert_eq!(penalty(&d), 0.0);
        }
    }
}
