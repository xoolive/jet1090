use super::bds08;
use deku::prelude::*;
use serde::{Deserialize, Serialize};

/**
 * ## Aircraft Identification (BDS 2,0)
 *
 * Comm-B message providing aircraft callsign/identification.  
 * Per ICAO Doc 9871 Table A-2-32: BDS code 2,0 — Aircraft identification
 *
 * Purpose: To report aircraft identification to the ground.
 *
 * Message Structure (56 bits):
 * | BDS  | CHAR1 | CHAR2 | CHAR3 | CHAR4 | CHAR5 | CHAR6 | CHAR7 | CHAR8 |
 * |------|-------|-------|-------|-------|-------|-------|-------|-------|
 * | 8    | 6     | 6     | 6     | 6     | 6     | 6     | 6     | 6     |
 *
 * Field Encoding per ICAO Doc 9871:
 *
 * **BDS Code** (bits 1-8):
 *   - Fixed value: 0x20 (0010 0000 binary = 2,0 hexadecimal)
 *   - Identifies this register as BDS 2,0
 *
 * **Aircraft Identification** (bits 9-56):
 *   - 8 characters, 6 bits each
 *   - Character encoding per ICAO Annex 10, Vol IV, Table 3-7
 *   - Same encoding as used in Extended Squitter BDS 0,8
 *   - Valid characters: A-Z, 0-9, and space
 *   - Character set (6-bit encoding):
 *     * 000000 = (no character/space)
 *     * 000001-011010 = A-Z
 *     * 110000-111001 = 0-9
 *     * 100000 = space
 *
 * Content Rules per ICAO Doc 9871:
 * - The aircraft identification shall be that employed in the flight plan
 * - When no flight plan is available, the registration marking shall be used
 * - Characters 1-8 are also used by the Extended Squitter application (BDS 0,8)
 * - Data may be input from sources other than Mode S ADLP
 *
 * Capability Indication:
 * - Support indicated by bit 33 in register 1,0 (0x10)
 * - Also indicated in registers 1,7 (0x17) and 1,8 (0x18)
 *
 * Note: This is the Comm-B version of aircraft identification.
 * The Extended Squitter version is BDS 0,8 with identical character encoding.
 *
 * Additional implementation guidelines: ICAO Doc 9871 §D.2.4.3
 */

#[derive(
    Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone, Default,
)]
#[serde(tag = "bds", rename = "20")]
pub struct AircraftIdentification {
    /// BDS Code (bits 1-8): Per ICAO Doc 9871 Table A-2-32  
    /// Fixed identifier for this register.  
    /// Must be 0x20 (0010 0000 binary = 2,0 hexadecimal).
    #[deku(bits = "8", map = "fail_if_not20")]
    #[serde(skip)]
    pub bds: u8,

    /// Aircraft Identification (bits 9-56): Per ICAO Doc 9871 Table A-2-32  
    /// Callsign or registration marking (8 characters, 6 bits each).  
    /// Character encoding per ICAO Annex 10, Vol IV, Table 3-7.  
    /// Valid characters: A-Z (1-26), 0-9 (48-57), space (32).  
    /// Content:
    ///   - Aircraft identification from flight plan (when available)
    ///   - Aircraft registration marking (when no flight plan)
    ///
    /// Same encoding as Extended Squitter BDS 0,8.
    #[deku(reader = "bds08::callsign_read(deku::reader)")]
    pub callsign: String,
}

fn fail_if_not20(value: u8) -> Result<u8, DekuError> {
    if value == 0x20 {
        Ok(value)
    } else {
        Err(DekuError::Assertion(
            "First bits must be 0x20 in BDS 2,0".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_valid_bds20() {
        let bytes = hex!("a0001838201584f23468207cdfa5");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            assert_eq!(
                bds.bds20,
                Some(super::AircraftIdentification {
                    bds: 32,
                    callsign: "EXS2MF".to_string()
                })
            );
        } else {
            unreachable!();
        }
    }
    #[test]
    fn test_invalid_bds20() {
        let bytes = hex!("a800178d10010080f50000d5893c");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBIdentityReply { bds, .. } = msg.df {
            assert_eq!(bds.bds20, None);
        } else {
            unreachable!();
        }
    }
}
