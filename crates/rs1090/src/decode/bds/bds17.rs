use deku::prelude::*;
use serde::{Deserialize, Serialize};

/**
 * ## Common Usage GICB Capability Report (BDS 1,7)
 *
 * Comm-B message indicating which GICB (Ground-Initiated Comm-B) services
 * are currently supported and available.  
 * Per ICAO Doc 9871 Table A-2-23: BDS code 1,7 — Common usage GICB capability report
 *
 * Purpose: To indicate common usage GICB services currently supported by
 * the aircraft installation.
 *
 * Message Structure (56 bits): 24 capability flags + 5 reserved + 27 zeros
 *
 * Capability Flags (bits 1-24): Each bit indicates register availability
 * - Bit 1: BDS 0,5 (Extended Squitter airborne position)
 * - Bit 2: BDS 0,6 (Extended Squitter surface position)
 * - Bit 3: BDS 0,7 (Extended Squitter status)
 * - Bit 4: BDS 0,8 (Extended Squitter identification and category)
 * - Bit 5: BDS 0,9 (Extended Squitter airborne velocity)
 * - Bit 6: BDS 0,A (Extended Squitter event-driven information)
 * - Bit 7: BDS 2,0 (Aircraft identification) - ALWAYS 1
 * - Bit 8: BDS 2,1 (Aircraft registration number)
 * - Bit 9: BDS 4,0 (Selected vertical intention)
 * - Bit 10: BDS 4,1 (Next waypoint identifier)
 * - Bit 11: BDS 4,2 (Next waypoint position)
 * - Bit 12: BDS 4,3 (Next waypoint information)
 * - Bit 13: BDS 4,4 (Meteorological routine report)
 * - Bit 14: BDS 4,5 (Meteorological hazard report)
 * - Bit 15: BDS 4,8 (VHF channel report)
 * - Bit 16: BDS 5,0 (Track and turn report)
 * - Bit 17: BDS 5,1 (Position coarse)
 * - Bit 18: BDS 5,2 (Position fine)
 * - Bit 19: BDS 5,3 (Air-referenced state vector)
 * - Bit 20: BDS 5,4 (Waypoint 1)
 * - Bit 21: BDS 5,5 (Waypoint 2)
 * - Bit 22: BDS 5,6 (Waypoint 3)
 * - Bit 23: BDS 5,F (Quasi-static parameter monitoring)
 * - Bit 24: BDS 6,0 (Heading and speed report)
 *
 * Reserved (bits 25-29): Reserved for aircraft capability, E,1, E,2, F,1
 *
 * Validation (bits 30-56): Must all be ZERO (27 bits)
 *
 * Encoding Rules per ICAO Doc 9871:
 * 1. Each bit set to 1 indicates associated register available in installation
 * 2. All registers constantly monitored at rate consistent with update rate
 * 3. Bit set to 1 only when valid data input at required rate or above
 * 4. Bit set to 1 if at least one field receives valid data at required rate
 *    (status bits for other fields set to 0)
 * 5. Bit 6 (BDS 0,A) set to 1 upon first loading, remains until power off
 *    or ADS-B transmission terminated
 * 6. Bits 17-18 (BDS 5,1/5,2) only set to 1 if STATUS bits in those
 *    registers are set to 1
 * 7. Bit 7 (BDS 2,0) is always 1 (aircraft identification always valid)
 *
 * Register Relationships:
 * - This register (1,7) is toggled in BDS 1,0 bit 36 when changed
 * - Sampled at approximately 1 minute intervals to check for changes
 * - Registers 1,8 to 1,C are independent of this register
 *
 * Note: A bit being set indicates the data is currently available at the
 * required update rate. The same aircraft may respond with different GICB
 * capability reports depending on data availability.
 *
 * Reference: ICAO Doc 9871 Table A-2-23
 */

#[derive(
    Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone, Default,
)]
#[serde(tag = "bds", rename = "17")]
pub struct CommonUsageGICBCapabilityReport {
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Extended squitter airborne position
    pub bds05: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Extended squitter surface position
    pub bds06: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Extended squitter status
    pub bds07: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Extended squitter identification and category
    pub bds08: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Extended squitter airborne velocity information
    pub bds09: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Extended squitter event-driven information
    pub bds0a: bool,

    #[deku(bits = "1", map = "fail_if_false")]
    #[serde(skip_serializing_if = "is_false")]
    /// Aircraft identification
    pub bds20: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Aircraft registration number
    pub bds21: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Selected vertical intention
    pub bds40: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Next waypoint identifier
    pub bds41: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Next waypoint position
    pub bds42: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Next waypoint information
    pub bds43: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Meteorological routine report
    pub bds44: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Meteorological hazard report
    pub bds45: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// VHF channel report
    pub bds48: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Track and turn report
    pub bds50: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Position coarse
    pub bds51: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Position fine
    pub bds52: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Air-referenced state vector
    pub bds53: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Waypoint 1
    pub bds54: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Waypoint 2
    pub bds55: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Waypoint 3
    pub bds56: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Quasi-static parameter monitoring
    pub bds5f: bool,

    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    /// Heading and speed report
    pub bds60: bool,

    #[deku(bits = "5")]
    #[serde(skip)]
    pub reserved: u8,

    #[deku(reader = "check_zeros(deku::reader)")]
    #[serde(skip)]
    pub check_flag: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn fail_if_false(value: bool) -> Result<bool, DekuError> {
    if value {
        Ok(value)
    } else {
        Err(DekuError::Assertion(
            "BDS 2,0 is always valid in BDS 1,7".into(),
        ))
    }
}

fn check_zeros<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
    reader: &mut Reader<R>,
) -> Result<bool, DekuError> {
    for i in 0..=3 {
        let value = u8::from_reader_with_ctx(
            reader,
            (
                deku::ctx::Endian::Big,
                deku::ctx::BitSize(if i == 0 { 3 } else { 8 }),
            ),
        )?;
        if value != 0 {
            return Err(DekuError::InvalidParam(
                "BDS 1,7 must have all final bits set to 0".into(),
            ));
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_valid_bds17() {
        let bytes = hex!("a0000638fa81c10000000081a92f");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            assert_eq!(
                bds.bds17,
                Some(CommonUsageGICBCapabilityReport {
                    bds05: true,
                    bds06: true,
                    bds07: true,
                    bds08: true,
                    bds09: true,
                    bds0a: false,
                    bds20: true,
                    bds21: false,
                    bds40: true,
                    bds41: false,
                    bds42: false,
                    bds43: false,
                    bds44: false,
                    bds45: false,
                    bds48: false,
                    bds50: true,
                    bds51: true,
                    bds52: true,
                    bds53: false,
                    bds54: false,
                    bds55: false,
                    bds56: false,
                    bds5f: false,
                    bds60: true,
                    reserved: 0,
                    check_flag: true
                })
            );
        } else {
            unreachable!();
        }
    }
    #[test]
    fn test_invalid_bds17() {
        let bytes = hex!("a0001838201584f23468207cdfa5");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            assert_eq!(bds.bds17, None);
        } else {
            unreachable!();
        }
    }
}
