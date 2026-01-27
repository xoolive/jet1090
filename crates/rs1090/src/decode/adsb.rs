use super::bds::{bds05, bds06, bds08, bds09, bds61, bds62, bds65};
use super::{Capability, ICAO};
use deku::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;

/**
 * An ADS-B frame is 112 bits long.
 *
 * It consists of five main parts, shown as follows:
 *
 * | DF  | CA  | ICAO | ME  | PI  |
 * | --- | --- | ---- | --- | --- |
 * | 5   | 3   | 24   | 56  | 24  |
 *
 */

#[derive(Debug, PartialEq, DekuRead, Clone, Serialize, Deserialize)]
pub struct ADSB {
    /// The transponder capability
    #[serde(skip)]
    pub capability: Capability,

    /// The ICAO aircraft address on 24 bytes
    pub icao24: ICAO,

    /// The message, prefixed by a typecode on 5 bytes
    #[serde(flatten)]
    pub message: ME,

    /// Parity/Interrogator ID
    #[serde(skip, default = "crate::decode::serde_default_icao_empty")]
    pub parity: ICAO,
}

impl fmt::Display for ADSB {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, " DF17. Extended Squitter")?;
        writeln!(f, "  Address:       {}", &self.icao24)?;
        writeln!(f, "  Air/Ground:    {}", &self.capability)?;
        write!(f, "{}", &self.message)
    }
}

/**
* The first 5 bytes of the Message Field [`ME`] encode the typecode.
*
* It is used to identify which kind of data is encode in the following bytes.
*
* | Typecode | Name                                              |
* | -------- | ------------------------------------------------- |
* | 0        | [`ME::NoPosition`]                                |
* | 1..=4    | [`bds08::AircraftIdentification`]                 |
* | 5..=8    | [`bds06::SurfacePosition`]                        |
* | 9..=18   | [`bds05::AirbornePosition`] (barometric altitude) |
* | 19       | [`bds09::AirborneVelocity`]                       |
* | 20..=22  | [`bds05::AirbornePosition`] (GNSS height)         |
* | 23       | [`ME::Reserved0`]                                 |
* | 24       | [`ME::SurfaceSystemStatus`]                       |
* | 25..=27  | [`ME::Reserved1`]                                 |
* | 28       | [`bds61::AircraftStatus`]                         |
* | 29       | [`bds62::TargetStateAndStatusInformation`]        |
* | 30       | [`ME::AircraftOperationalCoordination`]           |
* | 31       | [`bds65::AircraftOperationStatus`]                |
*/

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
pub struct Unused {
    #[deku(skip, pad_bits_after = "48", default = "true")]
    #[serde(skip)]
    unused: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
#[deku(id_type = "u8", bits = "5")]
//#[serde(untagged)]
#[serde(tag = "bds")]
pub enum ME {
    #[deku(id = "0")]
    NoPosition(Unused),

    #[deku(id_pat = "1..=4")]
    #[serde(rename = "08")]
    BDS08 {
        #[serde(skip, default = "u8::default")]
        id: u8,
        #[serde(flatten)]
        #[deku(ctx = "*id")]
        inner: bds08::AircraftIdentification,
    },

    #[deku(id_pat = "5..=8")]
    #[serde(rename = "06")]
    BDS06 {
        tc: u8,
        #[serde(flatten)]
        #[deku(ctx = "*tc")]
        inner: bds06::SurfacePosition,
    },

    #[deku(id_pat = "9..=18 | 20..=22")]
    #[serde(rename = "05")]
    BDS05 {
        tc: u8,
        #[serde(flatten)]
        #[deku(ctx = "*tc")]
        inner: bds05::AirbornePosition,
    },

    #[deku(id = "19")]
    #[serde(rename = "09")]
    BDS09(bds09::AirborneVelocity),

    #[deku(id = "23")]
    #[serde(rename = "id23")]
    Reserved0(Unused),

    #[deku(id = "24")]
    #[serde(rename = "id24")]
    SurfaceSystemStatus(Unused),

    #[deku(id_pat = "25..=27")]
    #[serde(rename = "id25_27")]
    Reserved1 { _id: u8, unused: u8 },

    #[deku(id = "28")]
    #[serde(rename = "61")]
    BDS61(bds61::AircraftStatus),

    #[deku(id = "29")]
    #[serde(rename = "62")]
    BDS62(bds62::TargetStateAndStatusInformation),

    #[deku(id = "30")]
    #[serde(rename = "id30")]
    AircraftOperationalCoordination(Unused),

    #[deku(id = "31")]
    #[serde(rename = "65")]
    BDS65(bds65::AircraftOperationStatus),
}

impl fmt::Display for ME {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ME::NoPosition { .. }
            | ME::Reserved0 { .. }
            | ME::Reserved1 { .. }
            | ME::SurfaceSystemStatus { .. }
            | ME::AircraftOperationalCoordination { .. } => Ok(()),
            ME::BDS05 { inner, .. } => {
                write!(f, "{inner}")
            }
            ME::BDS06 { inner, .. } => {
                write!(f, "{inner}")
            }
            ME::BDS08 { inner, .. } => {
                write!(f, "{inner}")
            }
            ME::BDS09(me) => {
                write!(f, "{me}")
            }
            ME::BDS61(me) => {
                write!(f, "{me}")
            }
            ME::BDS62(me) => {
                write!(f, "{me}")
            }
            ME::BDS65(me) => {
                write!(f, "{me}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use hexlit::hex;

    #[test]
    fn test_icao24() {
        let bytes = hex!("8D406B902015A678D4D220AA4BDA");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let ExtendedSquitterADSB(msg) = msg.df {
            assert_eq!(format!("{}", msg.icao24), "406b90");
            return;
        }
        unreachable!();
    }
}
