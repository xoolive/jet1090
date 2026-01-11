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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Address {
    /// Address to a TCP feed for Beast format (typically port 10003 or 30005), e.g. `localhost:10003`
    Tcp(AddressPath),
    /// Address to a UDP feed for Beast format (socat or dedicated configuration in jetvision interface), e.g. `:1234`
    Udp(String),
    /// Address to a websocket feed, e.g. `ws://localhost:9876/1234`
    Websocket(WebsocketPath),
    /// An SDR device (RTL-SDR, PlutoSDR, or SoapySDR) configured via URL, e.g. `rtlsdr://?freq=1090M&rate=2.4M&gain=49.6`
    #[cfg(any(feature = "rtlsdr", feature = "pluto", feature = "soapy"))]
    #[serde(skip)]
    Sdr(desperado::DeviceConfig),
    /// A token-based access to Sero Systems (require feature `sero`).
    Sero(SeroParams),
}

/**
 * Describe sources of raw ADS-B data.
 *
 * Several sensors can be behind a single source of data.
 * Optionally, give it a name (an alias) to spot it easily in decoded data.
 */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    /// The address to the raw ADS-B data feed
    #[serde(flatten)]
    pub address: Address,
    /// An (optional) alias for the source name (only for single sensors)
    pub name: Option<String>,
    /// Localize the source of data (only for single sensors)
    #[serde(flatten)]
    pub reference: Option<Position>,
    /// Localize the source of data, altitude (in m, WGS84 height)
    pub altitude: Option<f64>,
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
                // Build URL with defaults for desperado
                let host = url.host_str().unwrap_or("");
                let device_url = if host.is_empty() {
                    format!(
                        "rtlsdr://?freq={}M&rate={}M&gain={}",
                        (MODES_FREQ / 1e6) as u32,
                        (RATE_2_4M / 1e6) as u32,
                        RTLSDR_GAIN
                    )
                } else {
                    // Legacy format with serial or other args - not supported in new format
                    // Fall back to default device
                    format!(
                        "rtlsdr://?freq={}M&rate={}M&gain={}",
                        (MODES_FREQ / 1e6) as u32,
                        (RATE_2_4M / 1e6) as u32,
                        RTLSDR_GAIN
                    )
                };

                let config = desperado::DeviceConfig::from_str(&device_url)
                    .map_err(|e| {
                        format!("Failed to parse device config: {}", e)
                    })?;
                Address::Sdr(config)
            }
            #[cfg(feature = "pluto")]
            "pluto" | "plutoip" | "plutousb" => {
                // pluto://192.168.2.1 -> plutoip://192.168.2.1?freq=1090M&rate=2.4M&gain=50
                let device_url = match url.scheme() {
                    "pluto" => {
                        let host = url
                            .host_str()
                            .ok_or("pluto:// requires an IP address")?;
                        format!(
                            "plutoip://{}?freq={}M&rate={}M&gain=50",
                            host,
                            (MODES_FREQ / 1e6) as u32,
                            (RATE_2_4M / 1e6) as u32
                        )
                    }
                    "plutoip" | "plutousb" => {
                        // Pass through plutoip and plutousb URLs
                        url.as_str().to_string()
                    }
                    _ => unreachable!(),
                };

                let config = desperado::DeviceConfig::from_str(&device_url)
                    .map_err(|e| {
                        format!("Failed to parse device config: {}", e)
                    })?;
                Address::Sdr(config)
            }
            #[cfg(feature = "soapy")]
            "soapy" => {
                // soapy://driver=rtlsdr -> soapy://driver=rtlsdr?freq=1090M&rate=2.4M&gain=49.6
                let args = url.host_str().unwrap_or("");
                let device_url = format!(
                    "soapy://{}?freq={}M&rate={}M&gain=49.6",
                    args,
                    (MODES_FREQ / 1e6) as u32,
                    (RATE_2_4M / 1e6) as u32
                );

                let config = desperado::DeviceConfig::from_str(&device_url)
                    .map_err(|e| {
                        format!("Failed to parse device config: {}", e)
                    })?;
                Address::Sdr(config)
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
            reference: None,
            altitude: None,
        };

        if let Some(query) = url.query() {
            source.reference = Position::from_str(query).ok()
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
            #[cfg(any(
                feature = "rtlsdr",
                feature = "pluto",
                feature = "soapy"
            ))]
            Address::Sdr(config) => {
                // Generate a serial based on the device config
                let name = match config {
                    #[cfg(feature = "rtlsdr")]
                    desperado::DeviceConfig::RtlSdr(cfg) => {
                        format!("rtlsdr:{}", cfg.center_freq)
                    }
                    #[cfg(feature = "pluto")]
                    desperado::DeviceConfig::Pluto(cfg) => {
                        format!("pluto:{}", cfg.uri)
                    }
                    #[cfg(feature = "soapy")]
                    desperado::DeviceConfig::Soapy(cfg) => {
                        format!("soapy:{}", cfg.args)
                    }
                };
                build_serial(&name)
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
            #[cfg(any(
                feature = "rtlsdr",
                feature = "pluto",
                feature = "soapy"
            ))]
            Address::Sdr(config) => {
                // Check which SDR feature is needed based on the config
                #[cfg(not(any(
                    feature = "rtlsdr",
                    feature = "pluto",
                    feature = "soapy"
                )))]
                {
                    error!(
                        "Compile jet1090 with rtlsdr, pluto, or soapy feature for SDR support"
                    );
                    std::process::exit(127);
                }

                #[cfg(any(
                    feature = "rtlsdr",
                    feature = "pluto",
                    feature = "soapy"
                ))]
                {
                    let config = config.clone();
                    tokio::spawn(async move {
                        let source = IqAsyncSource::from_device_config(&config)
                            .await
                            .expect("Failed to create SDR source");
                        iqread::receiver(tx, source, serial, 2.4e6, name).await
                    });
                }
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
                assert!(matches!(
                    address,
                    Address::Sdr(desperado::DeviceConfig::RtlSdr(_))
                ));
            }

            let source = Source::from_str("rtlsdr://serial=00000001");
            assert!(source.is_ok());
            if let Ok(Source { address, .. }) = source {
                assert!(matches!(
                    address,
                    Address::Sdr(desperado::DeviceConfig::RtlSdr(_))
                ));
            }

            let source = Source::from_str("rtlsdr:@LFBO");
            assert!(source.is_ok());
            if let Ok(Source {
                address,
                name,
                reference: Some(pos),
                ..
            }) = source
            {
                assert!(matches!(
                    address,
                    Address::Sdr(desperado::DeviceConfig::RtlSdr(_))
                ));
                assert_eq!(name, None);
                assert_eq!(pos.latitude, 43.628101);
                assert_eq!(pos.longitude, 1.367263);
            }
        }

        let source = Source::from_str("http://default");
        assert!(source.is_err());

        let source = Source::from_str(":4003");
        assert!(source.is_ok());
        if let Ok(Source {
            address: Address::Tcp(path),
            name,
            reference,
            ..
        }) = source
        {
            assert_eq!(path, AddressPath::Short("0.0.0.0:4003".to_string()));
            assert_eq!(name, None);
            assert_eq!(reference, None);
        }

        let source = Source::from_str(":4003?LFBO");
        assert!(source.is_ok());
        if let Ok(Source {
            address: Address::Tcp(path),
            name,
            reference: Some(pos),
            ..
        }) = source
        {
            assert_eq!(path, AddressPath::Short("0.0.0.0:4003".to_string()));
            assert_eq!(name, None);
            assert_eq!(pos.latitude, 43.628101);
            assert_eq!(pos.longitude, 1.367263);
        }

        let source = Source::from_str("ws://1.2.3.4:4003/get?LFBO");
        assert!(source.is_ok());
        if let Ok(Source {
            address,
            name,
            reference: Some(pos),
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
            assert_eq!(pos.latitude, 43.628101);
            assert_eq!(pos.longitude, 1.367263);
        }
    }
}
