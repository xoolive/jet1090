use deku::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::trace;

/**
 * ## Meteorological Hazard Report (BDS 4,5)
 *
 * Comm-B message providing reports on severity of meteorological hazards.  
 * Per ICAO Doc 9871 Table A-2-69: BDS code 4,5 — Meteorological hazard report
 *
 * Purpose: Provides hazard severity reports, particularly useful for low-level flight.
 *
 * Message Structure (56 bits):
 * | TURB | SHEAR | BURST | ICE | WAKE | TEMP | PRESS | HEIGHT | RSVD |
 * |------|-------|-------|-----|------|------|-------|--------|------|
 * | 1+2  | 1+2   | 1+2   | 1+2 | 1+2  | 1+10 | 1+11  | 1+12   | 5    |
 *
 * Field Encoding per ICAO Doc 9871:
 *
 * **Turbulence** (bits 1-3):
 *   - Bit 1: Status (0=invalid, 1=valid)
 *   - Bits 2-3: 2-bit severity level
 *
 * **Wind Shear** (bits 4-6):
 *   - Bit 4: Status (0=invalid, 1=valid)
 *   - Bits 5-6: 2-bit severity level
 *
 * **Microburst** (bits 7-9):
 *   - Bit 7: Status (0=invalid, 1=valid)
 *   - Bits 8-9: 2-bit severity level
 *
 * **Icing** (bits 10-12):
 *   - Bit 10: Status (0=invalid, 1=valid)
 *   - Bits 11-12: 2-bit severity level
 *
 * **Wake Vortex** (bits 13-15):
 *   - Bit 13: Status (0=invalid, 1=valid)
 *   - Bits 14-15: 2-bit severity level
 *
 * **Severity Level Encoding** (for all hazards):
 *   - 00 (0): Nil
 *   - 01 (1): Light
 *   - 10 (2): Moderate
 *   - 11 (3): Severe
 *   - Definitions per PANS-ATM (Doc 4444)
 *
 * **Static Air Temperature** (bits 16-26):
 *   - Bit 16: Status (0=invalid, 1=valid)
 *   - Bit 17: Sign (0=positive, 1=negative)
 *   - Bits 18-26: 9-bit temperature value
 *     * MSB = 64°C, LSB = 0.25°C
 *     * Range: [-128, +128]°C (Annex 3 requires [-80, +60]°C)
 *     * Two's complement encoding
 *
 * **Average Static Pressure** (bits 27-38):
 *   - Bit 27: Status (0=invalid, 1=valid)
 *   - Bits 28-38: 11-bit pressure value
 *     * MSB = 1024 hPa, LSB = 1 hPa
 *     * **Direct encoding**: value = pressure in hPa (no offset)
 *     * Range: [0, 2048] hPa
 *
 * **Radio Height** (bits 39-51):
 *   - Bit 39: Status (0=invalid, 1=valid)
 *   - Bits 40-51: 12-bit radio altitude
 *     * MSB = 32,768 ft, LSB = 16 ft
 *     * Range: [0, 65,528] ft
 *     * Radio height = radar altimeter reading above ground level
 *
 * **Reserved** (bits 52-56): 5 bits reserved
 *
 * Note: Two's complement coding used for all signed fields (§A.2.2.2)
 */

#[derive(
    Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone, Default,
)]
#[serde(tag = "bds", rename = "45")]
pub struct MeteorologicalHazardReport {
    #[deku(reader = "read_level(deku::reader)")]
    /// Turbulence (bits 1-3): Per ICAO Doc 9871 Table A-2-69  
    /// Severity level (Nil, Light, Moderate, Severe) per PANS-ATM (Doc 4444)  
    /// Returns None if status bit (bit 1) is 0.
    pub turbulence: Option<Level>,

    #[deku(reader = "read_level(deku::reader)")]
    /// Wind Shear (bits 4-6): Per ICAO Doc 9871 Table A-2-69  
    /// Severity level (Nil, Light, Moderate, Severe) per PANS-ATM (Doc 4444)  
    /// Returns None if status bit (bit 4) is 0.
    pub wind_shear: Option<Level>,

    #[deku(reader = "read_level(deku::reader)")]
    /// Microburst (bits 7-9): Per ICAO Doc 9871 Table A-2-69  
    /// Severity level (Nil, Light, Moderate, Severe) per PANS-ATM (Doc 4444)  
    /// Returns None if status bit (bit 7) is 0.
    pub microburst: Option<Level>,

    #[deku(reader = "read_level(deku::reader)")]
    /// Icing (bits 10-12): Per ICAO Doc 9871 Table A-2-69  
    /// Severity level (Nil, Light, Moderate, Severe) per PANS-ATM (Doc 4444)  
    /// Returns None if status bit (bit 10) is 0.
    pub icing: Option<Level>,

    #[deku(reader = "read_level(deku::reader)")]
    /// Wake Vortex (bits 13-15): Per ICAO Doc 9871 Table A-2-69  
    /// Severity level (Nil, Light, Moderate, Severe) per PANS-ATM (Doc 4444)  
    /// Returns None if status bit (bit 13) is 0.
    pub wake_vortex: Option<Level>,

    #[deku(reader = "read_temperature(deku::reader)")]
    /// Static Air Temperature (bits 16-26): Per ICAO Doc 9871 Table A-2-69  
    /// 10-bit signed temperature (LSB=0.25°C, range [-128, +128]°C)  
    /// Two's complement encoding. Returns None if status bit (bit 16) is 0.
    pub static_temperature: Option<f64>,

    #[deku(reader = "read_pressure(deku::reader)")]
    /// Average Static Pressure (bits 27-38): Per ICAO Doc 9871 Table A-2-69  
    /// 11-bit pressure in hPa (LSB=1 hPa, direct encoding, range [0, 2048] hPa)  
    /// Returns None if status bit (bit 27) is 0.
    pub static_pressure: Option<u32>,

    #[deku(reader = "read_height(deku::reader)")]
    /// Radio Height (bits 39-51): Per ICAO Doc 9871 Table A-2-69  
    /// 12-bit radio altitude above ground (LSB=16 ft, range [0, 65,528] ft)  
    /// Returns None if status bit (bit 39) is 0.
    pub radio_height: Option<u32>,

    #[deku(bits = "5", map = "fail_if_not_zero")]
    #[serde(skip)]
    /// Reserved (bits 52-56): Must be all zeros
    pub reserved: u8,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Default)]
pub enum Level {
    #[default]
    Nil,
    Light,
    Moderate,
    Severe,
}

fn read_level<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<Level>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u8::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(2)),
    )?;

    trace!("Reading status {} value {}", status, value);

    match (status, value) {
        (true, 0) => Ok(Some(Level::Nil)),
        (true, 1) => Ok(Some(Level::Light)),
        (true, 2) => Ok(Some(Level::Moderate)),
        (true, 3) => Ok(Some(Level::Severe)),
        (true, _) => unreachable!(),
        (false, 0) => Ok(None),
        (false, _) => Err(DekuError::Assertion("Invalid level value".into())),
    }
}

fn read_temperature<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<f64>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let sign = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u16::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(9)),
    )?;

    let temperature = match sign {
        true => (value as f64 - 512.) * 0.25,
        false => value as f64 * 0.25,
    };

    trace!(
        "Reading temperature status {} value {}",
        status,
        temperature
    );

    let msg = "Invalid temperature value {}°C outside [-80, 60]";
    match (status, value, temperature) {
        (true, _, temperature) if (-80. ..=60.).contains(&temperature) => {
            Ok(Some(temperature))
        }
        (true, _, _) => Err(DekuError::Assertion(msg.into())),
        //(false, _) => Ok(None),
        // In practice, I see quite some pressure fields with invalid status but non zero values
        (false, 0, _) => Ok(None),
        (false, _, _) => {
            Err(DekuError::Assertion("Invalid temperature value".into()))
        }
    }
}

fn read_pressure<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<u32>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u32::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(11)),
    )?;

    trace!("Reading pressure status {} value {}", status, value);

    // No scaling formula needed (unlike BDS 4,0 which uses 800 + value * 0.1)
    match (status, value) {
        (true, value) => Ok(Some(value)),
        //(false, _) => Ok(None),
        // In practice, I see quite some pressure fields with invalid status but non zero values
        (false, 0) => Ok(None),
        (false, _) => {
            Err(DekuError::Assertion("Invalid pressure value".into()))
        }
    }
}

fn read_height<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<Option<u32>, DekuError> {
    let status = bool::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(1)),
    )?;
    let value = u32::from_reader_with_ctx(
        reader,
        (deku::ctx::Endian::Big, deku::ctx::BitSize(12)),
    )?;

    trace!("Reading height status {} value {}", status, value);

    match (status, value) {
        (true, value) => Ok(Some(value * 16)),
        (false, 0) => Ok(None),
        (false, _) => Err(DekuError::Assertion("Invalid height value".into())),
    }
}

fn fail_if_not_zero(value: u8) -> Result<u8, DekuError> {
    if value == 0 {
        Ok(value)
    } else {
        Err(DekuError::Assertion("Reserved bits must be zero".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_valid_bds45() {
        let bytes = hex!("a00004190001fb80000000000000");
        // let bytes = hex!("a00005b30001f940000000000000");
        let msg = Message::from_bytes((&bytes, 0)).unwrap().1;
        if let CommBAltitudeReply { bds, .. } = msg.df {
            let MeteorologicalHazardReport {
                turbulence,
                wind_shear,
                icing,
                wake_vortex,
                static_temperature,
                static_pressure,
                radio_height,
                ..
            } = bds.bds45.unwrap();
            assert_eq!(turbulence, None);
            assert_eq!(wind_shear, None);
            assert_eq!(icing, None);
            assert_eq!(wake_vortex, None);
            assert_eq!(static_temperature, Some(-4.5));
            assert_eq!(static_pressure, None);
            assert_eq!(radio_height, None);
        } else {
            unreachable!();
        }
    }
}
