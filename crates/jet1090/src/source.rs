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

/// Helper struct for deserializing RTL-SDR configuration from TOML
#[cfg(feature = "rtlsdr")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RtlSdrPath {
    /// Serial number or device index (empty string for default device)
    pub rtlsdr: String,
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
                Address::Rtlsdr(RtlSdrPath {
                    rtlsdr: host.to_string(),
                })
            }
            #[cfg(feature = "pluto")]
            "pluto" | "plutoip" | "plutousb" => {
                // Accept pluto://192.168.2.1 or plutoip://192.168.2.1 or plutousb://
                // Convert to unified format for TOML storage
                let uri = match url.scheme() {
                    "plutoip" => {
                        // plutoip://192.168.2.1 -> ip:192.168.2.1
                        let host = url
                            .host_str()
                            .ok_or("plutoip:// requires an IP address")?;
                        format!("ip:{}", host)
                    }
                    "plutousb" => {
                        // plutousb:// or plutousb://1 -> usb: or usb:1
                        let host = url.host_str().unwrap_or("");
                        if host.is_empty() {
                            "usb:".to_string()
                        } else {
                            format!("usb:{}", host)
                        }
                    }
                    "pluto" => {
                        // pluto://192.168.2.1 -> just the IP
                        // pluto://ip:192.168.2.1 -> ip:192.168.2.1
                        // pluto://usb:1 -> usb:1
                        let host = url.host_str().unwrap_or("");
                        if host.starts_with("ip:") || host.starts_with("usb:") {
                            host.to_string()
                        } else if !host.is_empty() {
                            // Plain IP address, assume ip: prefix
                            host.to_string()
                        } else {
                            return Err("pluto:// requires a URI (IP address or usb:device)".to_string());
                        }
                    }
                    _ => unreachable!(),
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
            #[cfg(feature = "rtlsdr")]
            Address::Rtlsdr(rtlsdr_path) => {
                build_serial(&format!("rtlsdr:{}", rtlsdr_path.rtlsdr))
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
            Address::Rtlsdr(rtlsdr_path) => {
                let device_str = &rtlsdr_path.rtlsdr;
                let device_url = if device_str.is_empty() {
                    format!(
                        "rtlsdr://?freq={}M&rate={}M&gain={}",
                        (MODES_FREQ / 1e6) as u32,
                        (RATE_2_4M / 1e6) as u32,
                        RTLSDR_GAIN
                    )
                } else {
                    format!(
                        "rtlsdr://{}?freq={}M&rate={}M&gain={}",
                        device_str,
                        (MODES_FREQ / 1e6) as u32,
                        (RATE_2_4M / 1e6) as u32,
                        RTLSDR_GAIN
                    )
                };

                tokio::spawn(async move {
                    let config = desperado::DeviceConfig::from_str(&device_url)
                        .expect("Failed to parse RTL-SDR device config");
                    let source = IqAsyncSource::from_device_config(&config)
                        .await
                        .expect("Failed to create RTL-SDR source");
                    iqread::receiver(tx, source, serial, RATE_2_4M, name).await
                });
            }
            #[cfg(feature = "pluto")]
            Address::Pluto(pluto_path) => {
                let uri = pluto_path.pluto.clone();
                // Build full pluto:// URL for desperado
                // If uri already has ip: or usb: prefix, use it directly
                // If it's just an IP address, it can be used as-is (desperado handles it)
                let device_url = format!(
                    "pluto://{}?freq={}M&rate={}M&gain=50",
                    uri,
                    (MODES_FREQ / 1e6) as u32,
                    (RATE_2_4M / 1e6) as u32
                );

                tokio::spawn(async move {
                    let config = desperado::DeviceConfig::from_str(&device_url)
                        .expect("Failed to parse PlutoSDR device config");
                    let source = IqAsyncSource::from_device_config(&config)
                        .await
                        .expect("Failed to create PlutoSDR source");
                    iqread::receiver(tx, source, serial, RATE_2_4M, name).await
                });
            }
            #[cfg(feature = "soapy")]
            Address::Soapy(soapy_path) => {
                let args = soapy_path.soapy.clone();
                let device_url = format!(
                    "soapy://{}?freq={}M&rate={}M&gain=49.6",
                    args,
                    (MODES_FREQ / 1e6) as u32,
                    (RATE_2_4M / 1e6) as u32
                );

                tokio::spawn(async move {
                    let config = desperado::DeviceConfig::from_str(&device_url)
                        .expect("Failed to parse SoapySDR device config");
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
                reference: Some(pos),
                ..
            }) = source
            {
                assert!(matches!(address, Address::Rtlsdr(_)));
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

    #[test]
    fn test_toml_deserialization() {
        // Test RTL-SDR deserialization
        #[cfg(feature = "rtlsdr")]
        {
            let toml = r#"
                rtlsdr = "serial=00000001"
                latitude = 43.5993189
                longitude = 1.4362472
            "#;
            let source: Source =
                toml::from_str(toml).expect("Failed to parse TOML");
            assert!(matches!(source.address, Address::Rtlsdr(_)));
            if let Address::Rtlsdr(path) = &source.address {
                assert_eq!(path.rtlsdr, "serial=00000001");
            }
            assert_eq!(source.reference.unwrap().latitude, 43.5993189);
            assert_eq!(source.reference.unwrap().longitude, 1.4362472);
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
}
