use deku::prelude::*;
use serde::{Deserialize, Serialize};

/**
 * ## Meteorological Routine Air Report (BDS 4,4)
 *
 * Comm-B message providing meteorological data collected from aircraft sensors.  
 * Per ICAO Doc 9871 Table A-2-68: BDS code 4,4 — Meteorological routine air report
 *
 * Purpose: Allows meteorological data collection by ground systems for weather
 * forecasting and analysis.
 *
 * Message Structure (56 bits):
 * | FOM | WIND SPD | WIND DIR | TEMP | PRESS | TURB | HUMID | RSVD |
 * |-----|----------|----------|------|-------|------|-------|------|
 * | 4   | 1+9      | 9        | 11   | 1+11  | 1+2  | 1+6   | 4    |
 *
 * Field Encoding per ICAO Doc 9871:
 *
 * **FOM/Source** (bits 1-4): Figure of Merit / Navigation source
 *   - 0: Invalid
 *   - 1: INS (Inertial Navigation System)
 *   - 2: GNSS (GPS, etc.)
 *   - 3: DME/DME
 *   - 4: VOR/DME
 *   - 5-15: Reserved
 *
 * **Wind Speed** (bits 5-14):
 *   - Bit 5: Status (0=invalid, 1=valid)
 *   - Bits 6-14: 9-bit wind speed
 *     * LSB = 1 kt, MSB = 256 kt
 *     * Range: [0, 511] kt (Annex 3 requires [0, 250] kt)
 *
 * **Wind Direction** (bits 15-23):
 *   - 9-bit direction (true north)
 *   - MSB = 180 degrees, LSB = 180/256 degrees (≈0.703°)
 *   - Range: [0, 360] degrees
 *   - Only valid if wind_speed status is valid
 *
 * **Static Air Temperature** (bits 24-34):
 *   - Bit 24: Sign (0=positive, 1=negative)
 *   - Bits 25-34: 10-bit temperature value
 *     * MSB = 64°C, LSB = 0.25°C
 *     * Range: [-128, +128]°C (Annex 3 requires [-80, +60]°C)
 *     * Two's complement encoding
 *
 * **Average Static Pressure** (bits 35-46):
 *   - Bit 35: Status (0=invalid, 1=valid)
 *   - Bits 36-46: 11-bit pressure value
 *     * MSB = 1024 hPa, LSB = 1 hPa
 *     * **Direct encoding**: value = pressure in hPa (no offset)
 *     * Range: [0, 2048] hPa
 *   - Note: Not an Annex 3 requirement
 *
 * **Turbulence** (bits 47-49):
 *   - Bit 47: Status (0=invalid, 1=valid)
 *   - Bits 48-49: 2-bit turbulence level
 *     * 00: Nil, 01: Light, 10: Moderate, 11: Severe
 *     * Definitions per PANS-ATM (Doc 4444)
 *
 * **Humidity** (bits 50-56):
 *   - Bit 50: Status (0=invalid, 1=valid)
 *   - Bits 51-56: 6-bit humidity percentage
 *     * MSB = 100%, LSB = 100/64% (≈1.5625%)
 *     * Range: [0, 100]%
 *
 * **Reserved** (bits 57-60): 4 bits reserved
 *
 * Note: Two's complement coding used for all signed fields (§A.2.2.2)
 */

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
#[serde(tag = "bds", rename = "44")]
pub struct MeteorologicalRoutineAirReport {
    /// Figure of Merit / Source (bits 1-4): Per ICAO Doc 9871 Table A-2-68  
    /// Indicates navigation data source quality:
    ///   - 0: Invalid
    ///   - 1: INS (Inertial Navigation System)
    ///   - 2: GNSS (Global Navigation Satellite System)
    ///   - 3: DME/DME (Distance Measuring Equipment)
    ///   - 4: VOR/DME (VHF Omnidirectional Range / DME)
    ///   - 5-15: Reserved
    #[deku(bits = 4)]
    #[deku(reader = "read_figure_of_merit(deku::reader)")]
    pub figure_of_merit: FigureOfMerit,

    #[deku(reader = "read_wind_speed(deku::reader)")]
    /// Wind Speed (bits 5-14): Per ICAO Doc 9871 Table A-2-68  
    /// 9-bit wind speed in knots (LSB=1 kt, range [0, 511] kt)  
    /// Returns None if status bit (bit 5) is 0.
    pub wind_speed: Option<u16>,

    #[deku(reader = "read_wind_direction(deku::reader, *wind_speed)")]
    /// Wind Direction (bits 15-23): Per ICAO Doc 9871 Table A-2-68  
    /// 9-bit direction from true north (LSB=180/256°, range [0, 360]°)  
    /// Only valid if wind_speed is Some.
    pub wind_direction: Option<f64>,

    #[deku(reader = "read_temperature(deku::reader)")]
    /// Static Air Temperature (bits 24-34): Per ICAO Doc 9871 Table A-2-68  
    /// 11-bit signed temperature (LSB=0.25°C, range [-128, +128]°C)  
    /// Two's complement encoding.
    pub temperature: f64,

    #[deku(reader = "read_pressure(deku::reader)")]
    /// Average Static Pressure (bits 35-46): Per ICAO Doc 9871 Table A-2-68  
    /// 11-bit pressure in hPa (LSB=1 hPa, direct encoding, range [0, 2048] hPa)  
    /// Returns None if status bit (bit 35) is 0.
    pub pressure: Option<u16>,

    #[deku(reader = "read_turbulence(deku::reader)")]
    /// Turbulence (bits 47-49): Per ICAO Doc 9871 Table A-2-68  
    /// 2-bit turbulence level (Nil, Light, Moderate, Severe)  
    /// Definitions per PANS-ATM (Doc 4444).  
    /// Returns None if status bit (bit 47) is 0.
    pub turbulence: Option<Turbulence>,

    #[deku(reader = "read_humidity(deku::reader)")]
    /// Humidity (bits 50-56): Per ICAO Doc 9871 Table A-2-68  
    /// 6-bit humidity percentage (LSB=100/64%, range [0, 100]%)  
    /// Returns None if status bit (bit 50) is 0.
    pub humidity: Option<f64>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub enum FigureOfMerit {
    /// Invalid
    Invalid,
    /// Inertial Navigation System
    INS,
    /// Global Navigation Satellite System
    GNSS,
    /// DME (Distance Measuring Equipment) / DME
    DMEDME,
    /// VHF Omnidirectional Range / DME
    VORDME,
    Reserved(u8),
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub enum Turbulence {
    Nil,
    Light,
    Moderate,
    Severe,
}

fn read_figure_of_merit<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<FigureOfMerit, DekuError> {
    let value = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(4)),
    )?;

    let fom = match value {
        0 => FigureOfMerit::Invalid,
        1 => FigureOfMerit::INS,
        2 => FigureOfMerit::GNSS,
        3 => FigureOfMerit::DMEDME,
        4 => FigureOfMerit::VORDME,
        n @ 5..=15 => FigureOfMerit::Reserved(n),
        _ => unreachable!(),
    };
    Ok(fom)
}
fn read_wind_speed<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<u16>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(9)),
    )?;

    if !status {
        if value != 0 {
            return Err(DekuError::Assertion(
                "Invalid wind speed value".into(),
            ));
        } else {
            return Ok(None);
        }
    }
    if value > 250 {
        let msg = format!("Invalid wind speed {value} kts > 250 kts");
        return Err(DekuError::Assertion(msg.into()));
    }

    Ok(Some(value))
}

fn read_wind_direction<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
    speed: Option<u16>,
) -> Result<Option<f64>, DekuError> {
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(9)),
    )?;

    if speed.is_none() {
        if value != 0 {
            return Err(DekuError::Assertion(
                "Invalid wind direction value".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    Ok(Some(value as f64 * 180. / 256.))
}

fn read_temperature<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<f64, DekuError> {
    let sign = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(10)),
    )?;

    let temp = if sign == 1 {
        (value as f64 - 1024.) * 0.25
    } else {
        value as f64 * 0.25
    };

    if !(-80. ..=60.).contains(&temp) {
        let msg = "Invalid temperature value {}°C outside [-80, 60]";
        return Err(DekuError::Assertion(msg.into()));
    }
    Ok(temp)
}

fn read_pressure<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<u16>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(11)),
    )?;

    if !status {
        if value != 0 {
            return Err(DekuError::Assertion("Invalid pressure value".into()));
        } else {
            return Ok(None);
        }
    }

    // BDS 4,4 Average Static Pressure: 11-bit value represents pressure directly in hPa
    // Per ICAO Doc 9871 Table A-2-68:
    //   - Range: [0, 2048] hPa
    //   - LSB = 1 hPa (raw value = pressure in hPa)
    //   - MSB = 1024 hPa
    // No scaling formula needed (unlike BDS 4,0 which uses 800 + value * 0.1)
    Ok(Some(value))
}

fn read_turbulence<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<Turbulence>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(2)),
    )?;

    if !status {
        if value != 0 {
            return Err(DekuError::Assertion(
                "Invalid turbulence status".into(),
            ));
        } else {
            return Ok(None);
        }
    }

    let value = match value {
        0 => Some(Turbulence::Nil),
        1 => Some(Turbulence::Light),
        2 => Some(Turbulence::Moderate),
        3 => Some(Turbulence::Severe),
        _ => unreachable!(),
    };

    Ok(value)
}

fn read_humidity<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<f64>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(6)),
    )?;

    if !status {
        if value != 0 {
            return Err(DekuError::Assertion("Invalid humidity value".into()));
        } else {
            return Ok(None);
        }
    }

    Ok(Some(value as f64 * 100. / 64.))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use approx::assert_relative_eq;
    use hexlit::hex;

    #[test]
    fn test_valid_bds44() {
        let bytes = hex!("a0001692185bd5cf400000dfc696");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            let MeteorologicalRoutineAirReport {
                wind_speed,
                wind_direction,
                temperature,
                pressure,
                humidity,
                ..
            } = bds.bds44.unwrap();
            assert_eq!(wind_speed.unwrap(), 22);
            assert_relative_eq!(
                wind_direction.unwrap(),
                344.5,
                max_relative = 1e-3
            );
            assert_relative_eq!(temperature, -48.75, max_relative = 1e-3);
            assert_eq!(pressure, None);
            assert_eq!(humidity, None);
        } else {
            unreachable!();
        }
    }
}
