use deku::prelude::*;
use serde::{Deserialize, Serialize};

/**
 * ## Track and Turn Report (BDS 5,0)
 *
 * Comm-B message providing aircraft track and turn data.  
 * Per ICAO Doc 9871 Table A-2-80: BDS code 5,0 — Track and turn report
 *
 * Purpose: Provides track and turn data to ground systems for improved
 * trajectory prediction and conflict detection.
 *
 * Message Structure (56 bits):
 * | ROLL  | TRACK | GS   | RATE | TAS  |
 * |-------|-------|------|------|------|
 * | 1+1+9 | 1+1+10| 1+10 | 1+1+9| 1+10 |
 *
 * Field Encoding per ICAO Doc 9871:
 *
 * **Roll Angle** (bits 1-11):
 *   - Bit 1: Status (0=invalid, 1=valid)
 *   - Bit 2: Sign (0=right wing down, 1=left wing down)
 *   - Bits 3-11: 9-bit roll value
 *     * MSB = 45 degrees, LSB = 45/256 degrees (≈0.1758°)
 *     * Range: [-90, +90] degrees
 *     * Negative = left wing down (sign bit = 1)
 *     * Two's complement encoding
 *
 * **True Track Angle** (bits 12-23):
 *   - Bit 12: Status (0=invalid, 1=valid)
 *   - Bit 13: Sign (0=east, 1=west, e.g., 315° = -45°)
 *   - Bits 14-23: 10-bit track value
 *     * MSB = 90 degrees, LSB = 90/512 degrees (≈0.1758°)
 *     * Range: [-180, +180] degrees
 *     * Two's complement, converted to [0, 360]° for display
 *
 * **Ground Speed** (bits 24-34):
 *   - Bit 24: Status (0=invalid, 1=valid)
 *   - Bits 25-34: 10-bit ground speed value
 *     * MSB = 1024 kt, LSB = 1024/512 kt = 2 kt
 *     * Range: [0, 2046] kt
 *     * Formula: groundspeed = value × 2 kt
 *
 * **Track Angle Rate** (bits 35-45):
 *   - Bit 35: Status (0=invalid, 1=valid)
 *   - Bit 36: Sign (0=positive, 1=negative/minus)
 *   - Bits 37-45: 9-bit turn rate value
 *     * MSB = 8 deg/s, LSB = 8/256 deg/s (≈0.03125 deg/s)
 *     * Range: [-16, +16] deg/s
 *     * Two's complement encoding
 *     * Sign must agree with roll angle (left roll = left turn)
 *
 * **True Airspeed** (bits 46-56):
 *   - Bit 46: Status (0=invalid, 1=valid)
 *   - Bits 47-56: 10-bit airspeed value
 *     * MSB = 1024 kt, LSB = 2 kt
 *     * Range: [0, 2046] kt
 *     * Formula: airspeed = value × 2 kt
 *
 * Validation Rules per ICAO Doc 9871:
 * - If parameter exceeds range, use maximum allowable value (requires GFM intervention)
 * - If parameter unavailable, all bits set to ZERO by GFM
 * - LSB values obtained by rounding
 * - Data should be from sources controlling the aircraft (when possible)
 *
 * Implementation Validation:
 * - Roll angle magnitude typically < 50° (assertion if exceeded)
 * - Ground speed typically < 600 kt (assertion if exceeded)
 * - TAS typically within [80, 500] kt
 * - |GS - TAS| typically < 200 kt
 * - Roll angle and track rate signs must agree
 *
 * Note: Two's complement coding used for all signed fields (§A.2.2.2)
 */
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
#[serde(tag = "bds", rename = "50")]
pub struct TrackAndTurnReport {
    /// Roll Angle (bits 1-11): Per ICAO Doc 9871 Table A-2-80  
    /// Aircraft roll angle in degrees.  
    /// Encoding details:
    ///   - Bit 1: Status (0=invalid, 1=valid)
    ///   - Bit 2: Sign (0=right wing down, 1=left wing down)
    ///   - Bits 3-11: 9-bit roll magnitude
    ///   - MSB = 45 degrees
    ///   - LSB = 45/256 degrees (≈0.1758°)
    ///   - Formula: roll = value × (45/256) degrees (two's complement)
    ///   - Range: [-90, +90] degrees
    ///   - Negative values = left wing down (sign bit = 1)
    ///
    /// Returns None if status bit is 0.
    /// Implementation validates abs(roll) ≤ 50° (typical operational limit).
    #[deku(reader = "read_roll(deku::reader)")]
    #[serde(rename = "roll")]
    pub roll_angle: Option<f64>,

    /// True Track Angle (bits 12-23): Per ICAO Doc 9871 Table A-2-80  
    /// Aircraft track angle in degrees (true north reference).  
    /// Encoding details:
    ///   - Bit 12: Status (0=invalid, 1=valid)
    ///   - Bit 13: Sign (0=east, 1=west)
    ///   - Bits 14-23: 10-bit track magnitude
    ///   - MSB = 90 degrees
    ///   - LSB = 90/512 degrees (≈0.1758°)
    ///   - Formula: track = value × (90/512) degrees (two's complement)
    ///   - Range: [-180, +180] degrees (internal), converted to [0, 360]° for output
    ///   - Example: 315° encoded as -45°
    ///
    /// Returns None if status bit is 0.
    #[deku(reader = "read_track(deku::reader)")]
    #[serde(rename = "track")]
    pub track_angle: Option<f64>,

    /// Ground Speed (bits 24-34): Per ICAO Doc 9871 Table A-2-80  
    /// Aircraft ground speed in knots.  
    /// Encoding details:
    ///   - Bit 24: Status (0=invalid, 1=valid)
    ///   - Bits 25-34: 10-bit ground speed value
    ///   - MSB = 1024 kt
    ///   - LSB = 2 kt (1024/512)
    ///   - Formula: groundspeed = value × 2 kt
    ///   - Range: [0, 2046] kt
    ///
    /// Returns None if status bit is 0.
    /// Implementation validates groundspeed ≤ 600 kt (typical operational limit).
    #[deku(reader = "read_groundspeed(deku::reader)")]
    pub groundspeed: Option<u16>,

    /// Track Angle Rate (bits 35-45): Per ICAO Doc 9871 Table A-2-80  
    /// Rate of change of track angle (turn rate) in degrees/second.  
    /// Encoding details:
    ///   - Bit 35: Status (0=invalid, 1=valid)
    ///   - Bit 36: Sign (0=positive/right turn, 1=negative/left turn)
    ///   - Bits 37-45: 9-bit turn rate magnitude
    ///   - MSB = 8 deg/s
    ///   - LSB = 8/256 deg/s (≈0.03125 deg/s)
    ///   - Formula: rate = value × (8/256) deg/s (two's complement)
    ///   - Range: [-16, +16] deg/s
    ///   - Special value: 0b111111111 = data not available
    ///
    /// Returns None if status bit is 0 or value = 0b111111111.
    /// Implementation validates that sign agrees with roll_angle (left roll = left turn).
    #[deku(reader = "read_rate(deku::reader, *roll_angle)")]
    pub track_rate: Option<f64>,

    /// True Airspeed (bits 46-56): Per ICAO Doc 9871 Table A-2-80  
    /// Aircraft true airspeed in knots.  
    /// Encoding details:
    ///   - Bit 46: Status (0=invalid, 1=valid)
    ///   - Bits 47-56: 10-bit airspeed value
    ///   - MSB = 1024 kt
    ///   - LSB = 2 kt
    ///   - Formula: airspeed = value × 2 kt
    ///   - Range: [0, 2046] kt
    ///
    /// Returns None if status bit is 0.
    /// Implementation validates TAS ∈ [80, 500] kt and |GS - TAS| < 200 kt.
    /// Note: IAS (Indicated Airspeed) is available in BDS 6,0.
    #[deku(reader = "read_tas(deku::reader, *groundspeed)")]
    #[serde(rename = "TAS")]
    pub true_airspeed: Option<u16>,
}

fn read_roll<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<f64>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let sign = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(9)),
    )?;

    if !status {
        if (sign != 0) | (value != 0) {
            return Err(DekuError::Assertion(
                "Non-null value with invalid status: roll angle".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    let roll = if sign > 0 {
        (value as f64 - 512.) * 45. / 256.
    } else {
        value as f64 * 45. / 256.
    };
    if roll.abs() > 50. {
        return Err(DekuError::Assertion(
            format!("Roll angle: abs({roll}) > 50").into(),
        ));
    }
    Ok(Some(roll))
}

fn read_track<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<f64>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let sign = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(10)),
    )?;

    if !status {
        if (sign != 0) | (value != 0) {
            return Err(DekuError::Assertion(
                "Non-null value with invalid status: track angle".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    let value = if sign == 1 {
        value as i16 - 1024
    } else {
        value as i16
    };
    let mut track = value as f64 * 90. / 512.;
    if track < 0. {
        track += 360.
    }

    Ok(Some(track))
}

fn read_groundspeed<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<u16>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(10)),
    )?;

    if !status {
        if value != 0 {
            return Err(DekuError::Assertion(
                "Non-null value with invalid status: groundspeed".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    let gs = value * 2;
    if gs > 600 {
        return Err(DekuError::Assertion(
            format!("Groundspeed value: {gs} > 600").into(),
        ));
    }
    Ok(Some(gs))
}

fn read_rate<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
    roll: Option<f64>,
) -> Result<Option<f64>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let sign = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(9)),
    )?;

    if !status {
        if (sign != 0) | (value != 0) {
            return Err(DekuError::Assertion(
                "Non-null value with invalid status: track rate".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    if value == 0b111111111 {
        return Ok(None);
    }

    let value = if sign == 1 {
        value as i16 - 512
    } else {
        value as i16
    };
    let rate = value as f64 * 8. / 256.;

    if let Some(roll) = roll {
        if roll * rate < 0. {
            // signs must agree: left wing down = turn left
            return Err(DekuError::Assertion(
                format!(
                    "Roll angle {roll} and track rate {rate} signs do not agree."
                )
                .into(),
            ));
        }
    }

    Ok(Some(rate))
}

fn read_tas<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
    gs: Option<u16>,
) -> Result<Option<u16>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(10)),
    )?;

    if !status {
        if value != 0 {
            return Err(DekuError::Assertion(
                "Non-null value with invalid status: true air speed".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    let tas = value * 2;

    if let Some(gs) = gs {
        if !(80..=500).contains(&tas) | ((gs as i16 - tas as i16).abs() > 200) {
            return Err(DekuError::Assertion(format!(
                "TAS = {tas} must be within [80, 500] and abs(GS - TAS) = {gs} < 200"
            ).into()));
        }
    }
    Ok(Some(tas))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use approx::assert_relative_eq;
    use hexlit::hex;

    #[test]
    fn test_valid_bds50() {
        let bytes = hex!("a000139381951536e024d4ccf6b5");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            let TrackAndTurnReport {
                roll_angle,
                track_angle,
                true_airspeed,
                groundspeed,
                track_rate,
            } = bds.bds50.unwrap();
            assert_relative_eq!(roll_angle.unwrap(), 2.1, max_relative = 1e-2);
            assert_relative_eq!(
                track_angle.unwrap(),
                114.258,
                max_relative = 1e-3
            );
            assert_eq!(groundspeed.unwrap(), 438);
            assert_eq!(true_airspeed.unwrap(), 424);
            assert_relative_eq!(
                track_rate.unwrap(),
                0.125,
                max_relative = 1e-3
            );
        } else {
            unreachable!();
        }
    }
    #[test]
    fn test_invalid_bds50() {
        let bytes = hex!("a0000638fa81c10000000081a92f");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            assert_eq!(bds.bds50, None);
        } else {
            unreachable!();
        }
    }
}
