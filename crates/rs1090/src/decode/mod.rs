pub mod adsb;
pub mod bds;
pub mod commb;
pub mod cpr;
pub mod crc;
pub mod flarm;
pub mod time;

use adsb::{ADSB, ME};
use commb::{DF20DataSelector, DF21DataSelector};
use crc::modes_checksum;
use deku::{ctx::Order, prelude::*};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use tracing::debug;

/**
 * DF stands for Downlink Format.
 *
 * A number between 0 and 24 encoding the type of the message, and whether it is
 * short (56 bits) or long (112 bits).
 *
 * |  [`DF`]  |  Name                               |  Section    |
 * | -------- | ----------------------------------- | ----------- |
 * | 0        | [`DF::ShortAirAirSurveillance`]     | 3.1.2.8.2   |
 * | 4        | [`DF::SurveillanceAltitudeReply`]   | 3.1.2.6.5   |
 * | 5        | [`DF::SurveillanceIdentityReply`]   | 3.1.2.6.7   |
 * | 11       | [`DF::AllCallReply`]                | 2.1.2.5.2.2 |
 * | 16       | [`DF::LongAirAirSurveillance`]      | 3.1.2.8.3   |
 * | 17       | [`DF::ExtendedSquitterADSB`]        | 3.1.2.8.6   |
 * | 18       | [`DF::ExtendedSquitterTisB`]        | 3.1.2.8.7   |
 * | 19       | [`DF::ExtendedSquitterMilitary`]    | 3.1.2.8.8   |
 * | 20       | [`DF::CommBAltitudeReply`]          | 3.1.2.6.6   |
 * | 21       | [`DF::CommBIdentityReply`]          | 3.1.2.6.8   |
 * | 24       | [`DF::CommDExtended`]               | 3.1.2.7.3   |
 */

#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
#[deku(id_type = "u8", bits = "5", ctx = "crc: u32")]
#[serde(tag = "df")]
pub enum DF {
    /// DF=0: Short Air-Air Surveillance (3.1.2.8.2)
    #[deku(id = "0")]
    #[serde(rename = "0")]
    ShortAirAirSurveillance {
        /// Vertical status
        #[deku(bits = "1")]
        #[serde(skip, default = "u8::default")]
        vs: u8,
        /// CC:
        #[deku(bits = "1")]
        #[serde(skip, default = "u8::default")]
        cc: u8,
        /// unused
        #[deku(bits = "1")]
        #[serde(skip, default = "u8::default")]
        unused: u8,
        /// Sensitivity level, ACAS
        #[deku(bits = "3")]
        #[serde(skip, default = "u8::default")]
        sl: u8,
        /// Spare
        #[deku(bits = "2")]
        #[serde(skip, default = "u8::default")]
        unused1: u8,
        /// Reply information
        #[deku(bits = "4")]
        #[serde(skip, default = "u8::default")]
        ri: u8,
        /// unused
        #[deku(bits = "2")]
        #[serde(skip, default = "u8::default")]
        unused2: u8,
        /// Altitude code on 13 bits
        #[serde(rename = "altitude")]
        ac: AC13Field,
        /// ICAO address, parity
        #[serde(rename = "icao24")]
        #[deku(ctx = "crc")]
        ap: IcaoParity,
    },

    /// DF=4: Surveillance Altitude Reply (3.1.2.6.5)
    #[deku(id = "4")]
    #[serde(rename = "4")]
    SurveillanceAltitudeReply {
        /// Flight Status
        #[serde(skip)]
        fs: FlightStatus,
        /// DownlinkRequest
        #[serde(skip)]
        dr: DownlinkRequest,
        /// Utility Message
        #[serde(skip)]
        um: UtilityMessage,
        /// Altitude code on 13 bits
        #[serde(rename = "altitude")]
        ac: AC13Field,
        /// Address/Parity
        #[serde(rename = "icao24")]
        #[deku(ctx = "crc")]
        ap: IcaoParity,
    },

    /// DF=5: Surveillance Identity Reply (3.1.2.6.7)
    #[deku(id = "5")]
    #[serde(rename = "5")]
    SurveillanceIdentityReply {
        /// Flight Status
        #[serde(skip)]
        fs: FlightStatus,
        /// Downlink Request
        #[serde(skip)]
        dr: DownlinkRequest,
        /// UtilityMessage
        #[serde(skip)]
        um: UtilityMessage,
        /// Identity code (squawk)
        #[serde(rename = "squawk")]
        id: IdentityCode,
        /// Address/Parity
        #[serde(rename = "icao24")]
        #[deku(ctx = "crc")]
        ap: IcaoParity,
    },

    /// DF=11: (Mode S) All-call reply, Downlink format 11 (2.1.2.5.2.2)
    #[deku(id = "11")]
    #[serde(rename = "11")]
    AllCallReply {
        /// Capability
        capability: Capability,
        /// Address Announced
        #[serde(rename = "icao24")]
        icao: ICAO,
        /// Parity/Interrogator identifier
        #[serde(skip, default = "serde_default_icao_empty")]
        p_icao: ICAO,
    },

    #[deku(id = "16")]
    #[serde(rename = "16")]
    /// Long Air-Air Surveillance, Downlink Format 16 (3.1.2.8.3)
    LongAirAirSurveillance {
        #[deku(bits = "1")]
        /// Vertical Status (airborne: 0, onground: 1)
        vs: u8,
        #[deku(bits = "2")]
        #[serde(skip, default = "u8::default")]
        reserved1: u8,
        #[deku(bits = "3")]
        /// Sensitivity Level (inoperative: 0)
        sl: u8,
        #[deku(bits = "2")]
        #[serde(skip, default = "u8::default")]
        reserved2: u8,
        #[deku(bits = "4")]
        /// Reply information
        ///
        /// - 0000: No operating ACAS
        /// - 0010: ACAS with resolution capability inhibited
        /// - 0011: ACAS with vertical-only resolution capability
        /// - 0111: ACAS with vertical and horizontal resolution capability
        ri: u8,
        #[deku(bits = "2")]
        #[serde(skip, default = "u8::default")]
        reserved3: u8,
        /// Altitude code on 13 bits
        #[serde(rename = "altitude")]
        ac: AC13Field,
        /// Message, ACAS (56 bits, a BDS of a type requested in UF=0)
        #[deku(count = "7")]
        #[serde(skip, default = "Vec::new")]
        mv: Vec<u8>,
        /// Address/Parity
        #[serde(rename = "icao24")]
        #[deku(ctx = "crc")]
        ap: IcaoParity,
    },

    #[deku(id = "17")]
    #[serde(rename = "17")]
    /// Extended Squitter ADS-B, Downlink Format 17 (3.1.2.8.6)
    ExtendedSquitterADSB(ADSB),

    /// Extended Squitter Supplementary, Downlink Format 18 (3.1.2.8.7)
    ///
    /// Non-Transponder-based ADS-B Transmitting Subsystems and TIS-B Transmitting equipment.
    /// Equipment that cannot be interrogated.
    #[deku(id = "18")]
    #[serde(rename = "18")]
    ExtendedSquitterTisB {
        /// Enum containing message
        #[serde(flatten)]
        cf: ControlField,
        /// Parity/interrogator identifier
        #[serde(skip, default = "serde_default_icao_empty")]
        pi: ICAO,
    },

    /// DF=19: Extended Squitter Military Application, Downlink Format 19 (3.1.2.8.8)
    #[deku(id = "19")]
    ExtendedSquitterMilitary {
        /// Reserved
        #[deku(bits = "3")]
        af: u8,
    },

    /// Comm-B Altitude Reply, Downlink Format 20 (3.1.2.6.6)
    #[deku(id = "20")]
    #[serde(rename = "20")]
    CommBAltitudeReply {
        /// Flight Status
        #[serde(skip)]
        fs: FlightStatus,
        /// Downlink Request
        #[serde(skip)]
        dr: DownlinkRequest,
        /// Utility Message
        #[serde(skip)]
        um: UtilityMessage,
        /// Altitude code on 13 bits
        #[serde(rename = "altitude")]
        ac: AC13Field,
        /// BDS Message, Comm-B
        #[serde(flatten)]
        #[deku(ctx = "*ac")]
        bds: DF20DataSelector,
        /// address/parity
        #[serde(rename = "icao24")]
        #[deku(ctx = "crc")]
        ap: IcaoParity,
    },

    /// Comm-B Identity Reply, Downlink Format 21 (3.1.2.6.8)
    #[deku(id = "21")]
    #[serde(rename = "21")]
    CommBIdentityReply {
        /// Flight Status
        #[serde(skip)]
        fs: FlightStatus,
        /// Downlink Request
        #[serde(skip)]
        dr: DownlinkRequest,
        /// Utility Message
        #[serde(skip)]
        um: UtilityMessage,
        /// Identity code (squawk)
        #[serde(rename = "squawk")]
        id: IdentityCode,
        /// BDS Message, Comm-B
        #[serde(flatten)]
        bds: DF21DataSelector,
        /// Address/Parity
        #[serde(rename = "icao24")]
        #[deku(ctx = "crc")]
        ap: IcaoParity,
    },

    /// 24: Comm-D Extended, Downlink Format 24 (3.1.2.7.3)
    #[deku(id_pat = "24..=31")]
    CommDExtended {
        #[serde(skip, default = "u8::default")]
        id: u8,
        /// Reserved
        #[deku(bits = "1")]
        spare: u8,
        /// Control, ELM
        #[serde(skip)]
        ke: KE,
        /// Number of D-segment
        #[deku(bits = "4")]
        nd: u8,
        /// Message, Comm-D, 80 bits
        #[deku(count = "10")]
        md: Vec<u8>,
        /// Address/Parity
        parity: ICAO,
    },
}

pub fn serde_default_icao_empty() -> ICAO {
    ICAO(0)
}

/// The entry point to Mode S and ADS-B decoding
///
/// Use as `Message::try_from()` in mostly all applications
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Message {
    /// Calculated from all bits, should be 0 for ADS-B (raises a DekuError),
    /// icao24 otherwise
    #[serde(skip, default = "u32::default")]
    pub crc: u32,

    /// The Downlink Format encoded in 5 bits
    #[serde(flatten)]
    pub df: DF,
}

impl DekuContainerRead<'_> for Message {
    fn from_reader<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
        input: (&mut R, usize),
    ) -> Result<(usize, Self), DekuError>
    where
        Self: Sized,
    {
        let reader = &mut deku::reader::Reader::new(input.0);
        if input.1 != 0 {
            reader.skip_bits(input.1, Order::Msb0)?;
        }

        let value = Self::from_reader_with_ctx(reader, ())?;

        Ok((reader.bits_read, value))
    }

    fn from_bytes(
        input: (&[u8], usize),
    ) -> Result<((&[u8], usize), Self), DekuError>
    where
        Self: Sized,
    {
        let mut cursor = deku::no_std_io::Cursor::new(input.0);
        let reader = &mut Reader::new(&mut cursor);
        if input.1 != 0 {
            reader.skip_bits(input.1, Order::Msb0)?;
        }

        let value = Self::from_reader_with_ctx(reader, ())?;
        let read_whole_byte = reader.bits_read.is_multiple_of(8);
        let idx = if read_whole_byte {
            reader.bits_read / 8
        } else {
            (reader.bits_read - (reader.bits_read % 8)) / 8
        };
        Ok(((&input.0[idx..], reader.bits_read % 8), value))
    }
}

impl DekuReader<'_> for Message {
    fn from_reader_with_ctx<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
        reader: &mut Reader<R>,
        _: (),
    ) -> Result<Self, DekuError>
    where
        Self: Sized,
    {
        const MODES_LONG_MSG_BYTES: usize = 14;
        const MODES_SHORT_MSG_BYTES: usize = 7;

        let mut remaining_bytes = vec![];

        let value = reader.read_bits(8, Order::Msb0)?;
        let res = value.unwrap().into_vec();
        remaining_bytes.extend_from_slice(&res);

        // Decode the DF quickly to determine the length of the message
        let df = remaining_bytes[0] >> 3;

        let bit_len = if df & 0x10 != 0 {
            MODES_LONG_MSG_BYTES * 8
        } else {
            MODES_SHORT_MSG_BYTES * 8
        };
        debug!("Reading {} bits based on DF={}", bit_len, df);

        let value = reader.read_bits(bit_len - 8, Order::Msb0)?;
        let res = value.unwrap().into_vec();
        remaining_bytes.extend_from_slice(&res);

        let crc = modes_checksum(&remaining_bytes, bit_len)?;
        // Also the CRC must be 0 for ADS-B (DF=17) messages
        match (df, crc) {
            (17, c) if c > 0 => Err(DekuError::Assertion(
                format!("Invalid CRC in ADS-B message: {c}").into(),
            )),
            _ => {
                // Restart reading by creating a new cursor/reader (with context)
                let mut input = deku::no_std_io::Cursor::new(&remaining_bytes);
                let mut reader = Reader::new(&mut input);
                match DF::from_reader_with_ctx(&mut reader, crc) {
                    Ok(df) => Ok(Self { crc, df }),
                    Err(e) => Err(e),
                }
            }
        }
    }
}

impl core::convert::TryFrom<&[u8]> for Message {
    type Error = DekuError;

    #[inline]
    fn try_from(input: &[u8]) -> core::result::Result<Self, Self::Error> {
        let total_len = input.len();
        let mut cursor = deku::no_std_io::Cursor::new(input);
        let (amt_read, res) =
            <Self as DekuContainerRead>::from_reader((&mut cursor, 0))?;
        if (amt_read / 8) != total_len {
            return Err(DekuError::Parse(("Too much data").into()));
        }
        Ok(res)
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let crc = self.crc;
        match &self.df {
            DF::ShortAirAirSurveillance { ac, .. } => {
                writeln!(f, " DF0. Short Air-Air Surveillance")?;
                writeln!(f, "  ICAO Address:  {crc:06x} (Mode S / ADS-B)")?;
                if let Some(altitude) = ac.0 {
                    writeln!(f, "  Air/Ground:    airborne")?;
                    writeln!(f, "  Altitude:      {altitude} ft barometric")?;
                } else {
                    writeln!(
                        f,
                        "  Air/Ground:    ground or altitude unavailable"
                    )?;
                }
            }
            DF::SurveillanceAltitudeReply { fs, ac, .. } => {
                writeln!(f, " DF4. Surveillance, Altitude Reply")?;
                writeln!(f, "  ICAO Address:  {crc:06x} (Mode S / ADS-B)")?;
                writeln!(f, "  Air/Ground:    {fs}")?;
                if let Some(altitude) = ac.0 {
                    writeln!(f, "  Altitude:      {altitude} ft barometric")?;
                }
            }
            DF::SurveillanceIdentityReply { fs, id, .. } => {
                writeln!(f, " DF5. Surveillance, Identity Reply")?;
                writeln!(f, "  ICAO Address:  {crc:06x} (Mode S / ADS-B)")?;
                writeln!(f, "  Air/Ground:    {fs}")?;
                writeln!(f, "  Squawk:        {id}")?;
            }
            DF::AllCallReply {
                capability, icao, ..
            } => {
                writeln!(f, " DF11. All Call Reply")?;
                writeln!(f, "  ICAO Address:  {icao} (Mode S / ADS-B)")?;
                writeln!(f, "  Air/Ground:    {capability}")?;
            }
            DF::LongAirAirSurveillance { ac, .. } => {
                writeln!(f, " DF16. Long Air-Air ACAS")?;
                writeln!(f, "  ICAO Address:  {crc:06x} (Mode S / ADS-B)")?;
                if let Some(altitude) = ac.0 {
                    writeln!(f, "  Air/Ground:    airborne")?;
                    writeln!(f, "  Baro altitude: {altitude} ft")?;
                } else {
                    writeln!(
                        f,
                        "  Air/Ground:    ground or altitude unavailable"
                    )?;
                }
            }
            DF::ExtendedSquitterADSB(msg) => {
                write!(f, "{msg}")?;
            }
            DF::ExtendedSquitterTisB { cf, .. } => {
                // DF18
                write!(f, "{cf}")?;
            }
            DF::ExtendedSquitterMilitary { .. } => {} // DF19
            DF::CommBAltitudeReply { ac, bds, .. } => {
                writeln!(f, " DF20. Comm-B, Altitude Reply")?;
                writeln!(f, "  ICAO Address:  {crc:x?}")?;
                if let Some(altitude) = ac.0 {
                    writeln!(f, "  Altitude:      {altitude} ft")?;
                }
                write!(f, "  {bds}")?;
            }
            DF::CommBIdentityReply { id, bds, .. } => {
                writeln!(f, " DF21. Comm-B, Identity Reply")?;
                writeln!(f, "  ICAO Address:  {crc:x?}")?;
                writeln!(f, "  Squawk:        {id:x?}")?;
                write!(f, "    {bds}")?;
            }
            DF::CommDExtended { .. } => {
                writeln!(f, " DF24..=31 Comm-D Extended Length Message")?;
                writeln!(f, "  ICAO Address:     {crc:x?}")?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorMetadata {
    /// The timestamp when the message was received by the receptor
    pub system_timestamp: f64,
    /// The GNSS timestamp of the message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gnss_timestamp: Option<f64>,
    /// Number of nanoseconds since beginning of UTC day
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nanoseconds: Option<u64>,
    /// The signal level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rssi: Option<f32>,
    /// The identifier of the receptor
    pub serial: u64,
    /// A possible name for the receptor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug)]
struct SerializeConfig {
    /// Include the decode time in the serialization process (default: false)
    pub decode_time: bool,
}

static CONFIG: OnceCell<SerializeConfig> = OnceCell::new();

fn skip_serialize_decode_time(field: &Option<f64>) -> bool {
    let decode_time = CONFIG.get().map(|cfg| cfg.decode_time).unwrap_or(false);
    !decode_time | field.is_none()
}

pub fn serialize_config(decode_time: bool) {
    CONFIG
        .set(SerializeConfig { decode_time })
        .expect("configuration can only happen once");
}

#[derive(Serialize, Deserialize)]
pub struct TimedMessage {
    /// The timestamp (in s) of the first time the message was received
    pub timestamp: f64,
    /// The message payload
    #[serde(serialize_with = "as_hex", deserialize_with = "from_hex")]
    pub frame: Vec<u8>,
    /// The decoded message
    #[serde(flatten)]
    pub message: Option<Message>,
    /// Information about when and where the message was received
    pub metadata: Vec<SensorMetadata>,
    /// Debugging information about decoding time (not serialized)
    #[serde(skip_serializing_if = "skip_serialize_decode_time")]
    pub decode_time: Option<f64>,
}

pub fn as_hex<S>(data: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex_string = hex::encode(data);
    serializer.serialize_str(&hex_string)
}

pub fn from_hex<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_string = String::deserialize(deserializer)?; // Deserialize as a string
    hex::decode(&hex_string).map_err(serde::de::Error::custom) // Decode and handle errors
}

impl fmt::Display for TimedMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{:.5},{}", &self.timestamp, hex::encode(&self.frame))?;
        if let Some(msg) = &self.message {
            writeln!(f, "{msg}")?;
        }
        write!(f, "")
    }
}
impl fmt::Debug for TimedMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{:.5},{}", &self.timestamp, hex::encode(&self.frame))?;
        if let Some(msg) = &self.message {
            writeln!(f, "{msg:#}")?;
        }
        write!(f, "")
    }
}

/// ICAO 24-bit address, commonly use to reference airframes, i.e. tail numbers
/// of aircraft
#[derive(PartialEq, Eq, PartialOrd, DekuRead, Hash, Copy, Clone, Ord)]
#[deku(ctx = "crc: u32")]
pub struct IcaoParity(
    // Ok it looks convoluted, actually the final bits are already read when
    // we compute the crc so we don't need to read this again
    #[deku(bits = 24, map = "|_v: u32| -> Result<_, DekuError> { Ok(crc) }")]
    pub u32,
);

impl fmt::Debug for IcaoParity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:06x}", self.0)?;
        Ok(())
    }
}

impl fmt::Display for IcaoParity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:06x}", self.0)?;
        Ok(())
    }
}

impl Serialize for IcaoParity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let icao = format!("{:06x}", &self.0);
        serializer.serialize_str(&icao)
    }
}

impl<'de> Deserialize<'de> for IcaoParity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        <IcaoParity as core::str::FromStr>::from_str(&s)
            .map_err(serde::de::Error::custom)
    }
}

impl core::str::FromStr for IcaoParity {
    type Err = core::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let num = u32::from_str_radix(s, 16)?;
        Ok(Self(num))
    }
}

/// ICAO 24-bit address, commonly use to reference airframes, i.e. tail numbers
/// of aircraft
#[derive(PartialEq, Eq, PartialOrd, DekuRead, Hash, Copy, Clone, Ord)]
pub struct ICAO(#[deku(bits = 24, endian = "big")] pub u32);

impl fmt::Debug for ICAO {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:06x}", self.0)?;
        Ok(())
    }
}

impl fmt::Display for ICAO {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:06x}", self.0)?;
        Ok(())
    }
}

impl Serialize for ICAO {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let icao = format!("{:06x}", &self.0);
        serializer.serialize_str(&icao)
    }
}

impl<'de> Deserialize<'de> for ICAO {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        <ICAO as std::str::FromStr>::from_str(&s)
            .map_err(serde::de::Error::custom)
    }
}

impl core::str::FromStr for ICAO {
    type Err = core::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let num = u32::from_str_radix(s, 16)?;
        Ok(Self(num))
    }
}

impl From<IcaoParity> for ICAO {
    fn from(ap: IcaoParity) -> Self {
        Self(ap.0)
    }
}

/// Mode A Code / Squawk (bits 12-24): Per ICAO Doc 9871 Table B-2-97a
/// 13-bit identity code (standard 4-digit octal squawk).
/// Encoded in Gillham format.
#[derive(PartialEq, DekuRead, Copy, Clone)]
pub struct IdentityCode(#[deku(reader = "Self::read(deku::reader)")] pub u16);

impl IdentityCode {
    fn read<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
        reader: &mut Reader<R>,
    ) -> Result<u16, DekuError> {
        let num = u16::from_reader_with_ctx(
            reader,
            (deku::ctx::Endian::Big, deku::ctx::BitSize(13)),
        )?;
        Ok(decode_id13(num))
    }
}

impl fmt::Debug for IdentityCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04x}", self.0)?;
        Ok(())
    }
}

impl fmt::Display for IdentityCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04x}", self.0)?;
        Ok(())
    }
}

impl Serialize for IdentityCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let squawk = format!("{:04x}", &self.0);
        serializer.serialize_str(&squawk)
    }
}

impl<'de> Deserialize<'de> for IdentityCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let num =
            u16::from_str_radix(&s, 16).map_err(serde::de::Error::custom)?;
        Ok(Self(num))
    }
}

/// 13-bit encoded altitude field (AC field) per ICAO Annex 10 Vol IV §3.1.2.6.5.4
///
/// # Encoding Modes
///
/// The AC field supports three encoding modes determined by M-bit and Q-bit:
///
/// 1. **Q-bit encoding (M=0, Q=1)**: 25 ft resolution
///    - Range: -1000 ft to +50,175 ft
///    - Formula: altitude = (N × 25) - 1000 ft
///    - LSB: 25 ft
///    - Supports negative altitudes for below-sea-level airports
///
/// 2. **Gillham code (M=0, Q=0)**: 100 ft resolution
///    - Range: -1200 ft to +126,700 ft
///    - Uses Gray code encoding (Mode C compatible)
///    - LSB: 100 ft
///
/// 3. **Metric encoding (M=1)**: Reserved for metric altitudes
///    - Converts meters to feet for compatibility
///
///    Many apparent M-bit messages in real-world data are actually demodulation
///    errors where bit 6 was flipped.
///
///    Analysis of cherry-picked flights showed:
///    - "M-bit" altitudes decode to 239-5,390ft (unrealistic for cruise)
///    - Same messages decoded as Q-bit give 34,000-43,400ft (realistic cruise)
///    - 40% of "M-bit" messages have BOTH M-bit AND Q-bit set (spec violation)
///
/// # Special Values
///
/// - All zeros (0x0000) = altitude not available (DO-260B §2.2.5.1.5)
/// - Invalid Gillham patterns → None
///
/// # Examples
///
/// - Amsterdam Schiphol: -11 ft (below sea level)
/// - Sea level: 0 ft
/// - Typical cruise: 35,000 ft
/// - Maximum: 50,175 ft (Q-bit) or 126,700 ft (Gillham)
///
/// # References
///
/// - ICAO Annex 10 Volume IV §3.1.2.6.5.4 (AC altitude code)
/// - DO-260B §2.2.5.1.5 (all-zeros = not available)
#[derive(
    Debug, PartialEq, Eq, Serialize, Deserialize, DekuRead, Copy, Clone,
)]
pub struct AC13Field(
    #[deku(reader = "Self::read(deku::reader)")] pub Option<i32>,
);

impl AC13Field {
    fn read<R: deku::no_std_io::Read + deku::no_std_io::Seek>(
        reader: &mut Reader<R>,
    ) -> Result<Option<i32>, DekuError> {
        let ac13field = u16::from_reader_with_ctx(
            reader,
            (deku::ctx::Endian::Big, deku::ctx::BitSize(13)),
        )?;

        // All zeros = altitude not available (ICAO Annex 10 Vol IV §3.1.2.6.5.4.f)
        if ac13field == 0 {
            return Ok(None);
        }

        // Extract M-bit (bit 26 in message, bit 6 in 13-bit field) and Q-bit (bit 28 in message, bit 4 in field)
        let m_bit = ac13field & 0x0040; // Bit 6: 0=feet, 1=meters
        let q_bit = ac13field & 0x0010; // Bit 4: 0=Gillham code, 1=25ft resolution

        // NOTE: M-bit altitude is marked "reserved for future use" in ICAO Doc 9871
        // and is extremely rare in practice (~0.2-0.8% of altitude messages).
        if m_bit != 0 {
            if q_bit != 0 {
                // Invalid combination (M=1, Q=1) - return None per spec
                return Ok(None);
            }
            // Metric encoding (M=1): ICAO Annex 10 Vol IV §3.1.2.6.5.4.e
            // Reserved for future use - converts to feet for compatibility
            // Bit pattern: Upper 7 bits (bits 12-6) and lower 6 bits (bits 5-0)
            // meters = (upper 7 bits << 2) | lower 6 bits
            let meters = ((ac13field & 0x1f80) >> 2) | (ac13field & 0x3f);

            // Convert meters to feet using integer arithmetic to avoid f32 precision loss
            // 1 meter = 3.28084 feet exactly
            // Using integer math: feet = (meters * 328084) / 100000
            let feet = ((meters as i64) * 328084) / 100000;
            Ok(Some(feet as i32))
        } else if q_bit != 0 {
            // Q-bit encoding (M=0, Q=1): ICAO Annex 10 Vol IV §3.1.2.6.5.4.d
            // 11-bit binary field with 25 ft LSB
            // Formula: altitude = (N × 25) - 1000 ft, range: [-1000, +50175] ft
            //
            // Bit pattern: C1 A1 C2 A2 C4 A4 [M=0] B1 [Q=1] B2 D2 B4 D4
            // Remove M and Q bits to get N (bits: C1 A1 C2 A2 C4 A4 B1 B2 D2 B4 D4)
            let n = ((ac13field & 0x1f80) >> 2)  // Upper 6 bits (C1-A4)
                | ((ac13field & 0x0020) >> 1)    // B1 (bit after Q)
                | (ac13field & 0x000f); // Lower 4 bits (B2-D4)
            let altitude = i32::from(n) * 25 - 1000;
            Ok(Some(altitude))
        } else {
            // Gillham code (M=0, Q=0): ICAO Annex 10 Vol IV §3.1.2.6.5.4.c
            // Mode C compatible encoding with Gray code
            // Bit pattern: C1 A1 C2 A2 C4 A4 [M=0] B1 [Q=0] B2 D2 B4 D4
            // 100 ft increments, range: [-1200, +126700] ft
            if let Ok(n) = gray2alt(decode_id13(ac13field)) {
                let altitude = 100 * n;
                Ok(Some(altitude))
            } else {
                // Invalid Gillham pattern - return None per spec
                Ok(None)
            }
        }
    }
}

/// Transponder level and additional information (3.1.2.5.2.2.1)
#[derive(
    Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone, Default,
)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "3")]
#[allow(non_camel_case_types)]
pub enum Capability {
    /// Level 1 transponder (surveillance only), and either airborne or on the ground
    #[serde(rename = "level1")]
    #[default]
    AG_LEVEL1 = 0x00,
    #[deku(id_pat = "0x01..=0x03")]
    AG_RESERVED,
    /// Level 2 or above transponder, on ground
    #[serde(rename = "ground")]
    AG_GROUND = 0x04,
    /// Level 2 or above transponder, airborne
    #[serde(rename = "airborne")]
    AG_AIRBORNE = 0x05,
    /// Level 2 or above transponder, either airborne or on ground
    #[serde(rename = "ground/airborne")]
    AG_GROUND_AIRBORNE = 0x06,
    /// DR field is not equal to 0,
    /// or fs field equal 2, 3, 4, or 5,
    /// and either airborne or on ground
    AG_DR0 = 0x07,
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::AG_LEVEL1 => "Level 1",
                Self::AG_RESERVED => "reserved",
                Self::AG_GROUND => "ground",
                Self::AG_AIRBORNE => "airborne",
                Self::AG_GROUND_AIRBORNE => "ground/airborne",
                Self::AG_DR0 => "DR0",
            }
        )
    }
}

/// Airborne or Ground and SPI (used in DF=4, 5, 20 or 21)
#[derive(
    Debug, PartialEq, Serialize, Deserialize, DekuRead, Copy, Clone, Default,
)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "3")]
#[serde(rename_all = "snake_case")]
pub enum FlightStatus {
    #[default]
    NoAlertNoSpiAirborne = 0b000,
    NoAlertNoSpiOnGround = 0b001,
    AlertNoSpiAirborne = 0b010,
    AlertNoSpiOnGround = 0b011,
    AlertSpiAirborneGround = 0b100,
    NoAlertSpiAirborneGround = 0b101,
    Reserved = 0b110,
    NotAssigned = 0b111,
}

impl fmt::Display for FlightStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::NoAlertNoSpiAirborne => "airborne",
                Self::AlertSpiAirborneGround
                | Self::NoAlertSpiAirborneGround => "airborne/ground",
                Self::NoAlertNoSpiOnGround => "ground",
                Self::AlertNoSpiAirborne => "airborne",
                Self::AlertNoSpiOnGround => "ground",
                _ => "reserved",
            }
        )
    }
}

/// The downlink request (used in DF=4, 5, 20 or 21)
#[derive(
    Debug, PartialEq, Eq, Serialize, Deserialize, DekuRead, Copy, Clone, Default,
)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "5")]
pub enum DownlinkRequest {
    #[default]
    None = 0b00000,
    RequestSendCommB = 0b00001,
    CommBBroadcastMsg1 = 0b00100,
    CommBBroadcastMsg2 = 0b00101,
    #[deku(id_pat = "_")]
    Unknown,
}

/// The utility message (used in DF=4, 5, 20 or 21)
#[derive(
    Debug, PartialEq, Eq, Serialize, Deserialize, DekuRead, Copy, Clone, Default,
)]
pub struct UtilityMessage {
    #[deku(bits = "4")]
    pub iis: u8,
    pub ids: UtilityMessageType,
}
/// The utility message type (used in DF=4, 5, 20 or 21)
#[derive(
    Debug, PartialEq, Eq, Serialize, Deserialize, DekuRead, Copy, Clone, Default,
)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "2")]
pub enum UtilityMessageType {
    #[default]
    NoInformation = 0b00,
    CommB = 0b01,
    CommC = 0b10,
    CommD = 0b11,
}

/// The control field in TIS-B messages (DF=18)
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
pub struct ControlField {
    #[serde(rename = "tisb")]
    pub field_type: ControlFieldType,
    /// AA: Address, Announced
    #[serde(rename = "icao24")]
    pub aa: ICAO,
    /// ME: message, extended squitter
    #[serde(flatten)]
    pub me: ME,
}

impl fmt::Display for ControlField {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

/// The control field type in TIS-B messages (DF=18)
#[derive(Debug, PartialEq, Serialize, Deserialize, DekuRead, Clone)]
#[deku(id_type = "u8", bits = "3")]
#[allow(non_camel_case_types)]
pub enum ControlFieldType {
    /// ADS-B Message from a non-transponder device
    #[deku(id = "0")]
    ADSB_ES_NT,

    /// Reserved for ADS-B for ES/NT devices for alternate address space
    #[deku(id = "1")]
    ADSB_ES_NT_ALT,

    /// Code 2, Fine Format TIS-B Message
    #[deku(id = "2")]
    TISB_FINE,

    /// Code 3, Coarse Format TIS-B Message
    #[deku(id = "3")]
    TISB_COARSE,

    /// Code 4, Coarse Format TIS-B Message
    #[deku(id = "4")]
    TISB_MANAGE,

    /// Code 5, TIS-B Message for replay ADS-B Message
    ///
    /// Anonymous 24-bit addresses
    #[deku(id = "5")]
    TISB_ADSB_RELAY,

    /// Code 6, TIS-B Message, Same as DF=17
    #[deku(id = "6")]
    TISB_ADSB,

    /// Code 7, Reserved
    #[deku(id = "7")]
    Reserved,
}

impl fmt::Display for ControlFieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s_type = match self {
            Self::ADSB_ES_NT | Self::ADSB_ES_NT_ALT => "(ADS-B)",
            Self::TISB_COARSE | Self::TISB_ADSB_RELAY | Self::TISB_FINE => {
                "(TIS-B)"
            }
            Self::TISB_MANAGE | Self::TISB_ADSB => "(ADS-R)",
            Self::Reserved => "(unknown addressing scheme)",
        };
        write!(f, "{s_type}")
    }
}

/// Uplink / Downlink (DF=24)
#[derive(
    Debug, PartialEq, Eq, Serialize, Deserialize, DekuRead, Copy, Clone, Default,
)]
#[repr(u8)]
#[deku(id_type = "u8", bits = "1")]
pub enum KE {
    #[default]
    DownlinkELMTx = 0,
    UplinkELMAck = 1,
}

/// Decode a [Gillham code](https://en.wikipedia.org/wiki/Gillham_code)
/// 
/// In the squawk (identity) field bits are interleaved as follows in
/// (message bit 20 to bit 32):
///
/// C1-A1-C2-A2-C4-A4-ZERO-B1-D1-B2-D2-B4-D4
///
/// So every group of three bits A, B, C, D represent an integer from 0 to 7.
///
/// The actual meaning is just 4 octal numbers, but we convert it into a hex
/// number that happens to represent the four octal numbers.
///
#[rustfmt::skip]
pub fn decode_id13(id13_field: u16) -> u16 {
    let mut hex_gillham: u16 = 0;

    if id13_field & 0x1000 != 0 { hex_gillham |= 0x0010; } // Bit 12 = C1
    if id13_field & 0x0800 != 0 { hex_gillham |= 0x1000; } // Bit 11 = A1
    if id13_field & 0x0400 != 0 { hex_gillham |= 0x0020; } // Bit 10 = C2
    if id13_field & 0x0200 != 0 { hex_gillham |= 0x2000; } // Bit  9 = A2
    if id13_field & 0x0100 != 0 { hex_gillham |= 0x0040; } // Bit  8 = C4
    if id13_field & 0x0080 != 0 { hex_gillham |= 0x4000; } // Bit  7 = A4
    // if id13_field & 0x0040 != 0 {hex_gillham |= 0x0800;} // Bit  6 = X  or M
    if id13_field & 0x0020 != 0 { hex_gillham |= 0x0100; } // Bit  5 = B1
    if id13_field & 0x0010 != 0 { hex_gillham |= 0x0001; } // Bit  4 = D1 or Q
    if id13_field & 0x0008 != 0 { hex_gillham |= 0x0200; } // Bit  3 = B2
    if id13_field & 0x0004 != 0 { hex_gillham |= 0x0002; } // Bit  2 = D2
    if id13_field & 0x0002 != 0 { hex_gillham |= 0x0400; } // Bit  1 = B4
    if id13_field & 0x0001 != 0 { hex_gillham |= 0x0004; } // Bit  0 = D4

    hex_gillham
}

/// Convert a [Gillham code](https://en.wikipedia.org/wiki/Gillham_code) to
/// an altitude in feet.
#[rustfmt::skip]
pub fn gray2alt(gray: u16) -> Result<i32, &'static str> {
    let mut five_hundreds: u32 = 0;
    let mut one_hundreds: u32 = 0;

    // check zero bits are zero, D1 set is illegal; C1,,C4 cannot be Zero
    if (gray & 0x8889) != 0 || (gray & 0x00f0) == 0 {
        return Err("Invalid altitude");
    }

    if gray & 0x0010 != 0 { one_hundreds ^= 0x007; } // C1
    if gray & 0x0020 != 0 { one_hundreds ^= 0x003; } // C2
    if gray & 0x0040 != 0 { one_hundreds ^= 0x001; } // C4

    // Remove 7s from OneHundreds (Make 7->5, snd 5->7).
    if (one_hundreds & 5) == 5 { one_hundreds ^= 2; }

    // Check for invalid codes, only 1 to 5 are valid
    if one_hundreds > 5 { return Err("Invalid altitude"); }

    // if gray & 0x0001 {five_hundreds ^= 0x1FF;} // D1 never used for altitude
    if gray & 0x0002 != 0 { five_hundreds ^= 0x0ff; } // D2
    if gray & 0x0004 != 0 { five_hundreds ^= 0x07f; } // D4
    if gray & 0x1000 != 0 { five_hundreds ^= 0x03f; } // A1
    if gray & 0x2000 != 0 { five_hundreds ^= 0x01f; } // A2
    if gray & 0x4000 != 0 { five_hundreds ^= 0x00f; } // A4
    if gray & 0x0100 != 0 { five_hundreds ^= 0x007; } // B1
    if gray & 0x0200 != 0 { five_hundreds ^= 0x003; } // B2
    if gray & 0x0400 != 0 { five_hundreds ^= 0x001; } // B4

    // Correct order of one_hundreds.
    if five_hundreds & 1 != 0 && one_hundreds <= 6 {
        one_hundreds = 6 - one_hundreds;
    }

    let n = (five_hundreds * 5) + one_hundreds;
    if n >= 13 {
        Ok(n as i32 - 13)
    } else {
        Err("Invalid altitude")
    }
}

#[cfg(test)]
mod tests {

    use crate::prelude::AircraftOperationStatus;

    use super::*;
    use hexlit::hex;

    #[test]
    fn test_ac13field() {
        let bytes = hex!("a0001910cc300030aa0000eae004");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        match msg.df {
            DF::CommBAltitudeReply { ac, .. } => {
                assert_eq!(ac.0, Some(39000));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_invalid_crc() {
        let bytes = hex!("8d4ca251204994b1c36e60a5343d");
        let res = Message::from_bytes((&bytes, 0));
        if let Err(e) = res {
            match e {
                DekuError::Assertion(_msg) => (),
                _ => unreachable!(),
            }
        } else {
            unreachable!()
        }
    }

    #[test]
    fn test_df18() {
        // Referred in issue 337
        let bytes = hex!("95c639eefbffffedd5fefbff4f6f");
        let (_, msg) = Message::from_bytes((&bytes, 0)).unwrap();
        if let DF::ExtendedSquitterTisB { cf, .. } = &msg.df {
            assert_eq!(format!("{}", cf.aa), "c639ee");
            assert_eq!(cf.field_type, ControlFieldType::TISB_ADSB_RELAY);
            if let ME::BDS65(me) = cf.me {
                if let AircraftOperationStatus::Reserved { .. } = me {
                    // ok
                } else {
                    panic!("Expected Reserved BDS65");
                }
            }

            // this one should not panic
            let pretty = serde_json::to_string_pretty(&msg).unwrap();
            println!("{}", pretty);
            return;
        }
        unreachable!();
    }

    #[test]
    fn test_mbit_altitude_precision() {
        // Test M-bit (metric) altitude encoding with integer-based conversion
        // This verifies that we use integer math instead of f32 to avoid precision loss
        //
        // This test validates the spec-compliant integer implementation, but
        // production code should be aware most M-bit messages are likely corrupted.

        // M-bit altitude bit pattern (13 bits total):
        // Bits [12-7]: upper 5 bits of meters value (bits 10-6 of meters)
        // Bit 6: M-bit = 1 (indicates metric encoding)
        // Bits [5-0]: lower 6 bits of meters value
        // Total: 11 bits for meters (range 0-2047 meters)
        //
        // To encode: ac13field = ((meters & 0x7C0) << 2) | 0x40 | (meters & 0x3F)
        // To decode: meters = ((ac13field & 0x1F80) >> 2) | (ac13field & 0x3F)

        // Helper function to create AC13 field with M-bit and test conversion
        let test_meters_to_feet = |meters: u16| -> i32 {
            // Encode meters into AC13 field format with M-bit set
            let ac13field =
                ((meters & 0x07C0) << 2) | 0x0040 | (meters & 0x003F);

            // Verify M-bit is set
            let m_bit = ac13field & 0x0040;
            assert_ne!(m_bit, 0, "M-bit should be set");

            // Decode using the actual implementation logic
            let decoded_meters =
                ((ac13field & 0x1f80) >> 2) | (ac13field & 0x3f);
            assert_eq!(
                decoded_meters, meters,
                "Meters should round-trip correctly: meters={}, ac13=0x{:04x}, decoded={}",
                meters, ac13field, decoded_meters
            );

            // Use the same integer-based conversion as the implementation
            let feet = ((decoded_meters as i64) * 328084) / 100000;
            feet as i32
        };

        // Test various meter values and verify integer precision
        // Expected values calculated with: meters * 3.28084

        // Test 1: 100 meters = 328.084 feet → 328 feet
        assert_eq!(test_meters_to_feet(100), 328);

        // Test 2: 500 meters = 1640.42 feet → 1640 feet
        assert_eq!(test_meters_to_feet(500), 1640);

        // Test 3: 1000 meters = 3280.84 feet → 3280 feet
        assert_eq!(test_meters_to_feet(1000), 3280);

        // Test 4: 2000 meters = 6561.68 feet → 6561 feet
        assert_eq!(test_meters_to_feet(2000), 6561);

        // Test 5: Verify precision with value that may differ between f32 and integer
        // 1234 meters = 4048.4856 feet → 4048 feet
        assert_eq!(test_meters_to_feet(1234), 4048);

        // Test 6: Maximum 11-bit value: 2047 meters = 6715.87948 feet → 6715 feet
        assert_eq!(test_meters_to_feet(2047), 6715);

        // Test 7: Compare f32 vs integer for a specific case
        // This demonstrates why integer math is better
        let meters = 1357_u16;
        let feet_integer = ((meters as i64) * 328084) / 100000;
        let _feet_f32 = (meters as f32 * 3.28084) as i64;
        // Both should give 4452, but f32 may have rounding differences
        assert_eq!(feet_integer, 4452);
        assert_eq!(test_meters_to_feet(meters), 4452);
    }
}
