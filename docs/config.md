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

```toml
deduplication = 800        # buffer interval for deduplication, in milliseconds
flags = false              # add a flag emoji column in the interactive table
history_expire = 10        # in minutes
interactive = false        # display a table view
log_file = "-"             # use together with RUSTLOG environment variable
output = "~/output.jsonl"  # the ~ (tilde) character is automatically expanded
postgres_url = "postgres://user:password@localhost/jet1090"  # insert messages into PostgreSQL
postgres_table = "jet1090" # table name (created automatically if missing)
prevent_sleep = false      # force the laptop not to enter sleep mode (useful when lid is closed)
redis_url = "redis://localhost:6379"
serve_port = 8080          # for the REST API
update_position = false    # auto-update the reference position (useful when on a moving aircraft)
verbose = false            # display decoded messages in the terminal
```

## Sources

### RTL-SDR

**Minimal configuration:**

```toml
[[sources]]
rtlsdr = {}
```

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
rtlsdr.serial = "00000001"
rtlsdr.manufacturer = "Realtek"
rtlsdr.product = "RTL2838UHIDIR"
```

**Gain parameter**

You can set a custom gain value for SDR devices using the `gain` parameter:

```toml
[[sources]]
name = "rtl-sdr-custom-gain"
rtlsdr = { device = 0 }
gain = 42.5
```

Default gain values: 49.6 dB (max value)

!!! note "Command-line Usage"

    When using RTL-SDR from the command line, you can use simple formats:

    ```bash
    # Default device (device 0)
    jet1090 rtlsdr://

    # By device index
    jet1090 rtlsdr://0
    jet1090 rtlsdr://1

    # By serial number
    jet1090 rtlsdr://serial=00000001

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

### Airspy

**Minimal configuration:**

```toml
[[sources]]
airspy = {}
```

Airspy devices can be selected by index or serial number:

```toml
[[sources]]
name = "airspy-default"
airspy = { device = 0 }
```

```toml
[[sources]]
name = "airspy-by-serial"
airspy = { serial = "0x35AC63DC2D8C7A4F" }
sample_rate = 6e6  # default sample rate
```

**Gain parameters**:

The Airspy uses a _sensitivity_ gain mode (0–21 steps for Airspy R2, 0–100 as a
percentage-style value). The default gain is **50**, which is a good middle point:

```toml
[[sources]]
name = "Airspy"
airspy = { device = 0 }
gain = 50           # default; lower if you have a strong signal / overloading
```

!!! note "Maximum gain and overload"

    The maximum sensitivity gain value depends on the firmware version and Airspy model. At high gain the device can saturate near busy airports. Reduce `gain` if you see many CRC errors.

!!! tip "External LNA and bias-tee"

    Airspy devices benefit greatly from an external low-noise amplifier (LNA) placed before the antenna connector. If your LNA is powered through the coax (bias-tee), enable it explicitly. It is **off by default**:

    ```toml
    [[sources]]
    name = "Airspy with LNA"
    airspy = { device = 0 }
    gain = 30          # lower gain when using an external LNA
    bias_tee = true    # power the LNA through the coax
    ```

!!! warning "Per-element gains vs. `gain`"

    You can set individual gain stages inside `airspy = { ... }` using
    `lna_gain`, `mixer_gain`, and `vga_gain` (0–14 / 0–15 range depending on
    stage). These are **mutually exclusive** with the top-level `gain` field:
    specifying both is an error.

    ```toml
    # Fine-grained control (advanced users)
    [[sources]]
    airspy = { device = 0, lna_gain = 12, mixer_gain = 10, vga_gain = 10 }
    ```

### HackRF

**Minimal configuration:**

```toml
[[sources]]
hackrf = {}
```

HackRF devices can be selected by device index:

```toml
[[sources]]
name = "hackrf-default"
hackrf = { device = 0 }
sample_rate = 6e6  # this is default sample rate with hackrf
```

**Gain parameters**:

The simplest way to control gain is the top-level `gain` field, which passes a
single scalar value to the driver:

```toml
[[sources]]
hackrf = { device = 0 }
gain = 100
```

When `gain` is omitted, jet1090 falls back to the per-element defaults described
in the advanced section below.

**DC offset**

HackRF is a direct-sampling receiver that suffers from a DC spike at the exact
centre frequency (1090 MHz). Tuning slightly off-centre avoids this. The default
offset is already **−75 kHz** — you normally do not need to set it explicitly:

```toml
[[sources]]
hackrf = { device = 0, freq_offset_hz = -75000 }  # already the default
```

!!! note "Command-line usage"

    ```bash
    # Default device — gains and offset applied automatically
    jet1090 hackrf://

    # Specific device index
    jet1090 hackrf://1

    # Per-element gain control requires a config file
    jet1090 --config ~/.config/jet1090/config.toml
    ```

!!! warning "Per-element gains vs. `gain`"

    For fine-grained tuning you can set each gain stage individually inside
    `hackrf = { ... }`. This is **mutually exclusive** with the top-level `gain`
    field — specifying both produces an error.

    The default values applied when neither `gain` nor per-element parameters are
    set are:

    ```toml
    [[sources]]
    hackrf = { device = 0, lna_gain = 40, vga_gain = 55, amp_enable = true, freq_offset_hz = -75000 }
    ```

    This works well for a modest rooftop antenna without an external LNA.

    - **lna_gain** (0–40 dB, 8 dB steps): Low-Noise Amplifier stage.
      Default: **40 dB**. Reduce if you are near a busy airport and experience
      saturation (many CRC errors).

    - **vga_gain** (0–62 dB, 2 dB steps): Variable Gain Amplifier stage.
      Default: **55 dB**. Avoid the maximum (62 dB) as it causes saturation.

    - **amp_enable** (`true`/`false`): RF Amplifier (+14 dB boost before the LNA).
      Default: **true**. Disable if you have a strong signal or an external LNA.

    - **freq_offset_hz** (integer Hz): Frequency offset to avoid the DC spike.
      Default: **-75000** (−75 kHz).

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
name = "soapy-bladerf"
soapy = "driver=bladerf"
latitude = 51.4706
longitude = -0.4619
```

For PlutoSDR through SoapySDR:

```toml
[[sources]]
name = "soapy-pluto"
soapy = "driver=plutosdr"
airport = "LFBO"
```

### IQ File (Offline Decoding)

You can decode pre-recorded IQ files captured from SDR devices. This is useful for:

- Testing and debugging ADS-B decoding without live hardware
- Processing historical recordings
- Offline analysis and development

**Supported IQ formats:**

- **cu8**: Complex unsigned 8-bit (RTL-SDR default format)
- **cs8**: Complex signed 8-bit
- **cs16**: Complex signed 16-bit little-endian

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
