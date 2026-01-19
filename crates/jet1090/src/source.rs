use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use rs1090::prelude::*;

#[cfg(any(feature = "rtlsdr", feature = "pluto", feature = "soapy"))]
use rs1090::source::iqread;
#[cfg(feature = "sero")]
use rs1090::source::sero;
#[cfg(feature = "ssh")]
use rs1090::source::ssh::{TunnelledTcp, TunnelledWebsocket};

#[cfg(any(feature = "rtlsdr", feature = "pluto", feature = "soapy"))]
use desperado::IqAsyncSource;
#[cfg(feature = "pluto")]
use desperado::pluto::PlutoConfig;
#[cfg(feature = "rtlsdr")]
use desperado::rtlsdr::{DeviceSelector, RtlSdrConfig};
#[cfg(feature = "soapy")]
use desperado::soapy::SoapyConfig;
#[cfg(any(feature = "rtlsdr", feature = "pluto", feature = "soapy"))]
use desperado::{DeviceConfig, Gain};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;
use tracing::error;
use url::Url;

#[cfg(any(feature = "rtlsdr", feature = "pluto", feature = "soapy"))]
const MODES_FREQ: f64 = 1.09e9;
#[cfg(any(feature = "rtlsdr", feature = "pluto", feature = "soapy"))]
const RATE_2_4M: f64 = 2.4e6;

#[cfg(feature = "rtlsdr")]
const RTLSDR_GAIN: f64 = 49.6;

#[cfg(feature = "pluto")]
const PLUTO_GAIN: f64 = 73.0;

/**
* A structure to describe the endpoint to access data.
*
* - The most basic one is a TCP Beast format endpoint (port 30005 for dump1090,
*   port 10003 for Radarcape devices, etc.)
* - If the sensor is not accessible, it is common practice to redirect the
*   Beast feed to a UDP endpoint on another IP address. There is a dedicated
*   setting on Radarcape devices; otherwise, see socat.
* - When the Beast format is sent as UDP, it can be dispatched again as a
*   websocket service: see wsbroad.
*
* ## Example code for setting things up
*
* - Example of socat command to redirect TCP output to UDP endpoint:  
*   `socat TCP:localhost:30005 UDP-DATAGRAM:1.2.3.4:5678`
*
* - Example of wsbroad command:  
*   `wsbroad 0.0.0.0:9876`
*
* - Then, redirect the data:  
*   `websocat -b -u udp-l:127.0.0.1:5678 ws://0.0.0.0:9876/5678`
*
* - Check data is coming:  
*   `websocat ws://localhost:9876/5678`
*
* For Sero Systems, check documentation at <https://doc.sero-systems.de/api/>
*/

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddressStruct {
    address: String,
    port: u16,
    jump: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AddressPath {
    Short(String),
    Long(AddressStruct),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebsocketStruct {
    //address: String,
    //port: u16,
    url: String,
    jump: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WebsocketPath {
    Short(String),
    Long(WebsocketStruct),
}

/// Structured RTL-SDR device configuration for TOML
#[cfg(feature = "rtlsdr")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RtlSdrPath {
    #[serde(flatten)]
    pub config: RtlSdrDeviceConfig,
}

/// RTL-SDR device configuration fields
#[cfg(feature = "rtlsdr")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RtlSdrDeviceConfig {
    /// Device index (0, 1, 2, ...)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<usize>,
    /// Serial number filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// Manufacturer filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    /// Product filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
}

/// Helper struct for deserializing PlutoSDR configuration from TOML
#[cfg(feature = "pluto")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlutoPath {
    /// PlutoSDR URI (IP address, USB device, or full URI like "ip:192.168.2.1" or "usb:1")
    pub pluto: String,
}

/// Helper struct for deserializing SoapySDR configuration from TOML
#[cfg(feature = "soapy")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SoapyPath {
    /// SoapySDR driver arguments (e.g., "driver=rtlsdr")
    pub soapy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Address {
    /// Address to a TCP feed for Beast format (typically port 10003 or 30005), e.g. `localhost:10003`
    Tcp(AddressPath),
    /// Address to a UDP feed for Beast format (socat or dedicated configuration in jetvision interface), e.g. `:1234`
    Udp(String),
    /// Address to a websocket feed, e.g. `ws://localhost:9876/1234`
    Websocket(WebsocketPath),
    /// An RTL-SDR device, e.g. `rtlsdr://` or `rtlsdr://serial=00000001`
    #[cfg(feature = "rtlsdr")]
    Rtlsdr(RtlSdrPath),
    /// A PlutoSDR device, e.g. `pluto://192.168.2.1` or `pluto://ip:192.168.2.1` or `pluto://usb:1`
    #[cfg(feature = "pluto")]
    Pluto(PlutoPath),
    /// A SoapySDR device, e.g. `soapy://driver=rtlsdr`
    #[cfg(feature = "soapy")]
    Soapy(SoapyPath),
    /// A token-based access to Sero Systems (require feature `sero`).
    Sero(SeroParams),
}

/**
 * Describe sources of raw ADS-B data.
 *
 * Several sensors can be behind a single source of data.
 * Optionally, give it a name (an alias) to spot it easily in decoded data.
 */
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    /// The address to the raw ADS-B data feed
    #[serde(flatten)]
    pub address: Address,
    /// An (optional) alias for the source name (only for single sensors)
    pub name: Option<String>,
    /// Latitude of the source (alternative to airport)
    pub latitude: Option<f64>,
    /// Longitude of the source (alternative to airport)
    pub longitude: Option<f64>,
    /// Airport code to set latitude/longitude (alternative to explicit coordinates)
    #[serde(skip_serializing)]
    pub airport: Option<String>,
    /// Localize the source of data, altitude (in m, WGS84 height)
    pub altitude: Option<f64>,
    /// Gain setting for SDR devices (RTL-SDR default: 49.6, PlutoSDR default: 73.0)
    #[cfg(any(feature = "rtlsdr", feature = "pluto", feature = "soapy"))]
    pub gain: Option<f64>,
}

// Custom deserializer to validate mutually exclusive fields
impl<'de> Deserialize<'de> for Source {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SourceHelper {
            #[serde(flatten)]
            address: Address,
            name: Option<String>,
            latitude: Option<f64>,
            longitude: Option<f64>,
            airport: Option<String>,
            altitude: Option<f64>,
            #[cfg(any(
                feature = "rtlsdr",
                feature = "pluto",
                feature = "soapy"
            ))]
            gain: Option<f64>,
        }

        let helper = SourceHelper::deserialize(deserializer)?;

        // Validate mutually exclusive position fields
        let has_coords =
            helper.latitude.is_some() || helper.longitude.is_some();
        let has_airport = helper.airport.is_some();

        if has_coords && has_airport {
            return Err(de::Error::custom(
                "Cannot specify both airport and latitude/longitude. Use either airport code OR explicit coordinates, not both.",
            ));
        }

        // Validate that if one coordinate is provided, both must be provided
        if helper.latitude.is_some() != helper.longitude.is_some() {
            return Err(de::Error::custom(
                "Both latitude and longitude must be specified together",
            ));
        }

        Ok(Source {
            address: helper.address,
            name: helper.name,
            latitude: helper.latitude,
            longitude: helper.longitude,
            airport: helper.airport,
            altitude: helper.altitude,
            #[cfg(any(
                feature = "rtlsdr",
                feature = "pluto",
                feature = "soapy"
            ))]
            gain: helper.gain,
        })
    }
}

impl Source {
    /// Get the position reference, resolving airport code if needed
    pub fn reference(&self) -> Option<Position> {
        if let (Some(lat), Some(lon)) = (self.latitude, self.longitude) {
            Some(Position {
                latitude: lat,
                longitude: lon,
            })
        } else if let Some(ref airport) = self.airport {
            Position::from_str(airport).ok()
        } else {
            None
        }
    }
}

fn build_serial(input: &str) -> u64 {
    // Create a hasher
    let mut hasher = DefaultHasher::new();
    // Hash the string
    input.hash(&mut hasher);
    // Get the hash as a u64
    hasher.finish()
}

impl FromStr for Source {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.replace("@", "?"); // retro-compatibility
        let default_tcp = Url::parse("tcp://").unwrap();

        let url = default_tcp.join(&s).map_err(|e| e.to_string())?;

        let address = match url.scheme() {
            "tcp" => Address::Tcp(AddressPath::Short(format!(
                "{}:{}",
                url.host_str().unwrap_or("0.0.0.0"),
                match url.host() {
                    Some(_) => url.port_or_known_default().unwrap_or(10003),
                    None => {
                        // deals with ":4003?LFBO" (parsed as "tcp:///:4003?LFBO")
                        url.path()
                            .strip_prefix("/:")
                            .unwrap()
                            .parse::<u16>()
                            .expect("A port number was expected")
                    }
                }
            ))),
            "udp" => Address::Udp(format!(
                "{}:{}",
                url.host_str().unwrap_or("0.0.0.0"),
                url.port_or_known_default().unwrap()
            )),
            #[cfg(feature = "rtlsdr")]
            "rtlsdr" => {
                // Parse CLI argument and convert to structured config
                let device_str = url.host_str().unwrap_or("");

                let config = if device_str.is_empty() {
                    // Default to device 0
                    RtlSdrDeviceConfig {
                        device: Some(0),
                        serial: None,
                        manufacturer: None,
                        product: None,
                    }
                } else if let Ok(idx) = device_str.parse::<usize>() {
                    // Numeric string -> device index
                    RtlSdrDeviceConfig {
                        device: Some(idx),
                        serial: None,
                        manufacturer: None,
                        product: None,
                    }
                } else if let Some(serial) = device_str.strip_prefix("serial=")
                {
                    // Serial number format
                    RtlSdrDeviceConfig {
                        device: None,
                        serial: Some(serial.to_string()),
                        manufacturer: None,
                        product: None,
                    }
                } else {
                    // Unknown format, warn and default to device 0
                    eprintln!(
                        "WARNING: Unrecognized RTL-SDR device format: '{}'\n\
                         Expected device index (0, 1, 2, ...) or 'serial=XXXXXXXX'.\n\
                         Defaulting to device 0.",
                        device_str
                    );
                    RtlSdrDeviceConfig {
                        device: Some(0),
                        serial: None,
                        manufacturer: None,
                        product: None,
                    }
                };

                Address::Rtlsdr(RtlSdrPath { config })
            }
            #[cfg(feature = "pluto")]
            "pluto" => {
                // pluto://192.168.2.1 -> just the IP
                // pluto://ip:192.168.2.1 -> ip:192.168.2.1
                // pluto:///usb:1.18.5 -> usb:1.18.5 (triple slash for URIs with colons)
                let uri = match url.host_str() {
                    Some(host) if !host.is_empty() => host.to_string(),
                    _ => {
                        // No host, try path component (for pluto:///usb:1.18.5)
                        let path = url.path();
                        if path.starts_with('/') && path.len() > 1 {
                            path[1..].to_string()
                        } else {
                            return Err("pluto:// requires a URI (IP address, ip:address, or usb:device). Use pluto:///usb:1.18.5 for USB devices with version numbers.".to_string());
                        }
                    }
                };
                Address::Pluto(PlutoPath { pluto: uri })
            }
            #[cfg(feature = "soapy")]
            "soapy" => {
                // soapy://driver=rtlsdr
                let args = url.host_str().unwrap_or("");
                Address::Soapy(SoapyPath {
                    soapy: args.to_string(),
                })
            }
            "ws" => Address::Websocket(WebsocketPath::Short(format!(
                "ws://{}:{}/{}",
                url.host_str().unwrap_or("0.0.0.0"),
                url.port_or_known_default().unwrap(),
                url.path().strip_prefix("/").unwrap()
            ))),
            _ => return Err("unsupported scheme".to_string()),
        };

        let mut source = Source {
            address,
            name: None,
            latitude: None,
            longitude: None,
            airport: None,
            altitude: None,
            #[cfg(any(
                feature = "rtlsdr",
                feature = "pluto",
                feature = "soapy"
            ))]
            gain: None,
        };

        if let Some(query) = url.query() {
            // Parse query parameters
            // Supports: ?LFBO, ?gain=40, ?LFBO&gain=40, ?gain=40&LFBO
            let mut airport_code = None;

            for param in query.split('&') {
                if let Some(gain_str) = param.strip_prefix("gain=") {
                    // Parse gain value
                    if let Ok(gain_val) = gain_str.parse::<f64>() {
                        #[cfg(any(
                            feature = "rtlsdr",
                            feature = "pluto",
                            feature = "soapy"
                        ))]
                        {
                            source.gain = Some(gain_val);
                        }
                    }
                } else if !param.is_empty() {
                    // Assume it's an airport code if not a key=value parameter
                    if !param.contains('=') {
                        airport_code = Some(param);
                    }
                }
            }

            // Try to parse airport code if found
            if let Some(code) = airport_code {
                if let Ok(pos) = Position::from_str(code) {
                    source.latitude = Some(pos.latitude);
                    source.longitude = Some(pos.longitude);
                }
            }
        };

        Ok(source)
    }
}

impl Source {
    pub fn serial(&self) -> u64 {
        match &self.address {
            Address::Tcp(address) => {
                let name = match address {
                    AddressPath::Short(s) => s.clone(),
                    AddressPath::Long(AddressStruct {
                        address, port, ..
                    }) => {
                        format!("{address}:{port}")
                    }
                };
                build_serial(&name)
            }
            Address::Udp(name) => build_serial(name),
            Address::Websocket(address) => {
                let name = match address {
                    WebsocketPath::Short(s) => s.clone(),
                    WebsocketPath::Long(WebsocketStruct { url, .. }) => {
                        url.clone()
                    }
                };
                build_serial(&name)
            }
            #[cfg(feature = "rtlsdr")]
            Address::Rtlsdr(path) => {
                let device_str = if let Some(idx) = path.config.device {
                    idx.to_string()
                } else if let Some(ref serial) = path.config.serial {
                    format!("serial={}", serial)
                } else {
                    "0".to_string()
                };
                build_serial(&format!("rtlsdr:{}", device_str))
            }
            #[cfg(feature = "pluto")]
            Address::Pluto(pluto_path) => {
                build_serial(&format!("pluto:{}", pluto_path.pluto))
            }
            #[cfg(feature = "soapy")]
            Address::Soapy(soapy_path) => {
                build_serial(&format!("soapy:{}", soapy_path.soapy))
            }
            Address::Sero(_) => 0,
        }
    }

    /**
     * Start an async task that listens to data and redirects it to a queue.
     * Messages will have a serial number and a name attached.
     *
     * The next step will be deduplication.
     */
    pub fn receiver(
        &self,
        tx: Sender<TimedMessage>,
        serial: u64,
        name: Option<String>,
    ) {
        match &self.address {
            #[cfg(feature = "rtlsdr")]
            Address::Rtlsdr(path) => {
                // Convert RtlSdrDeviceConfig to DeviceSelector
                let config = &path.config;
                let device = if let Some(idx) = config.device {
                    // Device index specified
                    DeviceSelector::Index(idx)
                } else if config.serial.is_some()
                    || config.manufacturer.is_some()
                    || config.product.is_some()
                {
                    // At least one filter specified
                    DeviceSelector::Filter {
                        manufacturer: config.manufacturer.clone(),
                        product: config.product.clone(),
                        serial: config.serial.clone(),
                    }
                } else {
                    // Empty config, default to device 0
                    DeviceSelector::Index(0)
                };

                // Use gain from config or default to 49.6 for RTL-SDR
                let gain_value = self.gain.unwrap_or(RTLSDR_GAIN);

                tokio::spawn(async move {
                    let rtlsdr_config = RtlSdrConfig {
                        device,
                        center_freq: MODES_FREQ as u32,
                        sample_rate: RATE_2_4M as u32,
                        gain: Gain::Manual(gain_value),
                        bias_tee: false,
                    };
                    let config = DeviceConfig::RtlSdr(rtlsdr_config);
                    let source = IqAsyncSource::from_device_config(&config)
                        .await
                        .expect("Failed to create RTL-SDR source");
                    iqread::receiver(tx, source, serial, RATE_2_4M, name).await
                });
            }
            #[cfg(feature = "pluto")]
            Address::Pluto(pluto_path) => {
                let mut uri = pluto_path.pluto.clone();

                // The pluto-sdr library requires URIs in the format "ip:..." or "usb:..."
                // If the URI doesn't already have a prefix, assume it's an IP and add "ip:"
                if !uri.starts_with("ip:") && !uri.starts_with("usb:") {
                    uri = format!("ip:{}", uri);
                }

                // Use gain from config or default to 50.0 for PlutoSDR
                let gain_value = self.gain.unwrap_or(PLUTO_GAIN);

                tokio::spawn(async move {
                    let pluto_config = PlutoConfig {
                        uri,
                        center_freq: MODES_FREQ as i64,
                        sample_rate: RATE_2_4M as i64,
                        gain: Gain::Manual(gain_value),
                    };
                    let config = DeviceConfig::Pluto(pluto_config);
                    let source = IqAsyncSource::from_device_config(&config)
                        .await
                        .expect("Failed to create PlutoSDR source");
                    iqread::receiver(tx, source, serial, RATE_2_4M, name).await
                });
            }
            #[cfg(feature = "soapy")]
            Address::Soapy(soapy_path) => {
                let args = soapy_path.soapy.clone();

                // Use gain from config or default to 49.6 for SoapySDR (same as RTL-SDR)
                let gain_value = self.gain.unwrap_or(RTLSDR_GAIN);

                tokio::spawn(async move {
                    let soapy_config = SoapyConfig {
                        args,
                        center_freq: MODES_FREQ,
                        sample_rate: RATE_2_4M,
                        channel: 0,
                        gain: Gain::Manual(gain_value),
                        gain_element: "TUNER".to_string(),
                    };
                    let config = DeviceConfig::Soapy(soapy_config);
                    let source = IqAsyncSource::from_device_config(&config)
                        .await
                        .expect("Failed to create SoapySDR source");
                    iqread::receiver(tx, source, serial, RATE_2_4M, name).await
                });
            }
            Address::Sero(sero) => {
                #[cfg(not(feature = "sero"))]
                {
                    error!(
                        "Compile jet1090 with the sero feature, {:?} argument ignored",
                        sero
                    );
                }
                #[cfg(feature = "sero")]
                {
                    let client = sero::SeroClient::from(sero);
                    tokio::spawn(async move {
                        if let Err(e) = sero::receiver(client, tx).await {
                            error!("{}", e.to_string());
                        }
                    });
                }
            }
            _ => {
                let server_address = match &self.address {
                    Address::Tcp(address) => match address {
                        AddressPath::Short(s) => {
                            beast::BeastSource::Tcp(s.to_owned())
                        }
                        #[cfg(not(feature = "ssh"))]
                        AddressPath::Long(AddressStruct {
                            address,
                            port,
                            ..
                        }) => beast::BeastSource::Tcp(format!(
                            "{}:{}",
                            address, port
                        )),
                        #[cfg(feature = "ssh")]
                        AddressPath::Long(AddressStruct {
                            address,
                            port,
                            jump: None,
                        }) => {
                            beast::BeastSource::Tcp(format!("{address}:{port}"))
                        }
                        #[cfg(feature = "ssh")]
                        AddressPath::Long(AddressStruct {
                            address,
                            port,
                            jump: Some(jump),
                        }) => beast::BeastSource::TunnelledTcp(TunnelledTcp {
                            address: address.to_owned(),
                            port: *port,
                            jump: jump.to_owned(),
                        }),
                    },
                    Address::Udp(s) => beast::BeastSource::Udp(s.to_owned()),
                    Address::Websocket(address) => match address {
                        WebsocketPath::Short(s) => {
                            beast::BeastSource::Websocket(s.to_owned())
                        }
                        #[cfg(not(feature = "ssh"))]
                        WebsocketPath::Long(WebsocketStruct {
                            url, ..
                        }) => beast::BeastSource::Websocket(url.to_owned()),
                        #[cfg(feature = "ssh")]
                        WebsocketPath::Long(WebsocketStruct {
                            url,
                            jump: None,
                            ..
                        }) => beast::BeastSource::Websocket(url.to_owned()),
                        #[cfg(feature = "ssh")]
                        WebsocketPath::Long(WebsocketStruct {
                            url,
                            jump: Some(jump),
                        }) => {
                            let parsed_url = Url::parse(url).unwrap();
                            beast::BeastSource::TunnelledWebsocket(
                                TunnelledWebsocket {
                                    address: parsed_url
                                        .host_str()
                                        .unwrap()
                                        .to_owned(),
                                    port: parsed_url
                                        .port_or_known_default()
                                        .unwrap(),
                                    url: url.to_owned(),
                                    jump: jump.to_owned(),
                                },
                            )
                        }
                    },
                    _ => unreachable!(),
                };
                tokio::spawn(async move {
                    if let Err(e) =
                        beast::receiver(server_address, tx, serial, name).await
                    {
                        error!("{}", e.to_string());
                    }
                });
            }
        }
    }
}

/// An intermediate structure defined so that you can keep your Sero entries in
/// your configuration file even if the sero feature is not activated
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeroParams {
    /// The access token
    pub token: String,
    /// Filter on DF messages to receive (default: all)
    pub df_filter: Option<Vec<u32>>,
    /// Filter on messages coming from a set of aircraft (default: all)
    pub aircraft_filter: Option<Vec<u32>>,
    /// Filter on sensor aliases (default: all)
    pub sensor_filter: Option<Vec<String>>,
    /// Jump to a different server (default: none)
    pub jump: Option<String>,
}

#[cfg(feature = "sero")]
impl From<&SeroParams> for sero::SeroClient {
    fn from(value: &SeroParams) -> Self {
        // TODO fallback to SERO_TOKEN environment variable
        // std::env::var("SERO_TOKEN")?
        sero::SeroClient {
            token: value.token.clone(),
            df_filter: value.df_filter.clone().unwrap_or_default(),
            aircraft_filter: value.aircraft_filter.clone().unwrap_or_default(),
            sensor_filter: value.sensor_filter.clone().unwrap_or_default(),
            jump: value.jump.clone(),
        }
    }
}
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_source() {
        #[cfg(feature = "rtlsdr")]
        {
            let source = Source::from_str("rtlsdr:");
            assert!(source.is_ok());
            if let Ok(Source { address, .. }) = source {
                assert!(matches!(address, Address::Rtlsdr(_)));
            }

            let source = Source::from_str("rtlsdr://serial=00000001");
            assert!(source.is_ok());
            if let Ok(Source { address, .. }) = source {
                assert!(matches!(address, Address::Rtlsdr(_)));
            }

            let source = Source::from_str("rtlsdr:@LFBO");
            assert!(source.is_ok());
            if let Ok(Source {
                address,
                name,
                latitude,
                longitude,
                ..
            }) = source
            {
                assert!(matches!(address, Address::Rtlsdr(_)));
                assert_eq!(name, None);
                assert_eq!(latitude, Some(43.628101));
                assert_eq!(longitude, Some(1.367263));
            }
        }

        #[cfg(feature = "pluto")]
        {
            // Test PlutoSDR with IP address
            let source = Source::from_str("pluto://192.168.2.1");
            assert!(source.is_ok());
            if let Ok(Source { address, .. }) = source {
                match address {
                    Address::Pluto(path) => {
                        assert_eq!(path.pluto, "192.168.2.1");
                    }
                    _ => unreachable!(),
                }
            }

            // Test PlutoSDR with hostname
            let source = Source::from_str("pluto://pluto.local");
            assert!(
                source.is_ok(),
                "Failed to parse pluto://pluto.local: {:?}",
                source.err()
            );
            if let Ok(Source { address, .. }) = source {
                match address {
                    Address::Pluto(path) => {
                        assert_eq!(path.pluto, "pluto.local");
                    }
                    _ => unreachable!(),
                }
            }

            // Test PlutoSDR with explicit ip: prefix (use triple slash for URIs with colons)
            let source = Source::from_str("pluto:///ip:192.168.2.1");
            assert!(
                source.is_ok(),
                "Failed to parse pluto:///ip:192.168.2.1: {:?}",
                source.err()
            );
            if let Ok(Source { address, .. }) = source {
                match address {
                    Address::Pluto(path) => {
                        assert_eq!(path.pluto, "ip:192.168.2.1");
                    }
                    _ => unreachable!(),
                }
            }

            // Test PlutoSDR with explicit ip: prefix and hostname
            let source = Source::from_str("pluto:///ip:pluto.local");
            assert!(
                source.is_ok(),
                "Failed to parse pluto:///ip:pluto.local: {:?}",
                source.err()
            );
            if let Ok(Source { address, .. }) = source {
                match address {
                    Address::Pluto(path) => {
                        assert_eq!(path.pluto, "ip:pluto.local");
                    }
                    _ => unreachable!(),
                }
            }

            // Test PlutoSDR with USB using triple slash (for URIs with colons)
            let source = Source::from_str("pluto:///usb:1.18.5");
            assert!(source.is_ok());
            if let Ok(Source { address, .. }) = source {
                match address {
                    Address::Pluto(path) => {
                        assert_eq!(path.pluto, "usb:1.18.5");
                    }
                    _ => unreachable!(),
                }
            }

            // Test PlutoSDR with simple USB
            let source = Source::from_str("pluto:///usb:");
            assert!(source.is_ok());
            if let Ok(Source { address, .. }) = source {
                match address {
                    Address::Pluto(path) => {
                        assert_eq!(path.pluto, "usb:");
                    }
                    _ => unreachable!(),
                }
            }
        }

        let source = Source::from_str("http://default");
        assert!(source.is_err());

        let source = Source::from_str(":4003");
        assert!(source.is_ok());
        if let Ok(Source {
            address: Address::Tcp(path),
            name,
            latitude,
            longitude,
            ..
        }) = source
        {
            assert_eq!(path, AddressPath::Short("0.0.0.0:4003".to_string()));
            assert_eq!(name, None);
            assert_eq!(latitude, None);
            assert_eq!(longitude, None);
        }

        let source = Source::from_str(":4003?LFBO");
        assert!(source.is_ok());
        if let Ok(Source {
            address: Address::Tcp(path),
            name,
            latitude,
            longitude,
            ..
        }) = source
        {
            assert_eq!(path, AddressPath::Short("0.0.0.0:4003".to_string()));
            assert_eq!(name, None);
            assert_eq!(latitude, Some(43.628101));
            assert_eq!(longitude, Some(1.367263));
        }

        let source = Source::from_str("ws://1.2.3.4:4003/get?LFBO");
        assert!(source.is_ok());
        if let Ok(Source {
            address,
            name,
            latitude,
            longitude,
            ..
        }) = source
        {
            assert_eq!(
                address,
                Address::Websocket(WebsocketPath::Short(
                    "ws://1.2.3.4:4003/get".to_string()
                ))
            );
            assert_eq!(name, None);
            assert_eq!(latitude, Some(43.628101));
            assert_eq!(longitude, Some(1.367263));
        }
    }

    #[test]
    fn test_toml_deserialization() {
        // Test RTL-SDR deserialization - structured format with device index
        #[cfg(feature = "rtlsdr")]
        {
            let toml = r#"
                rtlsdr = { device = 0 }
                latitude = 43.5993189
                longitude = 1.4362472
            "#;
            let source: Source = toml::from_str(toml)
                .expect("Failed to parse structured TOML with device");
            assert!(matches!(source.address, Address::Rtlsdr(_)));
            if let Address::Rtlsdr(path) = &source.address {
                assert_eq!(path.config.device, Some(0));
                assert_eq!(path.config.serial, None);
            } else {
                panic!("Expected Address::Rtlsdr");
            }

            // Test RTL-SDR deserialization - structured format with serial
            let toml = r#"
                rtlsdr = { serial = "00000001" }
                latitude = 43.5993189
                longitude = 1.4362472
            "#;
            let source: Source = toml::from_str(toml)
                .expect("Failed to parse structured TOML with serial");
            assert!(matches!(source.address, Address::Rtlsdr(_)));
            if let Address::Rtlsdr(path) = &source.address {
                assert_eq!(path.config.device, None);
                assert_eq!(path.config.serial, Some("00000001".to_string()));
            } else {
                panic!("Expected Address::Rtlsdr");
            }

            // Test RTL-SDR deserialization - structured format with all filters
            let toml = r#"
                rtlsdr = { serial = "00000001", manufacturer = "Realtek", product = "RTL2838UHIDIR" }
                latitude = 43.5993189
                longitude = 1.4362472
            "#;
            let source: Source = toml::from_str(toml)
                .expect("Failed to parse structured TOML with filters");
            assert!(matches!(source.address, Address::Rtlsdr(_)));
            if let Address::Rtlsdr(path) = &source.address {
                assert_eq!(path.config.device, None);
                assert_eq!(path.config.serial, Some("00000001".to_string()));
                assert_eq!(
                    path.config.manufacturer,
                    Some("Realtek".to_string())
                );
                assert_eq!(
                    path.config.product,
                    Some("RTL2838UHIDIR".to_string())
                );
            } else {
                panic!("Expected Address::Rtlsdr");
            }
        }

        // Test PlutoSDR deserialization
        #[cfg(feature = "pluto")]
        {
            // Test IP address format
            let toml = r#"
                name = "my-pluto"
                pluto = "192.168.2.1"
            "#;
            let source: Source =
                toml::from_str(toml).expect("Failed to parse TOML");
            assert!(matches!(source.address, Address::Pluto(_)));
            if let Address::Pluto(path) = &source.address {
                assert_eq!(path.pluto, "192.168.2.1");
            }
            assert_eq!(source.name, Some("my-pluto".to_string()));

            // Test ip: prefix format
            let toml = r#"
                pluto = "ip:192.168.2.1"
            "#;
            let source: Source =
                toml::from_str(toml).expect("Failed to parse TOML");
            assert!(matches!(source.address, Address::Pluto(_)));
            if let Address::Pluto(path) = &source.address {
                assert_eq!(path.pluto, "ip:192.168.2.1");
            }

            // Test usb: format
            let toml = r#"
                pluto = "usb:"
            "#;
            let source: Source =
                toml::from_str(toml).expect("Failed to parse TOML");
            assert!(matches!(source.address, Address::Pluto(_)));
            if let Address::Pluto(path) = &source.address {
                assert_eq!(path.pluto, "usb:");
            }
        }

        // Test SoapySDR deserialization
        #[cfg(feature = "soapy")]
        {
            let toml = r#"
                soapy = "driver=rtlsdr"
            "#;
            let source: Source =
                toml::from_str(toml).expect("Failed to parse TOML");
            assert!(matches!(source.address, Address::Soapy(_)));
            if let Address::Soapy(path) = &source.address {
                assert_eq!(path.soapy, "driver=rtlsdr");
            }
        }

        // Test TCP deserialization (should work regardless of features)
        let toml = r#"
            tcp = "localhost:10003"
            name = "local-beast"
        "#;
        let source: Source =
            toml::from_str(toml).expect("Failed to parse TOML");
        assert!(matches!(source.address, Address::Tcp(_)));
        assert_eq!(source.name, Some("local-beast".to_string()));
    }

    #[test]
    fn test_invalid_keys_rejected() {
        // Test that typos in field names are rejected (e.g., "gaoain" instead of "gain")
        #[cfg(any(feature = "rtlsdr", feature = "pluto", feature = "soapy"))]
        {
            let toml = r#"
                tcp = "localhost:10003"
                gaoain = 39
            "#;
            let result: Result<Source, _> = toml::from_str(toml);
            assert!(
                result.is_err(),
                "Expected error for typo 'gaoain', but parsing succeeded: {:?}",
                result
            );
            if let Err(e) = result {
                let error_msg = e.to_string();
                assert!(
                    error_msg.contains("unknown field")
                        || error_msg.contains("gaoain"),
                    "Error should mention unknown field, got: {}",
                    error_msg
                );
            }
        }

        // Test that invalid keys in the RTL-SDR device config are rejected
        #[cfg(feature = "rtlsdr")]
        {
            let toml = r#"
                rtlsdr = { device = 0, invalid_param = "bad" }
            "#;
            let result: Result<Source, _> = toml::from_str(toml);
            assert!(
                result.is_err(),
                "Expected error for invalid RTL-SDR field, but got: {:?}",
                result
            );
        }
    }

    #[test]
    #[cfg(feature = "rtlsdr")]
    fn test_gain_configuration() {
        // Test default gain (should be None in the struct, 49.6 will be used at runtime)
        let toml = r#"
            rtlsdr = { device = 0 }
            latitude = 43.5993189
            longitude = 1.4362472
        "#;
        let source: Source =
            toml::from_str(toml).expect("Failed to parse TOML");
        assert_eq!(source.gain, None);

        // Test explicit gain configuration
        let toml = r#"
            rtlsdr = { device = 0 }
            latitude = 43.5993189
            longitude = 1.4362472
            gain = 42.5
        "#;
        let source: Source =
            toml::from_str(toml).expect("Failed to parse TOML with gain");
        assert_eq!(source.gain, Some(42.5));

        // Test gain with serial number selection
        let toml = r#"
            rtlsdr = { serial = "00000001" }
            gain = 30.0
        "#;
        let source: Source = toml::from_str(toml)
            .expect("Failed to parse TOML with serial and gain");
        if let Address::Rtlsdr(path) = &source.address {
            assert_eq!(path.config.serial, Some("00000001".to_string()));
        }
        assert_eq!(source.gain, Some(30.0));
    }

    #[test]
    fn test_mutually_exclusive_position_fields() {
        // Test that airport and latitude/longitude cannot be specified together
        let toml = r#"
            tcp = "localhost:10003"
            airport = "LFBO"
            latitude = 43.628101
            longitude = 1.367263
        "#;
        let result: Result<Source, _> = toml::from_str(toml);
        assert!(
            result.is_err(),
            "Expected error when both airport and coordinates are specified: {:?}",
            result
        );
        if let Err(e) = result {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("airport")
                    || error_msg.contains("latitude")
                    || error_msg.contains("both"),
                "Error should mention conflicting fields, got: {}",
                error_msg
            );
        }

        // Test that latitude without longitude is rejected
        let toml = r#"
            tcp = "localhost:10003"
            latitude = 43.628101
        "#;
        let result: Result<Source, _> = toml::from_str(toml);
        assert!(
            result.is_err(),
            "Expected error when only latitude is specified: {:?}",
            result
        );
        if let Err(e) = result {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("latitude")
                    && error_msg.contains("longitude"),
                "Error should mention both latitude and longitude, got: {}",
                error_msg
            );
        }

        // Test that longitude without latitude is rejected
        let toml = r#"
            tcp = "localhost:10003"
            longitude = 1.367263
        "#;
        let result: Result<Source, _> = toml::from_str(toml);
        assert!(
            result.is_err(),
            "Expected error when only longitude is specified: {:?}",
            result
        );

        // Test that airport alone is valid
        let toml = r#"
            tcp = "localhost:10003"
            airport = "LFBO"
        "#;
        let result: Result<Source, _> = toml::from_str(toml);
        assert!(
            result.is_ok(),
            "Airport alone should be valid: {:?}",
            result
        );

        // Test that latitude+longitude together is valid
        let toml = r#"
            tcp = "localhost:10003"
            latitude = 43.628101
            longitude = 1.367263
        "#;
        let result: Result<Source, _> = toml::from_str(toml);
        assert!(
            result.is_ok(),
            "Latitude+longitude together should be valid: {:?}",
            result
        );
    }

    #[test]
    #[cfg(any(feature = "rtlsdr", feature = "pluto", feature = "soapy"))]
    fn test_gain_in_uri() {
        // Test gain parameter in URI
        let source = Source::from_str("rtlsdr://0?gain=40");
        assert!(
            source.is_ok(),
            "Failed to parse URI with gain: {:?}",
            source
        );
        if let Ok(src) = source {
            assert_eq!(src.gain, Some(40.0));
        }

        // Test gain with airport code (using ? syntax)
        let source = Source::from_str("rtlsdr://0?LFBO&gain=42.5");
        assert!(
            source.is_ok(),
            "Failed to parse URI with airport and gain: {:?}",
            source
        );
        if let Ok(src) = source {
            assert_eq!(src.gain, Some(42.5));
            assert_eq!(src.latitude, Some(43.628101));
            assert_eq!(src.longitude, Some(1.367263));
        }

        // Test gain with airport code (using @ syntax for retro-compatibility)
        let source = Source::from_str("rtlsdr://0@LFBO&gain=42.5");
        assert!(
            source.is_ok(),
            "Failed to parse URI with @ and gain: {:?}",
            source
        );
        if let Ok(src) = source {
            assert_eq!(src.gain, Some(42.5));
            assert_eq!(src.latitude, Some(43.628101));
            assert_eq!(src.longitude, Some(1.367263));
        }

        // Test gain before airport code
        let source = Source::from_str("rtlsdr://0?gain=35&LFBO");
        assert!(
            source.is_ok(),
            "Failed to parse URI with gain before airport: {:?}",
            source
        );
        if let Ok(src) = source {
            assert_eq!(src.gain, Some(35.0));
            assert_eq!(src.latitude, Some(43.628101));
            assert_eq!(src.longitude, Some(1.367263));
        }

        // Test TCP with gain
        let source = Source::from_str("tcp://localhost:10003?gain=30");
        assert!(
            source.is_ok(),
            "Failed to parse TCP URI with gain: {:?}",
            source
        );
        if let Ok(src) = source {
            assert_eq!(src.gain, Some(30.0));
        }

        // Test that invalid gain value is ignored (non-numeric)
        let source = Source::from_str("rtlsdr://0?gain=invalid");
        assert!(source.is_ok(), "Should parse URI even with invalid gain");
        if let Ok(src) = source {
            assert_eq!(src.gain, None); // Invalid gain should be ignored
        }
    }
}
