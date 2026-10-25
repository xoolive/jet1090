# Changelog

## Unreleased

### Configuration compatibility

- Restore legacy boolean `prevent_sleep` settings alongside named modes:
  `true` maps to non-forcing `warn`, and `false` to `none`.
- Keep `--prevent-sleep` optional and preserve the configured mode when the
  CLI flag is omitted. Test normal invocation without arguments, not only help.
- Report default configuration read/parse failures with their path instead of
  panicking and dumping the full TOML, which may contain credentials.
- Cover legacy boolean/named sleep settings and optional CLI arguments with
  inline regression tests.

### SeRo source health

- Give unnamed SeRo subscriptions recognizable opaque labels instead of the
  aggregate `source-0000000000000000`, without changing individual sensor IDs
  or displaying access tokens. Respect explicit source aliases.
- Mark the feed connected after its authenticated stream and sensor metadata
  are ready, even before receiving frames. Associate setup/stream failures and
  unexpected EOF with that source instead of leaving it permanently connecting.
- Preserve the library's existing `receiver` API; add a connection callback,
  propagate gRPC stream errors and stop when the consumer closes. Report
  disabled SeRo support without dumping credentials; keep shutdown silent.

### Interactive sources

- Add an `s`/`S` sources panel alongside the aircraft table, showing existing
  source names, color-coded connection states and recent failure reasons.
  Preserve search input and the error overlay; hide stale failure reasons
  after recovery without deleting retained diagnostics.
- Use a consistent health snapshot outside its mutex when rendering the
  summary, sources and error history. Cap the sources panel at half the screen.
- Avoid arithmetic overflow when navigating the aircraft table before any
  aircraft have been received.

### DF11 decoding

- Export a typed `interrogator` in DF11 all-call replies. Keep the raw
  `crc_syndrome` internal (`serde(skip)`); the enum lives alongside the DF
  structures in `decode/mod.rs`, without a standalone DF11 module.
  Syndromes 0–15 identify II0–II15, 17–79 identify SI1–SI63; 16 is explicitly
  `invalid_si0`, and values above 79 remain `invalid` with their full syndrome.
  Invalid frames are retained, not silently masked or discarded. The textual
  decoder display also shows these fields. Legacy JSON without the new fields
  remains readable and does not invent II0. Rust construction of the
  `DF::AllCallReply` variant now requires the two additional optional fields;
  binary decoding always populates them.

### Streaming file output

- Automatically compress `.jsonl.zst` output in jet1090 and decode1090 (file,
  raw messages and CAT48), using a shared asynchronous writer. Flush compressor
  and file buffers every second, including idle periods; finalize on normal
  completion. Other filenames remain uncompressed.
- Preserve jet1090 append behavior using concatenated Zstandard frames and
  finalize on SIGINT/SIGTERM/SIGHUP in headless mode as well as interactive mode.
- Add `decode1090 follow capture.jsonl.zst` for incremental, flushed reading of
  growing compressed/plain files and appended sessions (reads existing content
  first; no rotation/truncation support). Avoid `zstd` CLI piped-input buffering
  which can delay the equivalent `tail | zstd -dc` pipeline at low traffic rates.

### File replay

- Default to recorded input order and zero deduplication. Use
  `--input-order=false` for timestamp sorting; combine it with
  `--deduplication 400` to restore the previous defaults.
- Support direct `.jsonl.zst` / `.csv.zst` input, including concatenated
  Zstandard frames; report corrupt or truncated compressed streams as errors.
- Share positional ADS-B dispatch and per-sensor reference updates with
  jet1090. `decode1090 file --update-position` follows the same below-5000-ft
  policy; without it, per-message updates are disabled. This replaces the
  previous unconditional below-1000-ft single-reference replay policy.
- Support recorded arrival-order replay with `--input-order` (requires zero
  deduplication). Reject input/output aliases, including hard links.
- Parse floating-point timestamps with round-trip precision; buffer output
  and propagate write errors rather than silently ignoring them.

### Bug Fixes

- Keep BDS 2,0 decoded when identification is empty or contains reserved
  characters: `AircraftIdentification.callsign` is now `Option<String>`.
  Unavailable values are `None` and omitted from JSON; the `bds20` object stays
  present. Valid callsigns serialize unchanged. Live aircraft state retains its
  last known callsign when a new identification is unavailable.
- Retain existing BDS20 inference precedence when identification is unavailable.
  Header-based winner selection is a heuristic, not proof against alternative
  unheadered layouts (e.g. BDS44 for payload `20000000000000`).

## 0.6.0

### Breaking Changes

- Emit BDS 6,5 ADS-B `version` as an integer (`0..=7`) instead of a string.

### BDS Inference

- **State-aware BDS inference engine** based on ASTERIX cat48 ground truth (#488):
  - Full state-aware inference replacing the previous stateless approach
  - Probability density model for BDS register classification
  - Penalty-based scoring to improve type disambiguation
  - Fixes incorrect inference on BDS 4,0, 5,0, 6,0 and 6,2 (#489, #490)
- Do not infer reserved BDS fields — avoids false positives on reserved register values
- New `bds-infer` feature flag to enable the full inference pipeline

### Bug Fixes

- Fix Python `decode_bds65()` decoding of full Mode S frames
- Fix interactive Ctrl+C exit: process could get stuck in background after shutdown was already requested (#487)
- Only spawn `crossterm` EventHandler in interactive mode — avoids spurious event polling in non-TTY contexts (#491)
- Register `/icao24` REST route and align documentation with actual endpoints (#492)

## 0.5.1

### Interactive Mode & Terminal Handling

- **Terminal restoration on crash/panic**:
  - Terminal now properly restores to normal state even when jet1090 crashes or panics
  - Prevents terminal corruption requiring manual `reset` command
  - Implementation using RAII drop guard pattern and panic hooks
- **Signal handling improvements**:
  - Graceful terminal restoration on SIGINT (Ctrl+C), SIGTERM (kill -15), and SIGHUP
  - Terminal state is properly cleaned up when process is terminated
  - Note: SIGKILL (kill -9) cannot be caught and will still require manual terminal reset

### SDR & Demodulator

- **6 MS/s demodulator support** (#444):
  - Add support for 6 MS/s sample rate (RTL-SDR Blog V4 native rate)
  - Refactor demodulator architecture for better maintainability
  - Automatic sample rate detection and appropriate demodulator selection
  - Improved performance and reduced CPU usage at higher sample rates

### Core Improvements

- **Graceful shutdown for async tasks**:
  - Implement coordinated shutdown for all async source receivers
  - Proper cleanup of SDR resources and network connections
  - 2-second timeout for task termination to prevent hangs
- **Code quality improvements**:
  - Extract magic numbers to named constants in CPR and altitude decoding
  - Improved code readability and maintainability

### Bug Fixes

- Fix redis publish timeout handling: retry instead of panic
- Fix npm publish authentication issues
- Implement `serde::Deserialize` for `TimedMessage`

## 0.5.0

### Interactive Mode Improvements

- Fix timestamp ordering and TUI freezes (#438)
  - Messages now processed in correct chronological order
  - Eliminates TUI rendering freezes during high message rates
- Add optional country flags column to TUI (#441)
  - Enable with `--flags` CLI option or `flags = true` in config
  - Visual country identification in aircraft table
- Expand Malta ICAO24 allocation to 0x4d2fff (#440)

### Build & Configuration

- Faster builds for release profile
- More default options in configuration files
- Clean up CLI arguments
- Remove unused `logging` feature from `deku` dependency (#442)
- Improve documentation on SDR detection

### SDR Support & Configuration

- **BREAKING**: Refactor SDR handling to use desperado 0.2.0 library (#416)
  - Unified device configuration with `DeviceConfig` pattern
  - Three feature flags: `rtlsdr`, `soapy`, `pluto` (replace old `sdr` feature)
  - Make desperado an optional dependency (only included with SDR features)
- **RTL-SDR improvements** (#416):
  - Device selection by index, manufacturer, product, or serial number
  - URL format: `rtlsdr://`, `rtlsdr://device_id=0`, `rtlsdr://serial=12345`
  - Configurable gain control (CLI and TOML config)
  - Bias-tee support with `--bias-tee` flag
  - TOML configuration example:
    ```toml
    [[sources.rtlsdr]]
    device_id = 0
    gain = 49.6
    bias_tee = true
    ```
- **SoapySDR improvements** (#416):
  - Bias-tee support (feature-gated, driver-dependent)
  - Device selection by serial number: `soapy://serial=...`
  - Configurable sample rate in TOML
- **PlutoSDR improvements** (#416):
  - Simplified to single `pluto://` scheme
  - Triple-slash URI support: `pluto:///ip:192.168.2.1` or `pluto:///usb:1.27.5`
- **I/Q file playback** (#416):
  - Support for recorded I/Q data files (cu8, cs8, cs16 formats)
  - URL format: `file:///path/to/recording.iq?format=cu8`
  - Timestamps calculated from file modification time
  - Tilde expansion (`~/`) support for file paths
- **Interactive mode improvements** (#416):
  - Add `--interactive-expire <seconds>` option for configurable aircraft display timeout
  - Add `--no-interactive-expire` flag to disable expiration (useful for IQ file playback)
  - Default: 30 seconds (backward compatible)
- **Build improvements** (#416):
  - Add libusb dependency configuration for cargo-dist (Linux: `libusb-dev`, macOS: `libusb`)
  - Clean builds with `--no-default-features`
  - Remove duplicated code (expanduser utility, config structs)

### Decoding Improvements

- **CPR decoding robustness improvements** (#434):
  - Fix surface positions decoded at wrong airport (up to 589km errors)
    - Surface messages now properly track airport reference changes
    - Reduces position errors by 98.8% in real flight data
  - Add speed validation: reject positions requiring >1200 km/h movement
  - Improve distance thresholds (50km → 500km context-aware validation)
  - Keep last good position on validation failure (prevents cascading failures)
  - decode1090: Sort messages by timestamp before processing (fixes out-of-order CPR pairing)
  - Bidirectional CPR pairing with 30-second timeout
  - Better type safety for reserved velocity fields in BDS 0,9
- Fix BDS 0,6 ground speed formula: movement codes 13-38 now use correct 0.5 kt steps instead of 0.25 kt per ICAO Doc 9871
  - Affected taxiing aircraft and slow ground movements (2-15 kt range)
- Fix altitude decoding to support negative altitudes for below-sea-level airports (EHAM, etc.) (#422, #424)
- Fix altitude field 0x000 (all-zeros) to return None instead of 0 per DO-260B spec (#425, #426)
  - Affects ~1.3% of Mode S messages with altitude unavailable
- **BREAKING**: Change AC13Field to wrap Option<i32> instead of i32 for proper "altitude unavailable" handling
- Fix invalid Gillham altitude patterns returning misleading 0 ft instead of None
- Reinforce pressure decode logic

### New Features

- **decode1090**: Add CSV (Beast format) support with auto-detection
  - Supports Beast protocol (0x1a 0x32/0x33) message parsing
  - Extracts RSSI signal strength from Beast format
  - Auto-detects CSV vs JSONL format
  - Works with compressed files (.7z, .gz)
- **rs1090**: Add MessageProcessor builder pattern for Comm-B sanitization
  - Centralized Comm-B data sanitization with CommBContext
  - Extensible design for future context-based validation
  - Eliminates code duplication across decode1090 and jet1090

### Testing & Quality

- Add comprehensive regression test suite in `tests/regression/`
  - Automated field-level comparison between decoder versions
  - Executive summary with change statistics and validation
  - Support for both JSONL and CSV Beast format inputs
  - Documentation and quickstart guides included
- **Python bindings**: Improve type annotations in rs1090 module (#436)
  - Add ParamSpec to unpickle_fun for better parameter type preservation
  - Remove unused type: ignore comments (pandas now properly typed)
  - Replace generic type: ignore with specific error codes
  - Reduces mypy errors from 23 to 0

### Documentation

- Add comprehensive ICAO Doc 9871 specification documentation across all 20 BDS decoder files
  - Every BDS register now includes ICAO table references and section numbers
  - All field encodings documented with formulas, ranges, and LSB resolutions

### Other

- Implement no_history_expire option (fix #391)
- Include README in Python package distribution
- Fix Docker release script
- Fix permission issue on npm.js publishing
- Slow down flake.lock auto-updates
- Remove unnecessary key with trusted publisher
- Fix tests after breaking changes

## 0.4.15

- Repository renamed from `rs1090` to `jet1090`
- Add Python 3.14 support (#402, #404)
- Upgrade deku from 0.18.1 to 0.20.2 (#380)
- New logo for the project
- Update platform name for macOS Intel builds
- Fail with clear error message if no data source is provided (fix #365)
- Serialization of reserved BDS 6,5 messages (#337, #338)
- Remove NODE_AUTH_TOKEN from npm publish workflow
- Revert to fixed gain for RTL-SDR ADS-B reception
- Trigger workflow on push to master only
- Multiple dependency updates and flake.lock updates

## 0.4.14

- Fix flake.nix for Linux and macOS compatibility
- Fix compilation issues with Rust 1.90.0
- Set automatic gain for RTL-SDR
- Update syntax for dev dependencies
- Fix for macOS linking issues in Nix flake
- Move aircraft database to rs1090 core library
- Fix WASM dependency issues
- Bump tonic and prost versions
- Bump Rust version
- Include docs.rs link in README
- Update cargo-dist and maturin
- Code refactoring

## 0.4.13

- Switch to ubuntu-22.04 for GitHub Actions runner

## 0.4.12

- Switch to ubuntu-22.04 for builds

## 0.4.11

- Internal release infrastructure updates

## 0.4.10

- Internal release infrastructure updates

## 0.4.9

- Add ssh tunnelling for Websocket and SeRo sources (#199, #200)
- Add support for ProxyCommand in SSH configuration
- Reuse existing SSH connections for efficiency
- Upgrade makiko library, try several identities for SSH authentication
- Fix documentation for sources
- Fix link in documentation

## 0.4.8

- Add option --update-db to download the latest aircraft database
- Add ssh tunnelling for TCP sources (#193)
- Fix for ubuntu-22.04 due to libsoapy version constraints
- Preparation for release

## 0.4.7

- Adjust filter behaviour for redis output
- Add French stride mappings for aircraft registrations
- Remove libssl dependency on Ubuntu, include arm64 target (#188)
- Publish to ghcr.io including arm64 support
- Add ghcr.io container image for both amd64 and arm64 architectures
- Remove South African stride mapping (no longer in use)
- Default filter behaviour adjustments

## 0.4.6

- Build target fixes

## 0.4.5

- Corner-case numerical problem in CPR decoding fix #153 (#154)
- Decode position from CPR with reference in Python binding (#163)
- Test suite for WASM bindings (#164)
- Update reference altitude threshold to below 5000ft (previously 1000ft was too low for some airports)
- Filter on SeRo sensors
- Add /airport route to REST API
- Add /track?icao24=...&since=... API endpoint with timestamp filtering
- Add airports and aircraft information query support in WASM binding
- Query airports functionality in WASM
- Refactor aircraft_information/pattern function for WASM API
- Upgrade cargo-dist with fix for issue #872
- GitHub Actions workflow fixes

## 0.4.4

- Dependency adjustments

## 0.4.3

- Add search bar (regex accepted) in jet1090 TUI based on callsign, icao24, registration, typecode, and receptor name
- Filter icao24 addresses and DF (Downlink Format) types in output
- Sensors/Receptors now have altitude field
- Catch more errors for BDS 0,5 and 6,5 decoding
- Improve error messages on various BDS types
- Switch to stdio for TUI, configure logging for soapysdr (#147)
- Possibility to filter messages by DF and icao24 address
- Publish to ghcr.io (fix #127)
- Add documentation for Arch Linux installation (#145)
- Remove unsafe code from codebase
- Improve README documentation
- Trigger Docker/GHCR actions on release tags

## 0.4.2

- Fix BDS 6,5 bug for DF 21 messages
- Implement WebAssembly bindings (#139)
- Move documentation pages to new structure
- Initial commit for jet1090 user documentation site
- Include shell completion for nushell
- Update Python version support

## 0.4.1

- Fix building scripts in GitHub releases

## 0.4.0

- Basic deduplication algorithm
- Support for SeRo system data source
- Adjust name aliases and serial numbers
- Major refactoring

## 0.3.9

- New data model
- allow to specify which rtlsdr to use
- fix log_file being ignored in config files
- prefer XDG_CONFIG_HOME if defined over system default, useful in MacOS
- Improve BDS inference based on ground truth messages
- add option to prevent laptop from sleeping
- improve error message when redis server is not reachable

## 0.3.8

- Adapt to Python 3.13
- Propagate errors on non-null CRC chcks

## 0.3.7

- Switch to uv

## 0.3.6

- Select rtl-sdr with arguments
- Typing issue on Redis publish after upgrade

## 0.3.5

- Do not attempt to decode Beast 0x34 messages (avoid errors)
- Fix error forwarding

## 0.3.4

- Major code adjustments following upgrade to deku 0.17.0
- Upgrade cargo-dist, maturin, change version semantics (shared version)

## 0.3.0

- Adjust size for MPSC channels
- Decode Beast format from a websocket source
- Decode RSSI in Beast format, and implement it for RTL-SDR
- Improvement in CPR decoding
- Shell completion
- Support for nix/flake
