# Configuration options

All the options passed to the executable and visible in the help can also be configured as default in a configuration file.

```sh
jet1090 --help
```

By default, the configuration file is located in:

- `$HOME/.config/jet1090/config.toml` for Linux systems;
- `$HOME/Library/Application\ Support/jet1090/config.toml` for MacOS systems;
- `%HOME%\AppData\Roaming\jet1090\config.toml` for Windows systems

!!! tip "Support for `XDG_CONFIG_HOME`"

    If the `XDG_CONFIG_HOME` variable is set, it takes precedence over the folders detailed above.

    This means you can set this variable and use the `$HOME/.config` folders in MacOS systems as well.

!!! tip

    - You can also set a different configuration file in the `JET1090_CONFIG` environment variable.
    - You can also set that variable in the `.env` (dotenv) file located in the current folder and `jet1090` will look into it.

    If you have several scenarios requiring different configurations files, this option may be where to look at.

## General settings

If you set a configuration file, some parameters must be always present:

```toml
interactive = false      # display a table view
verbose = false          # display decoded messages in the terminal
prevent_sleep = false    # force the laptop not to enter sleep mode (useful when lid is closed)
update_position = false  # auto-update the reference position (useful when on a moving aircraft)
```

Other parameters are optional:

```toml
deduplication = 800        # buffer interval for deduplication, in milliseconds
history_expire = 10        # in minutes
log_file = "-"             # use together with RUSTLOG environment variable
output = "~/output.jsonl"  # the ~ (tilde) character is automatically expanded
redis_url = "redis://localhost:6379"
serve_port = 8080          # for the REST API
```

## Sources

!!! warning

    If you do not want to set any source in the configuration file, you must specify an empty list:

    ```toml
    sources = []
    ```

    Otherwise, **do not** include that line, and set as many sources as you need with the `[[sources]]` header.

### RTL-SDR

RTL-SDR devices can be configured by device index:

RTL-SDR devices are configured using a structured format in TOML.

**Device selection by index:**

```toml
[[sources]]
name = "rtl-sdr-first"
rtlsdr = { device = 0 }
airport = "LFBO"
```

**Device selection by serial number:**

```toml
[[sources]]
name = "rtl-sdr-by-serial"
rtlsdr = { serial = "00000001" }
airport = "LFBO"
```

**Device selection with multiple filters:**

```toml
[[sources]]
name = "rtl-sdr-filtered"
rtlsdr = { serial = "00000001", manufacturer = "Realtek", product = "RTL2838UHIDIR" }
airport = "EHAM"
```

!!! tip "RTL-SDR Device Selection"

    RTL-SDR devices can be selected using the following fields:
    
    - **device**: Device index (0, 1, 2, ...) - `rtlsdr = { device = 0 }`
    - **serial**: Serial number - `rtlsdr = { serial = "00000001" }`
    - **manufacturer**: Manufacturer name - `rtlsdr = { manufacturer = "Realtek" }`
    - **product**: Product name - `rtlsdr = { product = "RTL2838UHIDIR" }`
    
    You can combine filters (serial, manufacturer, product) for precise device matching. 
    All specified filters must match for the device to be selected. This is useful when 
    you have multiple RTL-SDR devices and want to ensure you always connect to the same 
    physical device, regardless of USB port order.

!!! tip "Gain Configuration"

    You can set a custom gain value for SDR devices using the `gain` parameter:

    ```toml
    [[sources]]
    name = "rtl-sdr-custom-gain"
    rtlsdr = { device = 0 }
    gain = 42.5
    airport = "LFBO"
    ```

    Default gain values:
    - **RTL-SDR**: 49.6 dB (optimized for ADS-B reception)
    - **PlutoSDR**: 73.0 dB
    - **SoapySDR**: 49.6 dB (same as RTL-SDR)

    If not specified, the default value for each device type will be used.

!!! tip "Bias-Tee Configuration (RTL-SDR and SoapySDR)"

    Enable bias-tee to provide power to an external Low Noise Amplifier (LNA):
    
    **RTL-SDR:**
    ```toml
    [[sources]]
    name = "rtl-sdr-with-lna"
    rtlsdr = { device = 0 }
    bias_tee = true
    gain = 42.5
    airport = "LFBO"
    ```
    
    **SoapySDR:**
    ```toml
    [[sources]]
    name = "soapy-rtlsdr-with-lna"
    soapy = "driver=rtlsdr"
    bias_tee = true
    gain = 42.5
    airport = "LFBO"
    ```
    
    **Default**: `false` (disabled)
    
    !!! warning "Hardware Safety"
    
        Only enable bias-tee if you have an LNA that requires power. 
        Enabling it without proper equipment can damage your hardware.
        
        Note: Bias-tee support in SoapySDR depends on the underlying driver.
        It's primarily supported when using SoapySDR with RTL-SDR devices.


!!! note "Command-line Usage"

    When using RTL-SDR from the command line, you can use simple formats:
    
    ```bash
    # By device index
    jet1090 rtlsdr://0
    jet1090 rtlsdr://1
    
    # By serial number  
    jet1090 rtlsdr://serial=00000001
    
    # Default device (device 0)
    jet1090 rtlsdr://
    
    # With custom gain
    jet1090 rtlsdr://0?gain=40
    
    # With bias-tee enabled
    jet1090 rtlsdr://0?bias_tee=true
    
    # With location (airport code) - using ? or @ syntax
    jet1090 rtlsdr://0?LFBO
    jet1090 rtlsdr://0@LFBO
    
    # Combining gain and location
    jet1090 rtlsdr://0?LFBO&gain=42.5
    jet1090 rtlsdr://0@LFBO&gain=42.5
    
    # Combining all parameters
    jet1090 rtlsdr://0?LFBO&gain=42.5&bias_tee=true
    ```

The `airport` parameter replaces the `latitude` and `longitude` parameter if they are not present.

### PlutoSDR

PlutoSDR devices can be configured via IP, hostname, or USB connection.

**Note**: The PlutoSDR library requires URIs in the format `ip:address` or `usb:device`. When you provide a simple IP address or hostname (e.g., `192.168.2.1` or `pluto.local`), jet1090 automatically adds the `ip:` prefix.

For IP connection (most common):

```toml
[[sources]]
name = "pluto"
pluto = "192.168.2.1"
latitude = 43.5993189
longitude = 1.4362472
altitude = 151.0
```

For hostname-based connection:

```toml
[[sources]]
name = "pluto"
pluto = "pluto.local"
airport = "LFBO"
```

You can also use the explicit `ip:` prefix with IP addresses or hostnames:

```toml
[[sources]]
name = "pluto"
pluto = "ip:192.168.2.1"
airport = "LFBO"
```

```toml
[[sources]]
name = "pluto"
pluto = "ip:pluto.local"
airport = "LFBO"
```

For USB-connected PlutoSDR:

```toml
[[sources]]
name = "pluto"
pluto = "usb:"
airport = "EHAM"
```

Or with a specific USB device (for USB devices with version numbers like `usb:1.18.5`, use command-line format `pluto:///usb:1.18.5`):

```toml
[[sources]]
name = "pluto"
pluto = "usb:1.18.5"
airport = "EHAM"
```

!!! tip "Command-line formats"

    When specifying PlutoSDR devices from the command line:
    
    ```bash
    # IP address or hostname (simple format - ip: prefix added automatically)
    jet1090 pluto://192.168.2.1
    jet1090 pluto://pluto.local
    
    # With explicit ip: prefix (use triple slashes for URIs with colons)
    jet1090 pluto:///ip:192.168.2.1
    jet1090 pluto:///ip:pluto.local
    
    # USB device (use triple slashes for URIs with colons)
    jet1090 pluto:///usb:1.7.5
    ```
    
    In TOML configuration files, you can use the URI directly without the triple slashes.

### SoapySDR

SoapySDR devices can be configured with driver arguments:

```toml
[[sources]]
name = "soapy-rtlsdr"
soapy = "driver=rtlsdr"
airport = "LFBO"
```

With custom gain and bias-tee:

```toml
[[sources]]
name = "soapy-rtlsdr-with-lna"
soapy = "driver=rtlsdr"
gain = 42.5
bias_tee = true
airport = "LFBO"
```

Or with other SoapySDR-compatible devices:

```toml
[[sources]]
name = "soapy-hackrf"
soapy = "driver=hackrf"
latitude = 51.4706
longitude = -0.4619
```

### IQ File (Offline Decoding)

You can decode pre-recorded IQ files captured from SDR devices. This is useful for:
- Testing and debugging ADS-B decoding without live hardware
- Processing historical recordings
- Offline analysis and development

**Supported IQ formats:**

- **cu8**: Complex unsigned 8-bit (RTL-SDR default format)
  - Range: 0-255, where 127.5 represents zero
  - Most common format from `rtl_sdr` command
- **cs8**: Complex signed 8-bit
  - Range: -128 to 127
- **cs16**: Complex signed 16-bit little-endian
  - Range: -32768 to 32767

**TOML Configuration:**

```toml
[[sources]]
file = "/path/to/recording.iq"
iq_format = "cu8"  # Optional, defaults to "cu8"
name = "Recording 2024-01-19"
airport = "LFBO"
```

With tilde expansion:

```toml
[[sources]]
file = "~/adsb-recordings/flight-2024-01-19.iq"
iq_format = "cu8"
name = "Historical Flight"
latitude = 43.5993189
longitude = 1.4362472
```

**Command-line Usage:**

```bash
# With absolute path
jet1090 "file:///home/user/adsb.iq?format=cu8"

# With tilde expansion
jet1090 "file://~/recordings/adsb.iq?format=cu8"

# Default format (cu8) - format parameter optional
jet1090 "file:///path/to/file.iq"

# With location context
jet1090 "file://~/adsb.iq?format=cu8&LFBO"
```

!!! tip "Recording IQ files"

    You can create IQ files using the `rtl_sdr` command (from rtl-sdr tools):
    
    ```bash
    # Record 60 seconds at 1090 MHz with 2.4 MS/s sample rate
    rtl_sdr -f 1090M -s 2.4M -g 49.6 -n 288000000 adsb.iq
    ```
    
    The file will be in cu8 format by default, which is directly compatible with jet1090.

!!! note "Playback Behavior"

    The IQ file is read sequentially from beginning to end. When the file ends, decoding stops. 
    The playback simulates real-time reception at the configured sample rate (2.4 MS/s for ADS-B).

### Beast format

External sources can be configured with the `tcp`, `udp` or `websocket` fields.

```toml
[[sources]]
name = "Toulouse"
tcp = "123.45.67.89:10003"
latitude = 43.5993189
longitude = 1.4362472
```

For the `websocket` you must specify the `ws://` prefix:

```toml
[[sources]]
websocket = "ws://123.45.67.89:8765/zurich"
airport = "LSZH"
```

!!! warning "Reference positions"

    When in a hurry, an airport code is enough to decode [surface messages](https://docs.rs/rs1090/latest/rs1090/decode/bds/bds06/struct.SurfacePosition.html) (otherwise, only `lat_cpr` and `lon_cpr` are provided). It may be useful to fill in precise values for `latitude`, `longitude` and `altitude` for multilateration applications.

!!! warning "Different names for different sources"

    The `name` entry is not mandatory but it is helpful to help recognize different sources in the output format. However, internally, an hashed version of the address is used to uniquely identify sources.

### SeRo Systems

You may input here your [SeRo Systems token](https://doc.sero-systems.de/api/) in order to receive your data. Extra filters are also available in order to limit the network bandwidth.

```toml
[[sources]]
sero.token = ""
sero.df_filter = [17, 18, 20, 21]  # (default: no filter)
# sero.aircraft_filter = []  # list of integer values corresponding to icao24 addresses (default: no filter)
```
