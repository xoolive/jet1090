use rs1090::prelude::*;

#[cfg(feature = "sero")]
use rs1090::source::sero;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::source::{Address, Source};

/**
 * A structure to describe information to label data produced by a sensor.
 */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sensor {
    /// The serial number is in general a hash of the Address structure
    pub serial: u64,
    /// An (optional) alias to label the sensor
    pub name: Option<String>,
    /// An (optional) position to decode ground messages (DF=18 or 17, BDS=0,6)
    pub reference: Option<Position>,
    /// An (optional) position altitude (in m, WGS84 height)
    pub altitude: Option<f64>,
    /// Number of aircraft the sensor currently sees, computed on `/sensors`
    /// requests
    pub aircraft_count: u64,
    /// System timestamp in seconds of the last message from the sensor,
    /// computed on `/sensors` requests
    pub last_timestamp: u64,
}

/**
 * Create a sensor or a list of sensors based on a source information.
 */
pub async fn sensors(value: &Source) -> Result<Vec<Sensor>, String> {
    let local_sensor = || {
        Ok(vec![Sensor {
            serial: value.serial(),
            name: value.name.clone(),
            reference: value.reference(),
            altitude: value.altitude,
            aircraft_count: 0,
            last_timestamp: 0,
        }])
    };

    match &value.address {
        Address::Tcp(_) | Address::Udp(_) | Address::Websocket(_) => {
            local_sensor()
        }
        #[cfg(feature = "sdr")]
        Address::File(_) => local_sensor(),
        #[cfg(feature = "rtlsdr")]
        Address::Rtlsdr(_) => local_sensor(),
        #[cfg(feature = "airspy")]
        Address::Airspy(_) => local_sensor(),
        #[cfg(feature = "hackrf")]
        Address::Hackrf(_) => local_sensor(),
        #[cfg(feature = "soapy")]
        Address::Soapy(_) => local_sensor(),
        Address::Sero(params) => {
            #[cfg(feature = "sero")]
            {
                let sero = sero::SeroClient::from(params);
                debug!("send {:?} to collect info", params);
                let info = sero.info().await.map_err(|error| {
                    format!("Sero sensor discovery failed: {error}")
                })?;
                Ok(info
                    .sensor_info
                    .iter()
                    .filter_map(|elt| {
                        let sensor = elt.sensor.as_ref()?;
                        let position = elt.gnss.as_ref().and_then(|gnss| {
                            gnss.position.as_ref().map(|pos| Position {
                                latitude: pos.latitude,
                                longitude: pos.longitude,
                            })
                        });
                        let altitude = elt.gnss.as_ref().and_then(|gnss| {
                            gnss.position.as_ref().map(|pos| pos.height)
                        });
                        Some(Sensor {
                            serial: sensor.serial,
                            reference: position,
                            altitude,
                            name: Some(elt.alias.to_string()),
                            aircraft_count: 0,
                            last_timestamp: 0,
                        })
                    })
                    .collect())
            }
            #[cfg(not(feature = "sero"))]
            {
                debug!("params {:?} unused", params);
                Ok(vec![])
            }
        }
    }
}
