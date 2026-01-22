use std::collections::BTreeMap;

use rs1090::data::aircraft::Aircraft;
use rs1090::decode::bds::bds09::AirborneVelocitySubType::{
    AirspeedSubsonic, GroundSpeedDecoding,
};
use rs1090::decode::bds::bds09::AirspeedType::{IAS, TAS};
use rs1090::decode::cpr::AircraftState;
use rs1090::decode::{IdentityCode, SensorMetadata, ICAO};
use rs1090::prelude::*;
use serde::Serialize;

/**
 * A state vector with the most up-to-date information about an aircraft
 */
#[derive(Debug, Serialize)]
pub struct Snapshot {
    /// The ICAO 24-bit address of the aircraft transponder
    pub icao24: String,
    /// The timestamp of the first seen message
    pub firstseen: u64,
    /// The timestamp of the last seen message
    pub lastseen: u64,
    /// The callsign of the aircraft, ICAO flight number for commercial aircraft, often matches registration in General Aviation.
    pub callsign: Option<String>,
    /// The tail number of the aircraft. If the aircraft is not known in the local database, some heuristics may reconstruct the tail number in some countries.
    pub registration: Option<String>,
    /// The ICAO code to the type of aircraft, e.g. A32O or B789
    pub typecode: Option<String>,
    /// The squawk code, a 4-digit number set on the transponder, 7700 for general emergencies
    pub squawk: Option<IdentityCode>,
    /// WGS84 latitude angle in degrees
    pub latitude: Option<f64>,
    /// WGS84 longitude angle in degrees
    pub longitude: Option<f64>,
    /// Barometric altitude in feet, expressed in ISA
    /// Can be negative for airports below sea level (e.g., Amsterdam Schiphol)
    /// Can be positive up to ~50,000 ft for cruise altitude
    pub altitude: Option<i32>,
    /// Altitude selected in the FMS
    pub selected_altitude: Option<u16>,
    /// Ground speed, in knots
    pub groundspeed: Option<f64>,
    /// Vertical rate of the aircraft, in feet/min
    pub vertical_rate: Option<i16>,
    /// The true track angle of the aircraft in degrees with respect to the geographic North
    pub track: Option<f64>,
    /// Indicated air speed, in knots
    pub ias: Option<u16>,
    /// True air speed, in knots
    pub tas: Option<u16>,
    /// The Mach number
    pub mach: Option<f64>,
    /// The roll angle of the aircraft in degrees (positive angle for banking to the right-hand side)
    pub roll: Option<f64>,
    /// The magnetic heading of the aircraft in degrees with respect to the magnetic North
    pub heading: Option<f64>,
    /// The NAC position indicator, for uncertainty
    pub nacp: Option<u8>,
    /// Number of messages received for the aircraft
    pub count: usize,
    /// Metadata information from the sensors seeing the aircraft
    pub metadata: Vec<SensorMetadata>,
    /// The ICAO code of the airport where the aircraft is currently located (surface only)
    pub airport: Option<String>,
}

/**
 * Contains information related to an aircraft: current state and history
 */
#[derive(Debug)]
pub struct StateVectors {
    /// The latest state of the aircraft
    pub cur: Snapshot,
    /// The history of received messages
    pub hist: Vec<TimedMessage>,
}

impl StateVectors {
    fn new(
        ts: u64,
        icao24: String,
        aircraftdb: &BTreeMap<String, Aircraft>,
    ) -> StateVectors {
        let hexid = u32::from_str_radix(&icao24, 16).unwrap_or(0);
        let ac = aircraftdb.get(&icao24);
        let typecode = match ac {
            Some(ac) => ac.typecode.to_owned(),
            None => None,
        };
        let mut registration = match ac {
            Some(ac) => ac.registration.to_owned(),
            None => None,
        };
        if registration.is_none() {
            // Heuristics to decode the tail number
            registration = rs1090::data::tail::tail(hexid);
        }

        let cur = Snapshot {
            icao24,
            firstseen: ts,
            lastseen: ts,
            callsign: None,
            registration,
            typecode,
            squawk: None,
            latitude: None,
            longitude: None,
            altitude: None,
            selected_altitude: None,
            groundspeed: None,
            vertical_rate: None,
            track: None,
            ias: None,
            tas: None,
            mach: None,
            roll: None,
            heading: None,
            nacp: None,
            count: 0,
            metadata: vec![],
            airport: None,
        };
        StateVectors {
            cur,
            hist: Vec::<TimedMessage>::new(),
        }
    }
}

fn icao24(msg: &Message) -> Option<String> {
    match &msg.df {
        ShortAirAirSurveillance { ap, .. } => Some(ap.to_string()),
        SurveillanceAltitudeReply { ap, .. } => Some(ap.to_string()),
        SurveillanceIdentityReply { ap, .. } => Some(ap.to_string()),
        AllCallReply { icao, .. } => Some(icao.to_string()),
        LongAirAirSurveillance { ap, .. } => Some(ap.to_string()),
        ExtendedSquitterADSB(ADSB { icao24, .. }) => Some(icao24.to_string()),
        ExtendedSquitterTisB { cf, .. } => Some(cf.aa.to_string()),
        CommBAltitudeReply { ap, .. } => Some(ap.to_string()),
        CommBIdentityReply { ap, .. } => Some(ap.to_string()),
        _ => None,
    }
}

pub async fn update_snapshot(
    shared: &crate::SharedState,
    msg: &mut TimedMessage,
    aircraftdb: &BTreeMap<String, Aircraft>,
    cpr_aircraft: &BTreeMap<ICAO, AircraftState>,
) {
    if let TimedMessage {
        timestamp,
        message: Some(message),
        metadata,
        ..
    } = msg
    {
        if let Some(icao24) = icao24(message) {
            let mut states = shared.state_vectors.write().await;
            let aircraft =
                states
                    .entry(icao24.to_string())
                    .or_insert(StateVectors::new(
                        *timestamp as u64,
                        icao24,
                        aircraftdb,
                    ));
            aircraft.cur.lastseen = *timestamp as u64;
            aircraft.cur.metadata = metadata.to_vec();
            aircraft.cur.count += 1;

            match &mut message.df {
                SurveillanceIdentityReply { id, .. } => {
                    aircraft.cur.squawk = Some(*id)
                }
                SurveillanceAltitudeReply { ac, .. } => {
                    aircraft.cur.altitude = ac.0;
                }
                ExtendedSquitterADSB(adsb) => match &adsb.message {
                    ME::BDS05 { inner: bds05, .. } => {
                        aircraft.cur.latitude = bds05.latitude;
                        aircraft.cur.longitude = bds05.longitude;
                        aircraft.cur.altitude = bds05.alt;
                        // Clear airport when airborne
                        aircraft.cur.airport = None;
                    }
                    ME::BDS06 { inner: bds06, .. } => {
                        aircraft.cur.latitude = bds06.latitude;
                        aircraft.cur.longitude = bds06.longitude;
                        aircraft.cur.track = bds06.track;
                        aircraft.cur.groundspeed = bds06.groundspeed;
                        aircraft.cur.altitude = None;
                        // Extract airport from CPR aircraft state
                        if let Ok(icao) = aircraft.cur.icao24.parse::<ICAO>() {
                            if let Some(state) = cpr_aircraft.get(&icao) {
                                aircraft.cur.airport = state.airport.clone();
                            }
                        }
                    }
                    ME::BDS08 { inner: bds08, .. } => {
                        if !bds08.callsign.contains("#") {
                            aircraft.cur.callsign =
                                Some(bds08.callsign.to_string())
                        }
                    }
                    ME::BDS09(bds09) => {
                        aircraft.cur.vertical_rate = bds09.vertical_rate;
                        match &bds09.velocity {
                            GroundSpeedDecoding(spd) => {
                                aircraft.cur.groundspeed =
                                    Some(spd.groundspeed);
                                aircraft.cur.track = Some(spd.track)
                            }
                            AirspeedSubsonic(spd) => {
                                match spd.airspeed_type {
                                    IAS => aircraft.cur.ias = spd.airspeed,
                                    TAS => aircraft.cur.tas = spd.airspeed,
                                }
                                aircraft.cur.heading = spd.heading;
                            }
                            _ => {}
                        }
                    }
                    ME::BDS61(bds61) => {
                        aircraft.cur.squawk = Some(bds61.squawk);
                    }
                    ME::BDS62(bds62) => {
                        aircraft.cur.selected_altitude =
                            bds62.selected_altitude;
                        aircraft.cur.nacp = Some(bds62.nac_p);
                    }
                    ME::BDS65(bds65) => match bds65 {
                        AircraftOperationStatus::Airborne(st) => {
                            match st.version {
                                rs1090::decode::bds::bds65::ADSBVersionAirborne::DOC9871AppendixB(v) => {
                                    aircraft.cur.nacp = Some(v.nac_p)
                                }
                                rs1090::decode::bds::bds65::ADSBVersionAirborne::DOC9871AppendixC(v) => {
                                    aircraft.cur.nacp = Some(v.nac_p)
                                }
                                _ => {}
                            }
                        }
                        AircraftOperationStatus::Surface(st) => {
                            match st.version {
                                rs1090::decode::bds::bds65::ADSBVersionSurface::DOC9871AppendixB(v) => {
                                    aircraft.cur.nacp = Some(v.nac_p)
                                }
                                rs1090::decode::bds::bds65::ADSBVersionSurface::DOC9871AppendixC(v) => {
                                    aircraft.cur.nacp = Some(v.nac_p)
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                },
                ExtendedSquitterTisB { cf, .. } => {
                    aircraft.cur.typecode = Some("GRND".to_string());
                    match &cf.me {
                        ME::BDS05 { inner: bds05, .. } => {
                            aircraft.cur.latitude = bds05.latitude;
                            aircraft.cur.longitude = bds05.longitude;
                            aircraft.cur.altitude = bds05.alt;
                            // Clear airport when airborne
                            aircraft.cur.airport = None;
                        }
                        ME::BDS06 { inner: bds06, .. } => {
                            aircraft.cur.latitude = bds06.latitude;
                            aircraft.cur.longitude = bds06.longitude;
                            aircraft.cur.track = bds06.track;
                            aircraft.cur.groundspeed = bds06.groundspeed;
                            aircraft.cur.altitude = None;
                            // Extract airport from CPR aircraft state
                            if let Ok(icao) =
                                aircraft.cur.icao24.parse::<ICAO>()
                            {
                                if let Some(state) = cpr_aircraft.get(&icao) {
                                    aircraft.cur.airport =
                                        state.airport.clone();
                                }
                            }
                        }
                        ME::BDS08 { inner: bds08, .. } => {
                            aircraft.cur.callsign =
                                Some(bds08.callsign.to_string())
                        }
                        _ => {}
                    }
                }
                CommBAltitudeReply { bds, .. } => {
                    if let Some(bds20) = &bds.bds20 {
                        if !bds20.callsign.contains("#") {
                            aircraft.cur.callsign =
                                Some(bds20.callsign.to_string());
                        }
                    }
                    if let Some(bds40) = &bds.bds40 {
                        aircraft.cur.selected_altitude =
                            bds40.selected_altitude_mcp;
                    }
                    if let Some(bds50) = &bds.bds50 {
                        aircraft.cur.roll = bds50.roll_angle;
                        aircraft.cur.track = bds50.track_angle;
                        aircraft.cur.groundspeed =
                            bds50.groundspeed.map(|x| x as f64);
                        aircraft.cur.tas = bds50.true_airspeed;
                    }
                    if let Some(bds60) = &bds.bds60 {
                        aircraft.cur.ias = bds60.indicated_airspeed;
                        aircraft.cur.mach = bds60.mach_number;
                        aircraft.cur.heading = bds60.magnetic_heading;
                        if bds60.inertial_vertical_velocity.is_some() {
                            aircraft.cur.vertical_rate =
                                bds60.inertial_vertical_velocity;
                        }
                    }
                }
                CommBIdentityReply { bds, .. } => {
                    if let Some(bds20) = &bds.bds20 {
                        if !bds20.callsign.contains("#") {
                            aircraft.cur.callsign =
                                Some(bds20.callsign.to_string());
                        }
                    }
                    if let Some(bds40) = &bds.bds40 {
                        aircraft.cur.selected_altitude =
                            bds40.selected_altitude_mcp;
                    }
                    if let Some(bds50) = &bds.bds50 {
                        aircraft.cur.roll = bds50.roll_angle;
                        aircraft.cur.track = bds50.track_angle;
                        aircraft.cur.groundspeed =
                            bds50.groundspeed.map(|x| x as f64);
                        aircraft.cur.tas = bds50.true_airspeed;
                    }
                    if let Some(bds60) = &bds.bds60 {
                        aircraft.cur.ias = bds60.indicated_airspeed;
                        aircraft.cur.mach = bds60.mach_number;
                        aircraft.cur.heading = bds60.magnetic_heading;
                        if bds60.inertial_vertical_velocity.is_some() {
                            aircraft.cur.vertical_rate =
                                bds60.inertial_vertical_velocity;
                        }
                    }
                }
                _ => {}
            };
        }
    }
}

pub async fn store_history(
    shared: &crate::SharedState,
    msg: TimedMessage,
    aircraftdb: &BTreeMap<String, Aircraft>,
) {
    if let TimedMessage {
        timestamp,
        message: Some(message),
        metadata,
        decode_time,
        ..
    } = msg
    {
        if let Some(icao24) = icao24(&message) {
            let mut states = shared.state_vectors.write().await;
            let aircraft =
                states
                    .entry(icao24.to_string())
                    .or_insert(StateVectors::new(
                        timestamp as u64,
                        icao24,
                        aircraftdb,
                    ));

            match message.df {
                ExtendedSquitterADSB(_)
                | ExtendedSquitterTisB { .. }
                | CommBAltitudeReply { .. }
                | CommBIdentityReply { .. } => {
                    aircraft.hist.push(TimedMessage {
                        timestamp,
                        frame: vec![],
                        message: Some(message),
                        metadata,
                        decode_time,
                    })
                }
                _ => {}
            }
        }
    }
}
