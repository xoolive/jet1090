#![allow(rustdoc::missing_crate_level_docs)]

use pyo3::exceptions::{PyAssertionError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rayon::prelude::*;
use rs1090::data::patterns::{
    aircraft_information as patterns, AircraftInformation,
};
use rs1090::decode::bds::bds05::AirbornePosition;
use rs1090::decode::bds::bds10::DataLinkCapability;
use rs1090::decode::bds::bds17::CommonUsageGICBCapabilityReport;
use rs1090::decode::bds::bds18::GICBCapabilityReportPart1;
use rs1090::decode::bds::bds19::GICBCapabilityReportPart2;
use rs1090::decode::bds::bds20::AircraftIdentification;
use rs1090::decode::bds::bds21::AircraftAndAirlineRegistrationMarkings;
use rs1090::decode::bds::bds30::ACASResolutionAdvisory;
use rs1090::decode::bds::bds40::SelectedVerticalIntention;
use rs1090::decode::bds::bds44::MeteorologicalRoutineAirReport;
use rs1090::decode::bds::bds45::MeteorologicalHazardReport;
use rs1090::decode::bds::bds50::TrackAndTurnReport;
use rs1090::decode::bds::bds60::HeadingAndSpeedReport;
use rs1090::decode::bds::bds65::AircraftOperationStatus;
use rs1090::decode::cpr::{
    airborne_position_with_reference, decode_positions,
    surface_position_with_reference, Position,
};
use rs1090::decode::flarm::Flarm;
use rs1090::prelude::*;

#[pyfunction]
fn decode_1090(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    if let Ok((_, msg)) = Message::from_bytes((&bytes, 0)) {
        let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
        Ok(pkl)
    } else {
        Ok([128, 4, 78, 46].to_vec()) // None
    }
}

fn decode_message_with_reference(me: &mut ME, reference: [f64; 2]) {
    let [latitude_ref, longitude_ref] = reference;
    match me {
        ME::BDS05 {
            inner: airborne, ..
        } => {
            if let Some(pos) = airborne_position_with_reference(
                airborne,
                latitude_ref,
                longitude_ref,
            ) {
                airborne.latitude = Some(pos.latitude);
                airborne.longitude = Some(pos.longitude);
            }
        }
        ME::BDS06 { inner: surface, .. } => {
            if let Some(pos) = surface_position_with_reference(
                surface,
                latitude_ref,
                longitude_ref,
            ) {
                surface.latitude = Some(pos.latitude);
                surface.longitude = Some(pos.longitude);
            }
        }
        _ => (),
    }
}

#[pyfunction]
fn decode_1090_with_reference(
    msg: String,
    reference: [f64; 2],
) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    if let Ok((_, mut msg)) = Message::from_bytes((&bytes, 0)) {
        match &mut msg.df {
            ExtendedSquitterTisB { cf, .. } => {
                decode_message_with_reference(&mut cf.me, reference)
            }
            ExtendedSquitterADSB(adsb) => {
                decode_message_with_reference(&mut adsb.message, reference)
            }
            _ => {}
        }
        let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
        Ok(pkl)
    } else {
        Ok([128, 4, 78, 46].to_vec()) // None
    }
}

struct DecodeError(DekuError);

impl From<DecodeError> for PyErr {
    fn from(error: DecodeError) -> Self {
        match error.0 {
            DekuError::Assertion(msg) => PyAssertionError::new_err(msg),
            _ => PyValueError::new_err(error.0.to_string()),
        }
    }
}

#[pyfunction]
fn decode_bds05(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    let tc = &bytes[4] >> 3;
    if (9..22).contains(&tc) && tc != 19 {
        let mut input = std::io::Cursor::new(&bytes[4..]);
        let mut reader = Reader::new(&mut input);
        match AirbornePosition::from_reader_with_ctx(&mut reader, tc) {
            Ok(msg) => {
                let pkl =
                    serde_pickle::to_vec(&msg, Default::default()).unwrap();
                Ok(pkl)
            }
            Err(e) => Err(DecodeError(e).into()),
        }
    } else {
        let msg =
            format!("Invalid typecode {tc} for BDS 0,5 (9 to 18 or 20 to 22)");
        Err(PyAssertionError::new_err(msg))
    }
}

#[pyfunction]
fn decode_bds10(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match DataLinkCapability::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds17(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match CommonUsageGICBCapabilityReport::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds18(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match GICBCapabilityReportPart1::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds19(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match GICBCapabilityReportPart2::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds20(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match AircraftIdentification::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds21(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match AircraftAndAirlineRegistrationMarkings::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds30(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match ACASResolutionAdvisory::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds40(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match SelectedVerticalIntention::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds44(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match MeteorologicalRoutineAirReport::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}
#[pyfunction]
fn decode_bds45(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match MeteorologicalHazardReport::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds50(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match TrackAndTurnReport::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds60(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    match HeadingAndSpeedReport::from_bytes((&bytes[4..], 0)) {
        Ok((_, msg)) => {
            let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
            Ok(pkl)
        }
        Err(e) => Err(DecodeError(e).into()),
    }
}

#[pyfunction]
fn decode_bds65(msg: String) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    let tc = &bytes[4] >> 3;
    let enum_id = &bytes[4] & 0b111;
    match (tc, enum_id) {
        (31, id) if id < 2 => {
            match AircraftOperationStatus::from_bytes((&bytes[4..], 5)) {
                Ok((_, msg)) => {
                    let pkl =
                        serde_pickle::to_vec(&msg, Default::default()).unwrap();
                    Ok(pkl)
                }
                Err(e) => Err(DecodeError(e).into()),
            }
        }
        _ => Err(PyAssertionError::new_err(format!(
            "Invalid typecode {tc} (31) or category {enum_id} (0 or 1) for BDS 6,5"
        ))),
    }
}

#[pyfunction]
fn decode_1090_vec(msgs_set: Vec<Vec<String>>) -> PyResult<Vec<u8>> {
    let res: Vec<Option<Message>> = msgs_set
        .par_iter()
        .map(|msgs| {
            msgs.iter()
                .map(|msg| {
                    let bytes = hex::decode(msg).unwrap();
                    if let Ok((_, msg)) = Message::from_bytes((&bytes, 0)) {
                        Some(msg)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .flat_map(|v: Vec<Option<Message>>| v)
        .collect();
    let pkl = serde_pickle::to_vec(&res, Default::default()).unwrap();
    Ok(pkl)
}

#[pyfunction]
#[pyo3(signature = (msgs_set, ts_set, reference=None))]
fn decode_1090t_vec(
    msgs_set: Vec<Vec<String>>,
    ts_set: Vec<Vec<f64>>,
    reference: Option<[f64; 2]>,
) -> PyResult<Vec<u8>> {
    let mut res: Vec<TimedMessage> = msgs_set
        .par_iter()
        .zip(ts_set)
        .map(|(msgs, ts)| {
            msgs.iter()
                .zip(ts)
                .filter_map(|(msg, timestamp)| {
                    let bytes = hex::decode(msg).unwrap();
                    if let Ok((_, message)) = Message::from_bytes((&bytes, 0)) {
                        Some(TimedMessage {
                            timestamp,
                            frame: bytes,
                            message: Some(message),
                            metadata: vec![],
                            decode_time: None,
                        })
                    } else {
                        None
                    }
                })
                .collect()
        })
        .flat_map(|v: Vec<TimedMessage>| v)
        .collect();

    let position = reference.map(|[latitude, longitude]| Position {
        latitude,
        longitude,
    });
    decode_positions(&mut res, position, &None);

    let pkl = serde_pickle::to_vec(&res, Default::default()).unwrap();
    Ok(pkl)
}

#[pyfunction]
fn decode_flarm(
    msg: String,
    ts: u32,
    reflat: f64,
    reflon: f64,
) -> PyResult<Vec<u8>> {
    let bytes = hex::decode(msg).unwrap();
    let reference = [reflat, reflon];
    if let Ok(msg) = Flarm::from_record(ts, &reference, &bytes) {
        let pkl = serde_pickle::to_vec(&msg, Default::default()).unwrap();
        Ok(pkl)
    } else {
        Ok([128, 4, 78, 46].to_vec()) // None
    }
}

#[pyfunction]
fn decode_flarm_vec(
    msgs_set: Vec<Vec<String>>,
    ts_set: Vec<Vec<u32>>,
    ref_lat: Vec<Vec<f64>>,
    ref_lon: Vec<Vec<f64>>,
) -> PyResult<Vec<u8>> {
    let reference: Vec<Vec<[f64; 2]>> = ref_lat
        .iter()
        .zip(ref_lon.iter())
        .map(|(lat, lon)| {
            lat.iter()
                .zip(lon.iter())
                .map(|(lat, lon)| [*lat, *lon])
                .collect()
        })
        .collect();
    let res: Vec<Flarm> = msgs_set
        .par_iter()
        .zip(ts_set)
        .zip(reference)
        .map(|((msgs, ts), reference)| {
            msgs.iter()
                .zip(ts)
                .zip(reference)
                .filter_map(|((msg, timestamp), reference)| {
                    let bytes = hex::decode(msg).unwrap();
                    Flarm::from_record(timestamp, &reference, &bytes).ok()
                })
                .collect()
        })
        .flat_map(|v: Vec<Flarm>| v)
        .collect();

    let pkl = serde_pickle::to_vec(&res, Default::default()).unwrap();
    Ok(pkl)
}

struct WrapAircraftInfo(AircraftInformation);

impl<'a> IntoPyObject<'a> for WrapAircraftInfo {
    type Target = PyDict;
    type Output = Bound<'a, Self::Target>;
    type Error = PyErr;

    fn into_pyobject(
        self,
        py: Python<'a>,
    ) -> Result<Self::Output, Self::Error> {
        let dict = PyDict::new(py);
        dict.set_item("icao24", self.0.icao24)?;
        if let Some(registration) = self.0.registration {
            dict.set_item("registration", registration)?;
        }
        dict.set_item(
            "country",
            self.0.country.or(Some("Unknown".to_string())),
        )?;
        dict.set_item("flag", self.0.flag.or(Some("🏳".to_string())))?;
        if let Some(pattern) = self.0.pattern {
            dict.set_item("pattern", pattern)?;
        }
        if let Some(category) = self.0.category {
            dict.set_item("category", category)?;
        }
        if let Some(comment) = self.0.comment {
            dict.set_item("comment", comment)?;
        }
        Ok(dict)
    }
}

#[pyfunction]
#[pyo3(signature = (icao24, registration=None))]
fn aircraft_information(
    icao24: &str,
    registration: Option<&str>,
) -> PyResult<WrapAircraftInfo> {
    Ok(WrapAircraftInfo(patterns(icao24, registration)?))
}

/// A Python module implemented in Rust.
#[pymodule]
fn _rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Decoding functions
    m.add_function(wrap_pyfunction!(decode_1090, m)?)?;
    m.add_function(wrap_pyfunction!(decode_1090_with_reference, m)?)?;
    m.add_function(wrap_pyfunction!(decode_1090_vec, m)?)?;
    m.add_function(wrap_pyfunction!(decode_1090t_vec, m)?)?;
    m.add_function(wrap_pyfunction!(decode_flarm, m)?)?;
    m.add_function(wrap_pyfunction!(decode_flarm_vec, m)?)?;

    // Comm-B BDS inference
    m.add_function(wrap_pyfunction!(decode_bds05, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds10, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds17, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds18, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds19, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds20, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds21, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds30, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds40, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds44, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds45, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds50, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds60, m)?)?;
    m.add_function(wrap_pyfunction!(decode_bds65, m)?)?;

    // icao24 functions
    m.add_function(wrap_pyfunction!(aircraft_information, m)?)?;

    Ok(())
}
