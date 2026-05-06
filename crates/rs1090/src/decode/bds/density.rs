//! Density-based scoring of decoded BDS candidates.
//!
//! For each candidate BDS code, a per-field log-density is computed under a
//! fitted Gaussian or Laplace distribution, and the **mean** of those
//! per-field log-densities is compared against [`DENSITY_THRESHOLD`]
//! (default −3.0). When the mean falls below the threshold, the candidate
//! is rejected.
//!
//! Distribution parameters are calibrated on a month of CAT 048 ground
//! truth such that the p99 of legitimate scores ≈ −2.0 — high enough that
//! cruise extremes, turning aircraft and high-wind situations still pass.
//!
//! Because the score is a *mean* across multiple decoded fields, it cannot
//! live inside a per-field deku reader. It is applied **after** a
//! successful decode in [`super::decode_bds`] / [`super::decode_payload`].
//! Candidates whose decoded struct exposes no scored field are accepted
//! unconditionally — there is no signal to score against.
//!
//! This module is only compiled when the `bds-infer` feature is enabled.

use super::DecodedBds;

/// Mean log-density rejection threshold (default: −3.0).
pub const DENSITY_THRESHOLD: f64 = -3.0;

/// Gaussian log-density (un-normalised; the constant `−ln(σ√2π)` is shared
/// across candidates and would just shift the threshold, so it is omitted).
#[inline]
fn gauss(value: f64, mu: f64, sigma: f64) -> f64 {
    let z = (value - mu) / sigma;
    -0.5 * z * z
}

/// Laplace log-density (un-normalised; the constant `−ln(2b)` is omitted
/// for the same reason as in [`gauss`]).
#[inline]
fn laplace(value: f64, median: f64, b: f64) -> f64 {
    if b <= 0.0 {
        return 0.0;
    }
    -(value - median).abs() / b
}

/// Accumulator helper: push `(log_density, 1)` only when `value` is `Some`.
struct Acc {
    total: f64,
    count: u32,
}

impl Acc {
    #[inline]
    fn new() -> Self {
        Self {
            total: 0.0,
            count: 0,
        }
    }
    #[inline]
    fn add(&mut self, ld: f64) {
        self.total += ld;
        self.count += 1;
    }
    #[inline]
    fn mean(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.total / f64::from(self.count))
        }
    }
}

/// Compute the mean log-density across the scored fields of a decoded BDS
/// candidate.
///
/// Returns `None` when no scored field is present (the candidate is then
/// accepted by [`passes`]).
#[must_use]
pub fn density_score(decoded: &DecodedBds) -> Option<f64> {
    let mut acc = Acc::new();
    match decoded {
        DecodedBds::Bds05(p) => {
            // altitude: gauss(26302, 10573)
            if let Some(alt) = p.alt {
                acc.add(gauss(f64::from(alt), 26302.0, 10573.0));
            }
        }
        DecodedBds::Bds40(s) => {
            // barometric_setting: laplace(1013.2, 5.4)
            if let Some(v) = s.barometric_setting {
                acc.add(laplace(v, 1013.2, 5.4));
            }
            // selected_mcp / selected_fms: gauss(24688, 10844)
            if let Some(v) = s.selected_altitude_mcp {
                acc.add(gauss(f64::from(v), 24688.0, 10844.0));
            }
            if let Some(v) = s.selected_altitude_fms {
                acc.add(gauss(f64::from(v), 24688.0, 10844.0));
            }
        }
        DecodedBds::Bds50(t) => {
            // roll: laplace(0, 10)
            if let Some(v) = t.roll_angle {
                acc.add(laplace(v, 0.0, 10.0));
            }
            // track_rate: laplace(0, 0.70)
            if let Some(v) = t.track_rate {
                acc.add(laplace(v, 0.0, 0.70));
            }
            // TAS: gauss(413, 120)
            if let Some(v) = t.true_airspeed {
                acc.add(gauss(f64::from(v), 413.0, 120.0));
            }
            // groundspeed: gauss(418, 129)
            if let Some(v) = t.groundspeed {
                acc.add(gauss(f64::from(v), 418.0, 129.0));
            }
        }
        DecodedBds::Bds60(h) => {
            // IAS: gauss(272, 55)
            if let Some(v) = h.indicated_airspeed {
                acc.add(gauss(f64::from(v), 272.0, 55.0));
            }
            // Mach: gauss(0.69, 0.21)
            if let Some(v) = h.mach_number {
                acc.add(gauss(v, 0.69, 0.21));
            }
            // vrate_inertial: gauss(-298, 1669)
            if let Some(v) = h.inertial_vertical_velocity {
                acc.add(gauss(f64::from(v), -298.0, 1669.0));
            }
        }
        DecodedBds::Bds44(m) => {
            // temperature: laplace(-21.0, 9.90). Always present (non-Option).
            acc.add(laplace(m.temperature, -21.0, 9.90));
            // wind_speed: gauss(52, 29)
            if let Some(v) = m.wind_speed {
                acc.add(gauss(f64::from(v), 52.0, 29.0));
            }
            // pressure (hPa): laplace(393, 290)
            if let Some(v) = m.pressure {
                acc.add(laplace(f64::from(v), 393.0, 290.0));
            }
        }
        DecodedBds::Bds45(m) => {
            // static_temperature: gauss(-14.2, 16.4)
            if let Some(v) = m.static_temperature {
                acc.add(gauss(v, -14.2, 16.4));
            }
            // static_pressure (hPa): laplace(393, 290)
            if let Some(v) = m.static_pressure {
                acc.add(laplace(f64::from(v), 393.0, 290.0));
            }
        }
        // No distributions are fitted for the remaining BDS codes
        // (06, 08, 09, 10, 17, 18, 19, 20, 21, 30, 61, 62, 65). They are
        // accepted unconditionally by the density rule (the decision is
        // deferred to other heuristics or to the structural decoders).
        _ => {}
    }
    acc.mean()
}

/// Decide whether the candidate passes the density rule alone.
///
/// Returns `true` when (a) the candidate has no scored field, or (b) the
/// mean log-density is at or above [`DENSITY_THRESHOLD`].
#[must_use]
pub fn passes(decoded: &DecodedBds) -> bool {
    match density_score(decoded) {
        Some(score) => score >= DENSITY_THRESHOLD,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cruise-typical BDS 50 (TAS 460, GS 470, roll −2°, track_rate 0)
    /// scores well above the rejection threshold.
    #[test]
    fn cruise_bds50_passes() {
        // Field-by-field hand-computed log-densities (un-normalised):
        //   roll = −2°  → laplace(0, 10)   = −0.20
        //   trk_rate=0  → laplace(0, 0.70) =  0.00
        //   TAS  = 460  → gauss(413, 120)  = −0.0767
        //   GS   = 470  → gauss(418, 129)  = −0.0813
        // mean ≈ −0.090, well above −3.0.
        let mut acc = Acc::new();
        acc.add(laplace(-2.0, 0.0, 10.0));
        acc.add(laplace(0.0, 0.0, 0.70));
        acc.add(gauss(460.0, 413.0, 120.0));
        acc.add(gauss(470.0, 418.0, 129.0));
        let m = acc.mean().unwrap();
        assert!(m > DENSITY_THRESHOLD, "cruise mean = {m}");
    }

    /// A BDS 60 with implausible-but-structurally-valid Mach (0.05) and
    /// IAS (50 kt) drifts well into the rejection zone.
    #[test]
    fn slow_bds60_fails() {
        // IAS  = 50   → gauss(272, 55)    ≈ −8.2
        // Mach = 0.05 → gauss(0.69, 0.21) ≈ −2.25
        // mean ≈ −5.2.
        let mut acc = Acc::new();
        acc.add(gauss(50.0, 272.0, 55.0));
        acc.add(gauss(0.05, 0.69, 0.21));
        let m = acc.mean().unwrap();
        assert!(m < DENSITY_THRESHOLD, "slow mean = {m}");
    }

    /// Per-field log-density helpers behave as advertised at the centre.
    #[test]
    fn density_at_centre_is_zero() {
        assert_eq!(gauss(1.0, 1.0, 2.0), 0.0);
        assert_eq!(laplace(1013.2, 1013.2, 5.4), 0.0);
        // Degenerate Laplace (b ≤ 0) returns 0 instead of dividing by zero.
        assert_eq!(laplace(1.0, 0.0, 0.0), 0.0);
    }
}
