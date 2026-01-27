use deku::prelude::*;
use serde::{Deserialize, Serialize};

/**
 * ## Data Link Capability Report (BDS 1,0)
 *
 * Comm-B message reporting Mode S transponder/data link capabilities.  
 * Per ICAO Doc 9871 Table A-2-16: BDS code 1,0 — Data link capability report
 *
 * Purpose: To report the data link capability of the Mode S transponder/data
 * link installation to ground systems.
 *
 * Message Structure (56 bits):
 * | BDS | CON | RES | OCC | ACAS | SUBNET | LEV5 | MSS | UEL | DEL | ID | SQU | SIC | GIC | HYB | RA | RTCA | DTE    |
 * |-----|-----|-----|-----|------|--------|------|-----|-----|-----|----|----|-----|-----|-----|----|----|--------|
 * | 8   | 1   | 5   | 1   | 1    | 7      | 1    | 1   | 3   | 4   | 1  | 1  | 1   | 1   | 1   | 1  | 2  | 16     |
 *
 * Field Encoding per ICAO Doc 9871 & Annex 10, Vol IV, §3.1.2.6.10.2:
 *
 * **BDS Code** (bits 1-8): Fixed value 0x10 (0001 0000 binary = 1,0 hex)
 *
 * **Continuation Flag** (bit 9):
 *   - 0 = no continuation registers
 *   - 1 = extract next register (1,1 then 1,2, etc. up to 1,6)
 *   - Used to determine extent of continuation into registers 1,1 to 1,6
 *   - If bit 9 = 1 in register 1,6, this is an error condition
 *
 * **Reserved** (bits 10-14): Must be 0
 *
 * **Overlay Command Capability (OCC)** (bit 15):
 *   - 0 = does not support BDS overlay (Data Parity)
 *   - 1 = supports BDS overlay capability
 *
 * **ACAS Operating** (bit 16): Reserved for ACAS
 *   - 0 = ACAS not operating
 *   - 1 = ACAS operating
 *
 * **Mode S Subnetwork Version Number** (bits 17-23): 7-bit version field
 *   - 0 = Mode S subnetwork not available
 *   - 1 = ICAO Doc 9688 (1996)
 *   - 2 = ICAO Doc 9688 (1998)
 *   - 3 = ICAO Annex 10, Vol III, Amdt 77
 *   - 4 = ICAO Doc 9871 (Ed 1), RTCA DO-181D, EUROCAE ED-73C
 *   - 5 = ICAO Doc 9871 (Ed 2), RTCA DO-181E, EUROCAE ED-73E
 *   - 6-127 = Reserved for future use
 *
 * **Transponder Enhanced Protocol Indicator** (bit 24):
 *   - 0 = Level 2 to 4 transponder
 *   - 1 = Level 5 transponder
 *
 * **Mode S Specific Services Capability** (bit 25):
 *   - 0 = no Mode S specific services supported
 *   - 1 = at least one Mode S specific service supported
 *   - When set to 1, particular capability reports shall be checked
 *   - Note: Registers 0,2; 0,3; 0,4; 1,0; 1,7-1,C; 2,0; 3,0 don't affect bit 25
 *
 * **Uplink ELM Average Throughput Capability** (bits 26-28):
 *   - 0 = No UELM Capability
 *   - 1 = 16 UELM segments in 1 second
 *   - 2 = 16 UELM segments in 500 ms
 *   - 3 = 16 UELM segments in 250 ms
 *   - 4 = 16 UELM segments in 125 ms
 *   - 5 = 16 UELM segments in 60 ms
 *   - 6 = 16 UELM segments in 30 ms
 *   - 7 = Reserved
 *
 * **Downlink ELM Throughput Capability** (bits 29-32):
 *   - Maximum number of ELM segments transponder can deliver
 *   - In response to single requesting interrogation (UF = 24)
 *
 * **Aircraft Identification Capability** (bit 33):
 *   - 0 = identification (callsign) not available
 *   - 1 = identification capability available
 *   - Set by transponder if data comes through separate interface (not ADLP)
 *
 * **Squitter Capability Subfield (SCS)** (bit 34):
 *   - 0 = squitter not operational
 *   - 1 = squitter operational (both BDS 0,5 and 0,6 updated in past 9-11 sec)
 *   - Registers 0,5 and 0,6 = Extended Squitter airborne/surface position
 *
 * **Surveillance Identifier Code (SIC)** (bit 35):
 *   - 0 = no surveillance identifier code capability
 *   - 1 = surveillance identifier code capability
 *
 * **Common Usage GICB Capability Report** (bit 36):
 *   - Toggled each time register 1,7 (common usage GICB capability) changes
 *   - Register 1,7 sampled at ~1 minute intervals to check for changes
 *
 * **ACAS Hybrid Surveillance** (bit 37): Reserved for ACAS
 *   - 0 = hybrid surveillance not fitted or not operational
 *   - 1 = hybrid surveillance fitted and operational
 *
 * **ACAS Resolution Advisory (RA)** (bit 38): Reserved for ACAS
 *   - 0 = ACAS generating TAs only
 *   - 1 = ACAS generating both TAs and RAs
 *
 * **ACAS RTCA Version** (bits 39-40): 2-bit RTCA standard version
 *   - 0 = RTCA/DO-185
 *   - 1 = RTCA/DO-185-A
 *   - 2 = RTCA/DO-185-B
 *   - 3 = Reserved
 *
 * **Data Terminal Equipment (DTE) Status** (bits 41-56):
 *   - 16-bit array indicating support status of DTE sub-addresses 0-15
 *   - Each bit (MSB to LSB) represents DTE subaddress 0 to 15
 *   - On-board DTE status sampled at ~1 minute intervals
 *
 * Update Policy per ICAO Doc 9871:
 * - Transponder may update bits 1-8, 16, 33, 35, 37-40 independent of ADLP
 * - These bits provided by transponder when capability report broadcast
 *   results from transponder-detected change in ADLP-reported capability
 *
 * Additional implementation guidelines: ICAO Annex 10, Vol IV, §4.3.8.4.2.2.2
 */

#[derive(
    Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone, Default,
)]
#[serde(tag = "bds", rename = "10")]
pub struct DataLinkCapability {
    /// BDS Code (bits 1-8): Per ICAO Doc 9871 Table A-2-16  
    /// Fixed identifier for this register.  
    /// Must be 0x10 (0001 0000 binary = 1,0 hexadecimal).
    #[deku(bits = "8", map = "fail_if_not10")]
    #[serde(skip)]
    pub bds: u8,

    /// Continuation Flag (bit 9): Per ICAO Doc 9871 Table A-2-16  
    /// Indicates if continuation registers (1,1 to 1,6) should be extracted.  
    /// Encoding:
    ///   - 0 = no continuation registers
    ///   - 1 = extract next register in sequence
    ///
    /// If bit 9 = 1 in register 1,6, this is an error condition.
    #[deku(bits = "1")]
    pub config: bool,

    /// Reserved (bits 10-14): Per ICAO Doc 9871 Table A-2-16  
    /// Must be 0.
    #[deku(bits = "5", map = "fail_if_not0")]
    #[serde(skip)]
    pub reserved: u8,

    /// Overlay Command Capability (bit 15): Per ICAO Doc 9871 Table A-2-16  
    /// Indicates BDS overlay (Data Parity) support.  
    /// Encoding:
    ///   - 0 = does not support BDS overlay
    ///   - 1 = supports BDS overlay capability
    #[deku(bits = "1")]
    pub ovc: bool,

    /// ACAS Operating (bit 16): Per ICAO Doc 9871 Table A-2-16  
    /// Reserved for ACAS. Indicates if ACAS is operating.  
    /// Encoding:
    ///   - 0 = ACAS not operating
    ///   - 1 = ACAS operating
    #[deku(bits = "1")]
    pub acas: bool,

    /// Mode S Subnetwork Version Number (bits 17-23): Per ICAO Doc 9871 Table A-2-16  
    /// 7-bit field indicating Mode S subnetwork version.  
    /// Encoding:
    ///   - 0 = Subnetwork not available
    ///   - 1 = ICAO Doc 9688 (1996)
    ///   - 2 = ICAO Doc 9688 (1998)
    ///   - 3 = ICAO Annex 10, Vol III, Amdt 77
    ///   - 4 = ICAO Doc 9871 (Ed 1), RTCA DO-181D, EUROCAE ED-73C
    ///   - 5 = ICAO Doc 9871 (Ed 2), RTCA DO-181E, EUROCAE ED-73E
    ///   - >5 = Reserved for future use
    #[deku(bits = "7")]
    pub subnet: u8,

    /// Transponder Enhanced Protocol Indicator (bit 24): Per ICAO Doc 9871 Table A-2-16  
    /// Indicates transponder level/capability.  
    /// Encoding:
    ///   - 0 = Level 2 to 4 transponder
    ///   - 1 = Level 5 transponder (enhanced protocol)
    #[deku(bits = "1")]
    pub level5: bool,

    /// Mode S Specific Services Capability (bit 25): Per ICAO Doc 9871 Table A-2-16  
    /// Indicates support for Mode S specific services.  
    /// Encoding:
    ///   - 0 = no Mode S specific services supported
    ///   - 1 = at least one Mode S specific service supported
    ///
    /// When set to 1, particular capability reports shall be checked.  
    /// Note: Registers 0,2; 0,3; 0,4; 1,0; 1,7-1,C; 2,0; 3,0 don't affect this bit.
    #[deku(bits = "1")]
    pub mode_s: bool,

    /// Uplink ELM Average Throughput Capability (bits 26-28): Per ICAO Doc 9871 Table A-2-16  
    /// 3-bit field indicating uplink ELM throughput.  
    /// Encoding:
    ///   - 0 = No UELM Capability
    ///   - 1 = 16 UELM segments in 1 second
    ///   - 2 = 16 UELM segments in 500 ms
    ///   - 3 = 16 UELM segments in 250 ms
    ///   - 4 = 16 UELM segments in 125 ms
    ///   - 5 = 16 UELM segments in 60 ms
    ///   - 6 = 16 UELM segments in 30 ms
    ///   - 7 = Reserved
    #[deku(bits = "3")]
    #[serde(skip)]
    pub uplink: u8,

    /// Downlink ELM Throughput Capability (bits 29-32): Per ICAO Doc 9871 Table A-2-16  
    /// 4-bit field indicating maximum number of ELM segments  
    /// transponder can deliver in response to single interrogation (UF=24).
    #[deku(bits = "4")]
    #[serde(skip)]
    pub downlink: u8,

    /// Aircraft Identification Capability (bit 33): Per ICAO Doc 9871 Table A-2-16  
    /// Indicates availability of aircraft identification (callsign).  
    /// Encoding:
    ///   - 0 = identification not available
    ///   - 1 = identification capability available
    ///
    /// Set by transponder if data comes through separate interface (not ADLP).
    #[deku(bits = "1")]
    pub identification: bool,

    /// Squitter Capability Subfield (bit 34): Per ICAO Doc 9871 Table A-2-16  
    /// Indicates if Extended Squitter is operational.  
    /// Encoding:
    ///   - 0 = squitter not operational
    ///   - 1 = both BDS 0,5 and 0,6 updated in past 9-11 seconds
    ///
    /// Registers 0,5 and 0,6 = Extended Squitter airborne/surface position.
    #[deku(bits = "1")]
    pub squitter: bool,

    /// Surveillance Identifier Code (bit 35): Per ICAO Doc 9871 Table A-2-16  
    /// Indicates SIC capability.  
    /// Encoding:
    ///   - 0 = no surveillance identifier code capability
    ///   - 1 = surveillance identifier code capability
    #[deku(bits = "1")]
    pub sic: bool,

    /// Common Usage GICB Capability Report (bit 36): Per ICAO Doc 9871 Table A-2-16  
    /// Toggled each time register 1,7 (common usage GICB capability) changes.  
    /// Register 1,7 sampled at approximately 1 minute intervals.
    #[deku(bits = "1")]
    pub gicb: bool,

    /// ACAS Hybrid Surveillance (bit 37): Per ICAO Doc 9871 Table A-2-16  
    /// Reserved for ACAS. Indicates hybrid surveillance status.  
    /// Encoding:
    ///   - 0 = hybrid surveillance not fitted or not operational
    ///   - 1 = hybrid surveillance fitted and operational
    #[deku(bits = "1")]
    pub acas_hybrid: bool,

    /// ACAS Resolution Advisory (bit 38): Per ICAO Doc 9871 Table A-2-16  
    /// Reserved for ACAS. Indicates RA capability.  
    /// Encoding:
    ///   - 0 = ACAS generating TAs only
    ///   - 1 = ACAS generating both TAs and RAs
    #[deku(bits = "1")]
    pub acas_ra: bool,

    /// ACAS RTCA Version (bits 39-40): Per ICAO Doc 9871 Table A-2-16  
    /// 2-bit field indicating RTCA standard version.  
    /// Encoding:
    ///   - 0 = RTCA/DO-185
    ///   - 1 = RTCA/DO-185-A
    ///   - 2 = RTCA/DO-185-B
    ///   - 3 = Reserved
    #[deku(bits = "2")]
    #[serde(skip)]
    pub acas_rtca: u8,

    /// Data Terminal Equipment Status (bits 41-56): Per ICAO Doc 9871 Table A-2-16  
    /// 16-bit array indicating support status of DTE sub-addresses 0-15.  
    /// Each bit (MSB to LSB) represents DTE subaddress 0 to 15.  
    /// On-board DTE status sampled at approximately 1 minute intervals.
    #[deku(bits = "16")]
    pub dte: u16,
}

fn fail_if_not0(value: u8) -> Result<u8, DekuError> {
    if value == 0 {
        Ok(value)
    } else {
        Err(DekuError::Assertion(
            "Reserved bits must be 0 in BDS 1,0".into(),
        ))
    }
}
fn fail_if_not10(value: u8) -> Result<u8, DekuError> {
    if value == 0x10 {
        Ok(value)
    } else {
        Err(DekuError::Assertion(
            "First bits must be 0x10 in BDS 1,0".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_valid_bds10() {
        let bytes = hex!("a800178d10010080f50000d5893c");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBIdentityReply { bds, .. } = msg.df {
            assert_eq!(
                bds.bds10,
                Some(DataLinkCapability {
                    bds: 16,
                    config: false,
                    reserved: 0,
                    ovc: false,
                    acas: true,
                    subnet: 0,
                    level5: false,
                    mode_s: true,
                    uplink: 0,
                    downlink: 0,
                    identification: true,
                    squitter: true,
                    sic: true,
                    gicb: true,
                    acas_hybrid: false,
                    acas_ra: true,
                    acas_rtca: 1,
                    dte: 0
                })
            );
        } else {
            unreachable!();
        }
    }
    #[test]
    fn test_invalid_bds10() {
        let bytes = hex!("a0001838201584f23468207cdfa5");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let CommBAltitudeReply { bds, .. } = msg.df {
            assert_eq!(bds.bds10, None);
        } else {
            unreachable!();
        }
    }
}
