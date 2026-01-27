use deku::prelude::*;
use serde::{Deserialize, Serialize};

/**
 * ## Mode S Specific Services GICB Capability Report Part 2 (BDS 1,9)
 *
 * Comm-B message indicating Mode S specific GICB service capabilities (Part 2 of 5).  
 * Per ICAO Doc 9871 Table A-2-25: BDS code 1,9 — Mode S specific services GICB capability report (2 of 5)
 *
 * Purpose: To indicate Mode S specific services currently supported by the
 * aircraft installation (continuation of BDS 1,8).
 *
 * Message Structure: Similar to BDS 1,7 and 1,8 but covers additional register range
 *
 * Encoding Rules per ICAO Doc 9871:
 * - Each bit indicates availability of corresponding Mode S specific register
 * - Bit set to 1 only when valid data is being input at required rate
 * - Independent of BDS 1,7 (common usage GICB capability)
 * - Part of extended capability reporting system (1,8 through 1,C)
 *
 * Note: Most bits must be 0 as they represent reserved/unused Mode S registers.
 * This register is part of the Mode S specific services capability indication.
 *
 * Reference: ICAO Doc 9871 Table A-2-25, §3.1.2.6.10.2
 */

#[derive(
    Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone, Default,
)]
#[serde(tag = "bds", rename = "19")]
pub struct GICBCapabilityReportPart2 {
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds70: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds6f: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds6e: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds6d: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds6c: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds6b: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds6a: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds69: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds68: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds67: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds66: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds65: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds64: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds63: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds62: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds61: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds60: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds5f: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds5e: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds5d: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds5c: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds5b: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds5a: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds59: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds58: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds57: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds56: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds55: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds54: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds53: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds52: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds51: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds50: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds4f: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds4e: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds4d: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds4c: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds4b: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds4a: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds49: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds48: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds47: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds46: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds45: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds44: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds43: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds42: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds41: bool,
    #[deku(bits = "1")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds40: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds3f: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds3e: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds3d: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds3c: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds3b: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds3a: bool,
    #[deku(bits = "1", map = "fail_if_true")]
    #[serde(skip_serializing_if = "is_false")]
    pub bds39: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn fail_if_true(value: bool) -> Result<bool, DekuError> {
    if value {
        Err(DekuError::Assertion(
            "Field is most probably false in BDS 1,9".into(),
        ))
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_valid_bds19() {
        let bytes = hex!("a00001ba00018003800080000000");
        let msg = Message::from_bytes((&bytes, 0)).unwrap().1;
        if let CommBAltitudeReply { bds, .. } = msg.df {
            assert!(bds.bds19.is_some());
        } else {
            unreachable!();
        }
    }
}
