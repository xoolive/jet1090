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

The following entry is equivalent to `rtlsdr://serial=00000001@LFBO` except that it sets an alias `rtl-sdr`:

```toml
[[sources]]
name = "rtl-sdr"
rtlsdr = "serial=00000001"
airport = "LFBO"
```

For the default RTL-SDR device, use an empty string:

```toml
[[sources]]
name = "rtl-sdr"
rtlsdr = ""
airport = "LFBO"
```

The `airport` parameter replaces the `latitude` and `longitude` parameter if they are not present.

### PlutoSDR

PlutoSDR devices can be configured via IP or USB connection.

For IP connection (most common):

```toml
[[sources]]
name = "pluto"
pluto = "192.168.2.1"
latitude = 43.5993189
longitude = 1.4362472
altitude = 151.0
```

You can also use the explicit `ip:` prefix:

```toml
[[sources]]
name = "pluto"
pluto = "ip:192.168.2.1"
airport = "LFBO"
```

For USB-connected PlutoSDR:

```toml
[[sources]]
name = "pluto"
pluto = "usb:"
airport = "EHAM"
```

Or with a specific USB device:

```toml
[[sources]]
name = "pluto"
pluto = "usb:1"
airport = "EHAM"
```

!!! note "Backward compatibility"

    The command-line also supports legacy schemes `plutoip://` and `plutousb://` which are automatically converted to `pluto://ip:` and `pluto://usb:` respectively.

### SoapySDR

SoapySDR devices can be configured with driver arguments:

```toml
[[sources]]
name = "soapy-rtlsdr"
soapy = "driver=rtlsdr"
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
