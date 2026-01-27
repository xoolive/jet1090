use deku::prelude::*;
use serde::{Deserialize, Serialize};

/**
 * ## Heading and Speed Report (BDS 6,0)
 *
 * Comm-B message providing aircraft heading and speed data.
 * Per ICAO Doc 9871 Table A-2-96: BDS code 6,0 — Heading and speed report
 *
 * Purpose: Provides heading and speed data to ground systems for improved
 * trajectory prediction and conflict detection.
 *
 * Message Structure (56 bits):
 * | HDG   | IAS  | MACH | BARO | INER |
 * |-------|------|------|------|------|
 * | 1+1+10| 1+10 | 1+10 | 1+1+9| 1+1+9|
 *
 * Field Encoding per ICAO Doc 9871:
 *
 * **Magnetic Heading** (bits 1-12):
 *   - Bit 1: Status (0=invalid, 1=valid)
 *   - Bit 2: Sign (0=east, 1=west)
 *   - Bits 3-12: 10-bit heading magnitude
 *     * MSB = 90 degrees
 *     * LSB = 90/512 degrees (≈0.1758°)
 *     * Range: [-180, +180] degrees (two's complement)
 *     * Converted to [0, 360]° for display (e.g., 315° = -45°)
 *
 * **Indicated Airspeed (IAS)** (bits 13-23):
 *   - Bit 13: Status (0=invalid, 1=valid)
 *   - Bits 14-23: 10-bit IAS value
 *     * MSB = 512 kt
 *     * LSB = 1 kt
 *     * Range: [0, 1023] kt
 *     * Formula: IAS = value × 1 kt
 *
 * **Mach Number** (bits 24-34):
 *   - Bit 24: Status (0=invalid, 1=valid)
 *   - Bits 25-34: 10-bit Mach value
 *     * MSB = 2.048 Mach
 *     * LSB = 2.048/512 Mach (≈0.004 Mach)
 *     * Range: [0, 4.092] Mach
 *     * Formula: Mach = value × (2.048/512)
 *
 * **Barometric Altitude Rate** (bits 35-45):
 *   - Bit 35: Status (0=invalid, 1=valid)
 *   - Bit 36: Sign (0=climbing, 1=descending/"below")
 *   - Bits 37-45: 9-bit vertical rate value
 *     * MSB = 8,192 ft/min
 *     * LSB = 32 ft/min (8192/256)
 *     * Range: [-16,384, +16,352] ft/min
 *     * Formula: rate = value × 32 ft/min (two's complement)
 *     * Special values: 0 or 511 = no rate information (returns 0)
 *
 * **Inertial Vertical Velocity** (bits 46-56):
 *   - Bit 46: Status (0=invalid, 1=valid)
 *   - Bit 47: Sign (0=climbing, 1=descending/"below")
 *   - Bits 48-56: 9-bit vertical velocity value
 *     * MSB = 8,192 ft/min
 *     * LSB = 32 ft/min (8192/256)
 *     * Range: [-16,384, +16,352] ft/min
 *     * Formula: velocity = value × 32 ft/min (two's complement)
 *     * Special values: 0 or 511 = no velocity information (returns 0)
 *
 * Data Source Notes per ICAO Doc 9871:
 * - **Barometric Altitude Rate**: Solely derived from barometric measurement
 *   (Air Data System or IRS/FMS). Usually unsteady and may suffer from
 *   barometric instrument inertia.
 * - **Inertial Vertical Velocity**: Derived from inertial equipment (IRS, AHRS)
 *   using different sources than barometric (FMC/GNSS integrated, FMC general,
 *   or IRS/FMS). More filtered and smooth parameter. When barometric altitude
 *   rate is integrated and smoothed with inertial data (baro-inertial), it
 *   shall be transmitted in this field.
 *
 * Validation Rules per ICAO Doc 9871:
 * - If parameter exceeds range, use maximum allowable value (requires GFM intervention)
 * - Data should be from sources controlling the aircraft (when possible)
 * - LSB values obtained by rounding
 * - If parameter unavailable, all bits set to ZERO by GFM
 *
 * Implementation Validation:
 * - IAS must be in range (0, 500] kt (operational validation)
 * - Mach must be in range (0, 1] (subsonic aircraft assumption)
 * - IAS and Mach cross-validation:
 *   * If IAS > 250 kt, then Mach must be ≥ 0.4 (250 kt ≈ Mach 0.45 at 10,000 ft)
 *   * If IAS < 150 kt, then Mach must be ≤ 0.5 (150 kt ≈ Mach 0.5 at FL400)
 * - Vertical rate/velocity abs value ≤ 6,000 ft/min (typical operational limit)
 *
 * Note: Two's complement coding used for all signed fields (§A.2.2.2)
 * Additional implementation guidelines in ICAO Doc 9871 §D.2.4.6
 */
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
#[serde(tag = "bds", rename = "60")]
pub struct HeadingAndSpeedReport {
    /// Magnetic Heading (bits 1-12): Per ICAO Doc 9871 Table A-2-96  
    /// Aircraft magnetic heading in degrees (magnetic north reference).  
    /// Encoding details:
    ///   - Bit 1: Status (0=invalid, 1=valid)
    ///   - Bit 2: Sign (0=east, 1=west)
    ///   - Bits 3-12: 10-bit heading magnitude
    ///   - MSB = 90 degrees
    ///   - LSB = 90/512 degrees (≈0.1758°)
    ///   - Formula: heading = value × (90/512) degrees (two's complement)
    ///   - Range: [-180, +180] degrees (internal), converted to [0, 360]° for output
    ///   - Example: 315° encoded as -45°
    ///
    /// Returns None if status bit is 0.
    #[deku(reader = "read_heading(deku::reader)")]
    #[serde(rename = "heading", skip_serializing_if = "Option::is_none")]
    pub magnetic_heading: Option<f64>,

    /// Indicated Airspeed (bits 13-23): Per ICAO Doc 9871 Table A-2-96  
    /// Aircraft indicated airspeed in knots.  
    /// Encoding details:
    ///   - Bit 13: Status (0=invalid, 1=valid)
    ///   - Bits 14-23: 10-bit IAS value
    ///   - MSB = 512 kt
    ///   - LSB = 1 kt
    ///   - Formula: IAS = value × 1 kt
    ///   - Range: [0, 1023] kt
    ///
    /// Returns None if status bit is 0.
    /// Implementation validates IAS ∈ (0, 500] kt (operational range).  
    /// Note: TAS (True Airspeed) is available in BDS 5,0.
    #[deku(reader = "read_ias(deku::reader)")]
    #[serde(rename = "IAS", skip_serializing_if = "Option::is_none")]
    pub indicated_airspeed: Option<u16>,

    /// Mach Number (bits 24-34): Per ICAO Doc 9871 Table A-2-96  
    /// Aircraft Mach number (ratio of aircraft speed to speed of sound).  
    /// Encoding details:
    ///   - Bit 24: Status (0=invalid, 1=valid)
    ///   - Bits 25-34: 10-bit Mach value
    ///   - MSB = 2.048 Mach
    ///   - LSB = 2.048/512 Mach (≈0.004 Mach)
    ///   - Formula: Mach = value × (2.048/512)
    ///   - Range: [0, 4.092] Mach
    ///
    /// Returns None if status bit is 0.  
    /// Implementation validates:
    ///   - Mach ∈ (0, 1] (subsonic aircraft)
    ///   - Cross-validation with IAS:
    ///     * If IAS > 250 kt: Mach ≥ 0.4 (250 kt ≈ Mach 0.45 at 10,000 ft)
    ///     * If IAS < 150 kt: Mach ≤ 0.5 (150 kt ≈ Mach 0.5 at FL400)
    #[deku(reader = "read_mach(deku::reader, *indicated_airspeed)")]
    #[serde(rename = "Mach", skip_serializing_if = "Option::is_none")]
    pub mach_number: Option<f64>,

    /// Barometric Altitude Rate (bits 35-45): Per ICAO Doc 9871 Table A-2-96  
    /// Vertical rate derived solely from barometric measurement in ft/min.  
    /// Encoding details:
    ///   - Bit 35: Status (0=invalid, 1=valid)
    ///   - Bit 36: Sign (0=climbing, 1=descending)
    ///   - Bits 37-45: 9-bit vertical rate magnitude
    ///   - MSB = 8,192 ft/min
    ///   - LSB = 32 ft/min (8192/256)
    ///   - Formula: rate = value × 32 ft/min (two's complement)
    ///   - Range: [-16,384, +16,352] ft/min
    ///   - Special values: 0 or 511 = no rate information (returns 0)
    ///
    /// Returns None if status bit is 0, returns Some(0) if value is 0 or 511.  
    /// Implementation validates abs(rate) ≤ 6,000 ft/min.  
    /// Source: Air Data System or Inertial Reference System/FMS.  
    /// Note: Usually unsteady and may suffer from barometric instrument inertia.
    #[deku(reader = "read_vertical(deku::reader)")]
    #[serde(
        rename = "vrate_barometric",
        skip_serializing_if = "Option::is_none"
    )]
    pub barometric_altitude_rate: Option<i16>,

    /// Inertial Vertical Velocity (bits 46-56): Per ICAO Doc 9871 Table A-2-96  
    /// Vertical velocity from inertial/navigational equipment in ft/min.  
    /// Encoding details:
    ///   - Bit 46: Status (0=invalid, 1=valid)
    ///   - Bit 47: Sign (0=climbing, 1=descending)
    ///   - Bits 48-56: 9-bit vertical velocity magnitude
    ///   - MSB = 8,192 ft/min
    ///   - LSB = 32 ft/min (8192/256)
    ///   - Formula: velocity = value × 32 ft/min (two's complement)
    ///   - Range: [-16,384, +16,352] ft/min
    ///   - Special values: 0 or 511 = no velocity information (returns 0)
    ///
    /// Returns None if status bit is 0, returns Some(0) if value is 0 or 511.  
    /// Implementation validates abs(velocity) ≤ 6,000 ft/min.  
    /// Sources: FMC/GNSS integrated, FMC (General), or IRS/FMS.  
    /// Note: More filtered and smooth than barometric rate. When barometric
    /// altitude rate is integrated and smoothed with inertial data
    /// (baro-inertial), it shall be transmitted in this field.
    #[deku(reader = "read_vertical(deku::reader)")]
    #[serde(
        rename = "vrate_inertial",
        skip_serializing_if = "Option::is_none"
    )]
    pub inertial_vertical_velocity: Option<i16>,
}

fn read_heading<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
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
                "Non-null value with invalid status: heading".into(),
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
    let mut heading = value as f64 * 90. / 512.;
    if heading < 0. {
        heading += 360.
    }

    Ok(Some(heading))
}

fn read_ias<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
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
                "Non-null value with invalid status: IAS".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    if (value == 0) | (value > 500) {
        return Err(DekuError::Assertion(
            format!("IAS value {value} is equal to 0 or greater than 500")
                .into(),
        ));
    }
    Ok(Some(value))
}

fn read_mach<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
    ias: Option<u16>,
) -> Result<Option<f64>, DekuError> {
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
                "Non-null value with invalid status: Mach".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    let mach = value as f64 * 2.048 / 512.;

    if (mach == 0.) | (mach > 1.) {
        return Err(DekuError::Assertion(
            format!("Mach value {mach} equal to 0 or greater than 1 ").into(),
        ));
    }
    if let Some(ias) = ias {
        /*
         * >>> pitot.aero.cas2mach(250, 10000)
         * 0.45229071380275554
         *
         * Let's do this:
         * 10000 ft has IAS max to 250, i.e. Mach 0.45
         * forbid IAS > 250 and Mach < 0.5
         */
        if (ias > 250) & (mach < 0.4) {
            return Err(DekuError::Assertion(
                format!(
                    "IAS: {ias} and Mach: {mach} (250kts is Mach 0.45 at 10,000 ft)"
                )
                .into(),
            ));
        }
        // this one is easy IAS = 150 (close to take-off) at FL 400 is Mach 0.5
        if (ias < 150) & (mach > 0.5) {
            return Err(DekuError::Assertion(
                format!(
                    "IAS: {ias} and Mach: {mach} (150kts is Mach 0.5 at FL400)"
                )
                .into(),
            ));
        }
    }
    Ok(Some(mach))
}

fn read_vertical<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<i16>, DekuError> {
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
                "Non-null value with invalid status: vertical rate".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    if (value == 0) | (value == 511) {
        // all zeros or all ones
        return Ok(Some(0));
    }
    let value = if sign == 1 {
        (value as i16 - 512) * 32
    } else {
        value as i16 * 32
    };

    if value.abs() > 6000 {
        Err(DekuError::Assertion(
            format!("Vertical rate absolute value {} > 6000", value.abs())
                .into(),
        ))
    } else {
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use approx::assert_relative_eq;
    use hexlit::hex;

    #[test]
    fn test_valid_bds60() {
        let bytes = hex!("a80004aaa74a072bfdefc1d5cb4f");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBIdentityReply { bds, .. } = msg.df {
            let HeadingAndSpeedReport {
                magnetic_heading,
                indicated_airspeed,
                mach_number,
                barometric_altitude_rate,
                inertial_vertical_velocity,
            } = bds.bds60.unwrap();
            assert_relative_eq!(
                magnetic_heading.unwrap(),
                110.391,
                max_relative = 1e-3
            );
            assert_eq!(indicated_airspeed.unwrap(), 259);
            assert_relative_eq!(mach_number.unwrap(), 0.7, max_relative = 1e-3);
            assert_eq!(barometric_altitude_rate.unwrap(), -2144);
            assert_eq!(inertial_vertical_velocity.unwrap(), -2016);
        } else {
            unreachable!();
        }
    }
    #[test]
    fn test_invalid_bds60() {
        let bytes = hex!("a0000638fa81c10000000081a92f");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            assert_eq!(bds.bds60, None);
        } else {
            unreachable!();
        }
    }
}
