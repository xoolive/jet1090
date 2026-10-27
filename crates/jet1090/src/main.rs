#![doc = include_str!("../readme.md")]

mod dedup;
mod filters;
mod health;
mod redis;
mod sensor;
mod shell;
mod snapshot;
mod source;
mod table;
mod tui;
mod util;
mod web;

use crate::health::SharedHealth;
use crate::tui::Event;
use crate::util::expanduser;
use crate::web::serve_web_api;
use clap::{Command, CommandFactory, Parser, ValueEnum, ValueHint};
use clap_complete::{generate, Generator};
use crossterm::event::KeyCode;
use ratatui::widgets::*;
use rs1090::data::aircraft;
use rs1090::decode::commb::MessageProcessor;
use rs1090::decode::cpr::{
    decode_sensor_position, reference_update_policy, AircraftState,
};
use rs1090::decode::serialize_config;
use rs1090::output::JsonlWriter;
use rs1090::prelude::*;
use sensor::Sensor;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs;
use tokio::sync::{broadcast, watch, Mutex, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{error, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Clone, Copy, Debug, Default, Deserialize, ValueEnum)]
#[serde(try_from = "PreventSleepInput")]
enum PreventSleep {
    #[default]
    None,
    Warn,
    Force,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PreventSleepInput {
    Legacy(bool),
    Mode(String),
}

impl TryFrom<PreventSleepInput> for PreventSleep {
    type Error = String;

    fn try_from(value: PreventSleepInput) -> Result<Self, Self::Error> {
        match value {
            PreventSleepInput::Legacy(true) => Ok(Self::Warn),
            PreventSleepInput::Legacy(false) => Ok(Self::None),
            PreventSleepInput::Mode(mode) => Self::from_str(&mode, false),
        }
    }
}

#[derive(Default, Deserialize, Parser)]
#[command(
    name = "jet1090",
    version,
    author = "xoolive",
    about = "Decode and serve Mode S demodulated raw messages"
)]
struct Options {
    /// Activate JSON output
    #[arg(short, long)]
    #[serde(default)]
    verbose: bool,

    /// Dump messages as JSONL (Zstandard if the filename ends in .jsonl.zst)
    #[arg(short, long, default_value=None, value_hint=ValueHint::FilePath)]
    output: Option<String>,

    /// Display a table in interactive mode (not compatible with verbose)
    #[arg(short, long)]
    #[serde(default)]
    interactive: bool,

    /// Show country flags in TUI display (only visible when width > 130)
    #[arg(long)]
    #[serde(default)]
    flags: bool,

    /// Port for the API endpoint (on 0.0.0.0)
    #[arg(long, default_value=None)]
    serve_port: Option<u16>,

    /// How much history to expire (in minutes), 0 for no history
    #[arg(
        long,
        env = "EXPIRE_AIRCRAFT",
        short = 'x',
        conflicts_with = "no_history_expire"
    )]
    history_expire: Option<u64>,

    /// Disable history expiration
    #[arg(long, conflicts_with = "history_expire")]
    #[serde(default)]
    no_history_expire: bool,

    /// How long to keep aircraft visible in interactive mode (in seconds), 0 for no expiration
    #[arg(
        long,
        default_value = "30",
        conflicts_with = "no_interactive_expire"
    )]
    interactive_expire: Option<u64>,

    /// Disable interactive mode aircraft expiration
    #[arg(long, conflicts_with = "interactive_expire")]
    #[serde(default)]
    no_interactive_expire: bool,

    /// Downlink formats to select for stdout, file output, Redis, /stream and history in REST API (keep empty to select all)
    #[arg(long, value_name = "DF")]
    df_filter: Option<Vec<u16>>,

    /// Aircraft addresses to select for stdout, file output, Redis, /stream and history in REST API (keep empty to select all)
    #[arg(long, value_name = "ICAO24")]
    aircraft_filter: Option<Vec<ICAO>>,

    /// Prevent the computer sleeping: none, warn about macOS pmset, or force it
    #[arg(long, value_enum, default_value = "none")]
    #[serde(default)]
    prevent_sleep: PreventSleep,

    /// Should we update the reference positions (if the receiver is moving)
    #[arg(short, long, default_value=None)]
    #[serde(default)]
    update_position: bool,

    /// Capture-time group expiry threshold in milliseconds (not wall-clock residence).
    #[arg(long, default_value = "450")]
    deduplication: Option<u32>,

    /// Monotonic lookahead in milliseconds, applied to collection and ready ordering.
    /// Defaults: 450 ms collection plus up to 200 ms ordering; capture span remains 450 ms.
    /// Ignored when the deduplication threshold is 0. Does not guarantee globally
    /// chronological output for arbitrarily stale feeds.
    #[arg(long, default_value = "200")]
    reorder_window: Option<u32>,

    /// Disable deduplication (messages are passed through without merging)
    #[arg(long, default_value=None)]
    #[serde(default)]
    no_deduplication: bool,

    /// Include decoding time statistics in the output
    #[arg(long)]
    #[serde(default)]
    stats: bool,

    /// Shell completion generation
    #[arg(long = "completion", value_enum)]
    #[serde(skip)]
    completion: Option<shell::Shell>,

    /// Download a new version of aircraft database
    #[arg(long)]
    #[serde(skip)]
    update_db: bool,

    /// List the sources of data following the format \[host:\]port\[\@reference\]
    ///
    /// `host` can be a DNS name, an IP address, `rtlsdr` (for RTL-SDR dongles),
    /// `airspy` (for Airspy Mini/R2/HF+ devices), or `hackrf` (for HackRF One/Micro devices),
    /// `port` must be a number,
    /// `reference` can be LFPG for major airports, `43.3,1.35` otherwise.
    ///
    /// To verify your SDR device is detected before running jet1090:
    /// - RTL-SDR: `rtl_test -t` or `rtl_eeprom`
    /// - Airspy: `airspy_info`
    /// - HackRF: `hackrf_info`
    /// - SoapySDR devices (including PlutoSDR): `SoapySDRUtil --find` or `SoapySDRUtil --probe`
    ///
    /// More details are available at: <https://mode-s.org/jet1090/sources>
    #[serde(default)]
    sources: Vec<source::Source>,

    /// logging file, use "-" for stdout (only in non-interactive mode)
    #[arg(short, long, value_name = "FILE")]
    log_file: Option<String>,

    /// Publish messages to a Redis pubsub
    #[arg(short, long, value_name = "REDIS URL")]
    redis_url: Option<String>,

    /// Redis topic for the messages, default to "jet1090"
    #[arg(long, value_name = "REDIS TOPIC")]
    redis_topic: Option<String>,

    /// Redis retry interval in seconds (0 skips failed batches with a warning)
    #[arg(long, value_name = "SECONDS")]
    #[serde(default)]
    redis_retry_interval: Option<u64>,
}

#[cfg(target_os = "macos")]
async fn check_macos_sleep_setting(mode: PreventSleep) -> bool {
    use tokio::process::Command;

    let output =
        match Command::new("pmset").arg("-g").arg("custom").output().await {
            Ok(output) => output,
            Err(error) => {
                warn!(%error, "Unable to check macOS sleep settings");
                return false;
            }
        };
    let settings = String::from_utf8_lossy(&output.stdout);
    let mut in_battery = false;
    let battery_disablesleep = settings.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed == "Battery Power:" || trimmed == "batt:" {
            in_battery = true;
            return false;
        }
        if trimmed == "AC Power:"
            || (trimmed.ends_with(":") && trimmed != "Battery Power:")
        {
            in_battery = false;
        }
        if in_battery {
            let mut fields = trimmed.split_whitespace();
            return fields.next() == Some("disablesleep")
                && fields.next() == Some("1");
        }
        false
    });

    if battery_disablesleep {
        return false;
    }

    if matches!(mode, PreventSleep::Force) {
        match Command::new("sudo")
            .args(["pmset", "-b", "disablesleep", "1"])
            .status()
            .await
        {
            Ok(status) if status.success() => return true,
            Ok(status) => {
                warn!(%status, "Unable to enable macOS battery sleep prevention")
            }
            Err(error) => {
                warn!(%error, "Unable to run sudo pmset for macOS sleep prevention")
            }
        }
    }

    warn!("macOS battery sleep prevention is disabled; run `sudo pmset -b disablesleep 1` to enable it");
    false
}

#[cfg(target_os = "macos")]
async fn restore_macos_sleep_setting() {
    use tokio::process::Command;

    match Command::new("sudo")
        .args(["pmset", "-b", "disablesleep", "0"])
        .status()
        .await
    {
        Ok(status) if status.success() => {}
        Ok(status) => {
            warn!(%status, "Unable to restore macOS battery sleep setting")
        }
        Err(error) => {
            warn!(%error, "Unable to restore macOS battery sleep setting")
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from a .env file
    dotenv::dotenv().ok();

    let mut options = Options::default();

    let mut cfg_path = match std::env::var("XDG_CONFIG_HOME") {
        Ok(xdg_config) => expanduser(PathBuf::from(xdg_config)),
        Err(_) => dirs::config_dir().unwrap_or_default(),
    };
    cfg_path.push("jet1090");
    cfg_path.push("config.toml");

    if cfg_path.exists() {
        let string = fs::read_to_string(&cfg_path).await.map_err(|error| {
            format!(
                "Failed to read configuration file '{}': {error}",
                cfg_path.display()
            )
        })?;
        options = toml::from_str(&string).map_err(|error| {
            format!(
                "Failed to parse configuration file '{}': {error}",
                cfg_path.display()
            )
        })?;
    }

    if let Ok(config_file) = std::env::var("JET1090_CONFIG") {
        let path = expanduser(PathBuf::from(config_file));
        let string = fs::read_to_string(&path).await.map_err(|error| {
            format!(
                "Failed to read configuration file '{}': {error}",
                path.display()
            )
        })?;
        options = toml::from_str(&string).map_err(|error| {
            format!(
                "Failed to parse configuration file '{}': {error}",
                path.display()
            )
        })?;
    }

    let mut cli_options = Options::parse();

    // Generate completion instructions
    if let Some(generator) = cli_options.completion {
        let mut cmd = Options::command();
        print_completions(generator, &mut cmd);
        return Ok(());
    }

    if cli_options.update_db {
        aircraft::update_db().await.unwrap();
        return Ok(());
    }

    if cli_options.verbose {
        options.verbose = true;
    }
    if cli_options.output.is_some() {
        options.output = cli_options.output;
    }
    if cli_options.interactive {
        options.interactive = true;
    }
    if cli_options.flags {
        options.flags = true;
    }
    if cli_options.serve_port.is_some() {
        options.serve_port = cli_options.serve_port;
    }
    if cli_options.no_history_expire {
        options.no_history_expire = true;
        options.history_expire = None;
    } else if let Some(history_expire) = cli_options.history_expire {
        options.no_history_expire = false;
        options.history_expire = Some(history_expire);
    }
    if cli_options.no_interactive_expire {
        options.interactive_expire = Some(0);
    } else if let Some(interactive_expire) = cli_options.interactive_expire {
        options.interactive_expire = Some(interactive_expire);
    }
    if cli_options.df_filter.is_some() {
        options.df_filter = cli_options.df_filter;
    }
    if cli_options.aircraft_filter.is_some() {
        options.aircraft_filter = cli_options.aircraft_filter;
    }
    if !matches!(cli_options.prevent_sleep, PreventSleep::None) {
        options.prevent_sleep = cli_options.prevent_sleep;
    }
    if cli_options.update_position {
        options.update_position = cli_options.update_position;
    }
    if cli_options.log_file.is_some() {
        options.log_file = cli_options.log_file;
    }
    if cli_options.redis_url.is_some() {
        options.redis_url = cli_options.redis_url;
    }
    if cli_options.redis_topic.is_some() {
        options.redis_topic = cli_options.redis_topic;
    }
    if cli_options.redis_retry_interval.is_some() {
        options.redis_retry_interval = cli_options.redis_retry_interval;
    }
    if cli_options.stats {
        options.stats = cli_options.stats;
    }
    if cli_options.deduplication.is_some() {
        options.deduplication = cli_options.deduplication;
    }
    if cli_options.reorder_window.is_some() {
        options.reorder_window = cli_options.reorder_window;
    }
    if options.stats {
        serialize_config(true);
    }

    if options.history_expire.is_none() && !options.no_history_expire {
        options.history_expire = Some(15);
    }

    options.sources.append(&mut cli_options.sources);

    // Capture operational warnings by default; RUST_LOG can raise or lower verbosity.
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn"));
    let health = health::new_health();
    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(health::UiEventLayer::new(health.clone()));
    if options.interactive {
        // Writing tracing events to stdout or stderr corrupts the alternate TUI screen.
        subscriber.init();
    } else {
        match options.log_file.as_deref() {
            Some("-") => subscriber.with(fmt::layer().pretty()).init(),
            Some(log_file) => {
                let file =
                    std::fs::File::create(log_file).unwrap_or_else(|_| {
                        panic!("fail to create log file: {log_file}")
                    });
                let file_layer =
                    fmt::layer().with_writer(file).with_ansi(false);
                subscriber.with(file_layer).init();
            }
            None => subscriber.init(),
        }
    }

    if options.sources.is_empty() {
        eprintln!(
            "No source of data specified, use --help for more information"
        );
        std::process::exit(1);
    }

    let redis_connection = match options.redis_url {
        Some(url) => {
            let client = ::redis::Client::open(url)
                .map_err(|error| format!("Invalid Redis URL: {error}"))?;
            let connection = tokio::time::timeout(
                redis::CONNECT_TIMEOUT,
                client.get_multiplexed_async_connection(),
            )
            .await
            .map_err(|_| {
                format!(
                    "Timed out after {} seconds connecting to Redis",
                    redis::CONNECT_TIMEOUT.as_secs()
                )
            })?
            .map_err(|error| format!("Failed to connect to Redis: {error}"))?;
            Some((client, connection))
        }
        None => None,
    };
    let redis_topic = options.redis_topic.unwrap_or("jet1090".to_string());
    let redis_retry_interval =
        Duration::from_secs(options.redis_retry_interval.unwrap_or(5));
    let mut redis_batch = redis_connection.map(|(client, connection)| {
        redis::Batch::new(client, connection, redis_topic, redis_retry_interval)
    });

    let filters = filters::Filters {
        df_filter: options
            .df_filter
            .map(|df| df.into_iter().map(|v| format!("{v}")).collect()),
        aircraft_filter: options.aircraft_filter,
    };

    let mut file = if let Some(output_path) = options.output {
        let output_path = expanduser(PathBuf::from(output_path));
        Some(JsonlWriter::open(output_path, true).await?)
    } else {
        None
    };

    let aircraftdb = aircraft::aircraft().await;

    #[cfg(target_os = "macos")]
    let macos_sleep_changed =
        if !matches!(options.prevent_sleep, PreventSleep::None) {
            check_macos_sleep_setting(options.prevent_sleep).await
        } else {
            false
        };

    let _awake = match options.prevent_sleep {
        PreventSleep::None => None,
        PreventSleep::Warn | PreventSleep::Force => Some(
            keepawake::Builder::default()
                .display(false)
                .idle(true)
                .sleep(true)
                .reason("jet1090 decoding in progress")
                .app_name("jet1090")
                .app_reverse_domain("io.github.jet1090")
                .create()?,
        ),
    };

    let mut aircraft: BTreeMap<ICAO, AircraftState> = BTreeMap::new();

    const SENSOR_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);
    let mut references = BTreeMap::<u64, Option<Position>>::new();
    let mut sensors = BTreeMap::<u64, Sensor>::new();
    for source in options.sources.iter() {
        let source_label = source.health_label();
        tracing::info!(source = source_label, "Discovering sensor metadata");
        match tokio::time::timeout(
            SENSOR_DISCOVERY_TIMEOUT,
            sensor::sensors(source),
        )
        .await
        {
            Ok(Ok(source_sensors)) => {
                for sensor in source_sensors {
                    references.insert(sensor.serial, sensor.reference);
                    sensors.insert(sensor.serial, sensor);
                }
            }
            Ok(Err(error_message)) => {
                error!(
                    source = source_label,
                    error = %error_message,
                    "Sensor discovery failed; continuing without sensor metadata"
                );
            }
            Err(_) => {
                error!(
                    source = source_label,
                    timeout_seconds = SENSOR_DISCOVERY_TIMEOUT.as_secs(),
                    "Sensor discovery timed out; continuing without sensor metadata"
                );
            }
        }
    }

    // Create shared state accessible by all tasks
    let shared = Arc::new(SharedState::new(sensors, health.clone()));
    let shared_dec = shared.clone();
    let shared_web = shared.clone();
    let shared_exp = shared.clone();
    let mut quit_rx = shared_dec.quit_tx.subscribe();
    for source in &options.sources {
        health::register_source(&health, source.health_label());
    }

    // Do not enter the alternate screen until source discovery is complete:
    // discovery can wait on remote services and would otherwise leave a blank TUI.
    let terminal = if options.interactive {
        Some(tui::init()?)
    } else {
        None
    };
    let _terminal_guard = if terminal.is_some() {
        Some(tui::TerminalGuard)
    } else {
        None
    };
    let width = match &terminal {
        Some(terminal) => terminal.size()?.width,
        None => 0,
    };
    let mut events = terminal.is_some().then(|| tui::EventHandler::new(width));

    // Handle signals in headless mode too, so compressed output is finalized.
    {
        let shared_signal = shared.clone();
        let restore_terminal = terminal.is_some();

        #[cfg(unix)]
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            // signal when process is interrupted (ctrl-c)
            let mut sigint = signal(SignalKind::interrupt())
                .expect("Failed to create SIGINT handler");
            // signal when process is terminated (kill -15)
            let mut sigterm = signal(SignalKind::terminate())
                .expect("Failed to create SIGTERM handler");
            // signal when terminal is disconnected
            let mut sighup = signal(SignalKind::hangup())
                .expect("Failed to create SIGHUP handler");

            tokio::select! {
                _ = sigint.recv() => {},
                _ = sigterm.recv() => {},
                _ = sighup.recv() => {},
            }

            // Restore terminal and signal shutdown
            if restore_terminal {
                tui::restore().ok();
            }
            shared_signal.request_quit();
        });

        #[cfg(windows)]
        tokio::spawn(async move {
            use tokio::signal::windows::{
                ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown,
            };

            let mut sig_c = ctrl_c().expect("ctrl_c");
            let mut sig_close = ctrl_close().expect("ctrl_close");
            let mut sig_shutdown = ctrl_shutdown().expect("ctrl_shutdown");
            let mut sig_logoff = ctrl_logoff().expect("ctrl_logoff");

            tokio::select! {
                _ = sig_c.recv() => {},
                _ = sig_close.recv() => {},
                _ = sig_shutdown.recv() => {},
                _ = sig_logoff.recv() => {},
            }

            // Restore terminal and signal shutdown
            if restore_terminal {
                tui::restore().ok();
            }
            shared_signal.request_quit();
        });
    }

    // Create TUI-specific state (only for interactive mode)
    let app_tui = Arc::new(Mutex::new(Jet1090 {
        items: Vec::new(),
        state: TableState::default().with_selected(0),
        scroll_state: ScrollbarState::new(0),
        sort_key: SortKey::default(),
        sort_asc: false,
        width,
        is_search_mode: false,
        search_query: "".to_string(),
        interactive_expire: options.interactive_expire.unwrap_or(30),
        flags: options.flags,
        show_sources: false,
        show_errors: false,
        error_scroll: 0,
    }));

    let mut tui_handle = None;
    if let Some(mut terminal) = terminal {
        // Render synchronously once so a working table is visible before the
        // input task receives its first periodic tick.
        {
            let mut app = app_tui.lock().await;
            let state_vectors = shared.state_vectors.read().await;
            terminal.draw(|frame| {
                table::build_table(frame, &mut app, &shared, &state_vectors)
            })?;
        }

        let app_tui_task = app_tui.clone();
        let shared_tui = shared.clone();
        let mut events =
            events.take().expect("event handler in interactive mode");
        tui_handle = Some(tokio::spawn(async move {
            loop {
                let event = match events.next().await {
                    Ok(event) => event,
                    Err(error) => {
                        error!(error = %error, "Interactive event handler failed");
                        shared_tui.request_quit();
                        break;
                    }
                };
                if let Err(error) =
                    update(&mut *app_tui_task.lock().await, event, &shared_tui)
                {
                    error!(error = %error, "Interactive update failed");
                    shared_tui.request_quit();
                    break;
                }
                let mut app = app_tui_task.lock().await;
                if shared_tui.should_quit.load(Ordering::Relaxed) {
                    break;
                }
                if shared_tui.should_clear.swap(false, Ordering::Relaxed) {
                    // Clearing only removes incidental stdout from SDR backends.
                    // A terminal that cannot be cleared can still render normally.
                    if let Err(error) = terminal.clear() {
                        warn!(error = %error, "Interactive terminal clear failed; continuing");
                    }
                }
                let state_vectors = shared_tui.state_vectors.read().await;
                if let Err(error) = terminal.draw(|frame| {
                    table::build_table(
                        frame,
                        &mut app,
                        &shared_tui,
                        &state_vectors,
                    )
                }) {
                    error!(error = %error, "Interactive terminal draw failed");
                    shared_tui.request_quit();
                    break;
                }
            }
            tui::restore()
        }));
    }

    if let Some(minutes) = options.history_expire {
        // No need to start this task if we don't store history
        if minutes > 0 {
            tokio::spawn(expire_aircraft(shared_exp.clone(), minutes));
        }
    }

    if let Some(port) = options.serve_port {
        tokio::spawn(serve_web_api(shared_web, port));
    }

    // I am not sure whether this size calibration is relevant, but let's try...
    // adding one in order to avoid the stupid error when you set a size = 0
    let multiplier = references.len();
    let (tx, mut rx) = tokio::sync::mpsc::channel(100 * multiplier + 1);
    let (tx_dedup, mut rx_dedup) =
        tokio::sync::mpsc::channel(100 * multiplier + 1);

    // Create shutdown signal channel for coordinated task termination
    let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

    // One ingress observer owns raw-grid evidence; dedup only consumes it.
    let clock_grids = dedup::ClockGrids::default();
    // Collect task handles for graceful shutdown
    let mut source_handles = Vec::new();

    for source in options.sources.into_iter() {
        let serial = source.serial();
        let tx_copy = tx.clone();
        let source_name = source.name.clone();
        let source_label = source.health_label();
        health::register_source(&health, source_label.clone());
        let shutdown_rx = shutdown_tx.subscribe();
        let handle = source.receiver(
            tx_copy,
            serial,
            source_name,
            shutdown_rx,
            health.clone(),
            clock_grids.clone(),
        );
        source_handles.push(handle);
    }

    // Conditionally spawn deduplication task
    if !options.no_deduplication {
        tokio::spawn(async move {
            dedup::deduplicate_messages(
                rx,
                tx_dedup,
                options.deduplication.unwrap_or(450),
                options.reorder_window.unwrap_or(200),
                clock_grids,
            )
            .await;
        });
    } else {
        // Pass through without deduplication, but still decode messages
        tokio::spawn(async move {
            while let Some(mut msg) = rx.recv().await {
                let start = SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("SystemTime before unix epoch")
                    .as_secs_f64();

                if let Ok((_, decoded)) = Message::from_bytes((&msg.frame, 0)) {
                    msg.decode_time = Some(
                        SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .expect("SystemTime before unix epoch")
                            .as_secs_f64()
                            - start,
                    );
                    msg.message = Some(decoded);

                    if tx_dedup.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        });
    }

    let update_reference = reference_update_policy(options.update_position);

    let mut last_reference_update: f64 = 0.0;
    let mut first_msg = true;
    let mut output_failure = None;

    loop {
        if *quit_rx.borrow() {
            break;
        }

        let batch_ready = async {
            match redis_batch.as_ref() {
                Some(batch) => batch.ready().await,
                None => std::future::pending().await,
            }
        };
        let maybe_msg = tokio::select! {
            biased;
            _ = quit_rx.changed() => None,
            _ = batch_ready => {
                if !redis_batch.as_mut().expect("Redis batch ready")
                    .flush_or_quit(&mut quit_rx).await {
                    break;
                }
                continue;
            },
            maybe_msg = rx_dedup.recv() => maybe_msg,
        };

        let Some(mut msg) = maybe_msg else {
            break;
        };
        if first_msg {
            // Do not clear the terminal here: crossterm's clear path queries
            // cursor position and can fail in otherwise functional terminals.
            first_msg = false;
        }

        // Periodically update all sensor references to lowest aircraft position
        if msg.timestamp - last_reference_update > 300.0 {
            for reference in references.values_mut() {
                rs1090::decode::cpr::update_global_reference(
                    &aircraft,
                    reference,
                    msg.timestamp,
                );
            }
            last_reference_update = msg.timestamp;
        }

        decode_sensor_position(
            &mut msg,
            &mut aircraft,
            &mut references,
            &update_reference,
            options.update_position,
        );

        // Sanitize Comm-B messages and record surviving BDS registers
        // for use as evidence in subsequent GICB bitmap validation.
        if let Some(message) = &mut msg.message {
            MessageProcessor::new(message, &mut aircraft)
                .sanitize_commb()
                .record_observed_bds()
                .finish();
        }

        snapshot::update_snapshot(
            &shared_dec,
            &mut msg,
            &aircraftdb,
            &aircraft,
        )
        .await;

        let is_in = filters::Filters::is_in(&filters, &msg);

        if let Ok(json) = serde_json::to_string(&msg) {
            if is_in {
                if options.verbose {
                    println!("{json}");
                }

                if let Some(f) = &mut file {
                    if let Err(error) = f.write_line(&json).await {
                        error!(%error, "File output failed");
                        output_failure = Some(error);
                        shared_dec.request_quit();
                        break;
                    }
                }

                let json = Arc::new(json);
                if let Some(batch) = &mut redis_batch {
                    if batch.needs_flush_before(&json)
                        && !batch.flush_or_quit(&mut quit_rx).await
                    {
                        warn!("Redis flush interrupted; current message was not added to the batch");
                        break;
                    }
                    batch.push(json.clone());
                }

                // Redis ACKs are awaited once per batch, not for each message.
                let _ = shared_dec.stream_tx.send(json);
            }
        }

        match options.history_expire {
            Some(0) => (),
            _ => {
                if is_in {
                    snapshot::store_history(&shared_dec, msg, &aircraftdb).await
                }
            }
        }

        if shared_dec.should_quit.load(Ordering::Relaxed) {
            break;
        }
    }

    let finished_file = if let Some(file) = &mut file {
        file.finish().await
    } else {
        Ok(())
    };
    let output_result = match output_failure {
        Some(error) => Err(error),
        None => finished_file,
    };

    let redis_result = match redis_batch {
        Some(batch) => batch.finish().await,
        None => Ok(()),
    };
    if let Err(error) = &redis_result {
        warn!(%error, "Redis output did not finish cleanly");
    }

    // Signal all source tasks and the TUI to shut down.
    shared_dec.request_quit();
    let _ = shutdown_tx.send(());

    // Wait for all source tasks to finish with timeout
    let timeout = Duration::from_secs(2);
    for handle in source_handles {
        let _ = tokio::time::timeout(timeout, handle).await;
    }
    if let Some(handle) = tui_handle {
        let _ = tokio::time::timeout(timeout, handle).await;
    }
    if options.interactive {
        tui::restore().ok();
        health::print_report(&health);
    }

    #[cfg(target_os = "macos")]
    if macos_sleep_changed {
        restore_macos_sleep_setting().await;
    }
    output_result?;
    redis_result?;
    Ok(())
}

async fn expire_aircraft(shared: Arc<SharedState>, minutes: u64) {
    loop {
        sleep(Duration::from_secs(60)).await;
        {
            let mut state_vectors = shared.state_vectors.write().await;
            let now = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("SystemTime before unix epoch")
                .as_secs();

            let remove_keys = state_vectors
                .iter()
                .filter(|(_key, value)| now > value.cur.lastseen + minutes * 60)
                .map(|(key, _)| key.to_string())
                .collect::<Vec<String>>();

            for key in remove_keys {
                state_vectors.remove(&key);
            }

            let _ = state_vectors
                .iter_mut()
                .map(|(_key, value)| {
                    value.hist.retain(|elt| {
                        now < (elt.timestamp as u64) + minutes * 60
                    })
                })
                .collect::<Vec<()>>();
        }
    }
}
/// Shared application state split into components to reduce lock contention
#[derive(Debug, Default)]
pub struct Jet1090 {
    // TUI-specific state (only accessed by TUI task, kept in Mutex)
    state: TableState,
    items: Vec<String>,
    scroll_state: ScrollbarState,
    sort_key: SortKey,
    sort_asc: bool,
    width: u16,
    is_search_mode: bool,
    search_query: String,
    interactive_expire: u64,
    flags: bool,
    show_sources: bool,
    show_errors: bool,
    error_scroll: usize,
}

/// Shared state that multiple tasks need to access
#[derive(Debug)]
pub struct SharedState {
    /// Aircraft state vectors - read-heavy (RwLock for concurrent reads)
    state_vectors: Arc<RwLock<BTreeMap<String, snapshot::StateVectors>>>,
    /// Sensor information - read-only after initialization
    sensors: BTreeMap<u64, Sensor>,
    /// Quit flag - lock-free atomic
    should_quit: Arc<AtomicBool>,
    /// Quit notification for tasks blocked on async receives
    quit_tx: watch::Sender<bool>,
    /// Clear screen flag - lock-free atomic
    should_clear: Arc<AtomicBool>,
    /// Recent operational events and source health for the interactive UI
    health: SharedHealth,
    /// Serialized messages for /stream subscribers
    stream_tx: broadcast::Sender<Arc<String>>,
}

/// How far a /stream client may fall behind before it is moved to the live
/// edge of the feed and the messages in between are dropped
const STREAM_BUFFER: usize = 4096;

impl SharedState {
    fn new(sensors: BTreeMap<u64, Sensor>, health: SharedHealth) -> Self {
        let (quit_tx, _quit_rx) = watch::channel(false);
        let (stream_tx, _stream_rx) = broadcast::channel(STREAM_BUFFER);

        Self {
            state_vectors: Arc::new(RwLock::new(BTreeMap::new())),
            sensors,
            should_quit: Arc::new(AtomicBool::new(false)),
            quit_tx,
            should_clear: Arc::new(AtomicBool::new(false)),
            health,
            stream_tx,
        }
    }

    fn request_quit(&self) {
        self.should_quit.store(true, Ordering::Relaxed);
        let _ = self.quit_tx.send(true);
    }
}

#[derive(Debug, Default, PartialEq)]
pub enum SortKey {
    CALLSIGN,
    ALTITUDE,
    VRATE,
    #[default]
    COUNT,
    FIRST,
    LAST,
}

fn update(
    jet1090: &mut Jet1090,
    event: Event,
    shared: &SharedState,
) -> std::io::Result<()> {
    match event {
        Event::Key(key) => {
            use KeyCode::*;

            match (jet1090.is_search_mode, key.code) {
                (true, Char(c)) => jet1090.search_query.push(c),
                (true, Backspace) => {
                    jet1090.search_query.pop();
                }
                (true, Enter) => jet1090.is_search_mode = false,
                (true, Esc) => {
                    jet1090.is_search_mode = false;
                    jet1090.search_query = "".to_string()
                }
                (false, Char('j')) | (_, Down) => jet1090.next(),
                (false, Char('k')) | (_, Up) => jet1090.previous(),
                (false, Char('g')) | (_, PageUp) | (_, Home) => jet1090.home(),
                (false, Char('q')) => shared.request_quit(),
                (false, Esc) if !jet1090.show_errors => shared.request_quit(),
                (false, Char('a')) => jet1090.sort_key = SortKey::ALTITUDE,
                (false, Char('c')) => jet1090.sort_key = SortKey::CALLSIGN,
                (false, Char('v')) => jet1090.sort_key = SortKey::VRATE,
                (false, Char('.')) => jet1090.sort_key = SortKey::COUNT,
                (false, Char('f')) => jet1090.sort_key = SortKey::FIRST,
                (false, Char('l')) => jet1090.sort_key = SortKey::LAST,
                (false, Char('-')) => jet1090.sort_asc = !jet1090.sort_asc,
                (false, Char('/')) => jet1090.is_search_mode = true,
                (false, Char('s')) | (false, Char('S')) => {
                    jet1090.show_sources = !jet1090.show_sources;
                }
                (false, Char('e')) => {
                    jet1090.show_errors = !jet1090.show_errors;
                    if jet1090.show_errors {
                        health::mark_events_read(&shared.health);
                    }
                }
                (false, Char('E')) => {
                    jet1090.show_errors = false;
                    health::mark_events_read(&shared.health);
                }
                (false, Char('J')) if jet1090.show_errors => {
                    jet1090.error_scroll =
                        jet1090.error_scroll.saturating_add(1)
                }
                (false, Char('K')) if jet1090.show_errors => {
                    jet1090.error_scroll =
                        jet1090.error_scroll.saturating_sub(1)
                }
                (false, Esc) if jet1090.show_errors => {
                    jet1090.show_errors = false;
                    health::mark_events_read(&shared.health);
                }
                _ => {}
            }
        }
        Event::Tick(size) => jet1090.width = size,
        _ => {}
    }
    Ok(())
}

impl Jet1090 {
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i);
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i);
    }
    pub fn home(&mut self) {
        self.state.select(Some(0));
        self.scroll_state = self.scroll_state.position(0);
    }
}

fn print_completions<G: Generator>(gen: G, cmd: &mut Command) {
    generate(gen, cmd, cmd.get_name().to_string(), &mut io::stdout());
}

#[cfg(test)]
mod tests {

    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn press(app: &mut Jet1090, shared: &SharedState, key: KeyCode) {
        update(
            app,
            Event::Key(KeyEvent::new(key, KeyModifiers::NONE)),
            shared,
        )
        .unwrap();
    }

    #[test]
    fn sources_toggle_without_acknowledging_errors() {
        let shared = SharedState::new(BTreeMap::new(), health::new_health());
        shared.health.lock().unwrap().unread_errors = 2;
        let mut app = Jet1090::default();
        assert!(!app.show_sources);
        press(&mut app, &shared, KeyCode::Char('s'));
        assert!(app.show_sources);
        assert_eq!(shared.health.lock().unwrap().unread_errors, 2);
        press(&mut app, &shared, KeyCode::Char('e'));
        assert!(app.show_errors);
        assert!(app.show_sources);
        press(&mut app, &shared, KeyCode::Esc);
        assert!(!app.show_errors);
        assert!(app.show_sources);
        assert!(!shared.should_quit.load(Ordering::Relaxed));
        press(&mut app, &shared, KeyCode::Char('S'));
        assert!(!app.show_sources);
    }

    #[test]
    fn search_treats_sources_keys_as_text() {
        let shared = SharedState::new(BTreeMap::new(), health::new_health());
        let mut app = Jet1090::default();
        for key in ['/', 's', 'S'] {
            press(&mut app, &shared, KeyCode::Char(key));
        }
        assert!(app.is_search_mode);
        assert_eq!(app.search_query, "sS");
        assert!(!app.show_sources);
        press(&mut app, &shared, KeyCode::Enter);
        press(&mut app, &shared, KeyCode::Char('S'));
        assert!(app.show_sources);
        assert_eq!(app.search_query, "sS");
    }

    #[test]
    fn sources_can_be_opened_and_navigated_without_aircraft() {
        let shared = SharedState::new(BTreeMap::new(), health::new_health());
        let mut app = Jet1090::default();
        app.state.select(Some(0));
        press(&mut app, &shared, KeyCode::Char('s'));
        for key in [
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Home,
        ] {
            press(&mut app, &shared, key);
        }
        assert!(app.show_sources);
        assert!(!shared.should_quit.load(Ordering::Relaxed));
    }

    #[test]
    fn sleep_flag_is_optional_and_explicit_modes_still_work() {
        let options = Options::try_parse_from(["jet1090"]).unwrap();
        assert!(matches!(options.prevent_sleep, PreventSleep::None));
        for mode in ["none", "warn", "force"] {
            let options =
                Options::try_parse_from(["jet1090", "--prevent-sleep", mode])
                    .unwrap();
            assert_eq!(
                options
                    .prevent_sleep
                    .to_possible_value()
                    .unwrap()
                    .get_name(),
                mode
            );
        }
        assert!(Options::try_parse_from([
            "jet1090",
            "--prevent-sleep",
            "invalid"
        ])
        .is_err());
    }

    #[test]
    fn legacy_sleep_booleans_and_string_modes_load() {
        for (value, expected) in [
            ("true", "warn"),
            ("false", "none"),
            ("\"none\"", "none"),
            ("\"warn\"", "warn"),
            ("\"force\"", "force"),
        ] {
            let options: Options =
                toml::from_str(&format!("prevent_sleep = {value}")).unwrap();
            assert_eq!(
                options
                    .prevent_sleep
                    .to_possible_value()
                    .unwrap()
                    .get_name(),
                expected
            );
        }
        assert!(matches!(
            toml::from_str::<Options>("").unwrap().prevent_sleep,
            PreventSleep::None
        ));
        for value in ["42", "\"invalid\"", "[]"] {
            assert!(toml::from_str::<Options>(&format!(
                "prevent_sleep = {value}"
            ))
            .is_err());
        }
    }

    #[test]
    fn test_config() {
        let options: Options = toml::from_str(
            r#"
            verbose = false
            interactive = true
            serve_port = 8080
            no_history_expire = true
            prevent_sleep = "warn"
            update_position = false

            [[sources]]
            udp = "0.0.0.0:1234"
            airport = 'LFBO'

            [[sources]]
            udp = "0.0.0.0:3456"
            latitude = 48.723
            longitude = 2.379
            "#,
        )
        .unwrap();

        assert!(options.interactive);
        assert!(options.history_expire.is_none());
        assert_eq!(options.sources.len(), 2);
    }
}
