#![doc = include_str!("../readme.md")]

mod dedup;
mod filters;
mod sensor;
mod shell;
mod snapshot;
mod source;
mod table;
mod tui;
mod web;

use crate::tui::Event;
use crate::web::serve_web_api;
use clap::{Command, CommandFactory, Parser, ValueHint};
use clap_complete::{generate, Generator};
use crossterm::event::KeyCode;
use ratatui::widgets::*;
use redis::AsyncCommands;
use rs1090::data::aircraft;
use rs1090::decode::commb::MessageProcessor;
use rs1090::decode::cpr::{decode_position, AircraftState};
use rs1090::decode::serialize_config;
use rs1090::prelude::*;
use sensor::Sensor;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Default, Deserialize, Parser)]
#[command(
    name = "jet1090",
    version,
    author = "xoolive",
    about = "Decode and serve Mode S demodulated raw messages"
)]
struct Options {
    /// Activate JSON output
    #[arg(short, long, default_value = "false")]
    verbose: bool,

    /// Dump a copy of the received messages as .jsonl
    #[arg(short, long, default_value=None, value_hint=ValueHint::FilePath)]
    output: Option<String>,

    /// Display a table in interactive mode (not compatible with verbose)
    #[arg(short, long, default_value = "false")]
    interactive: bool,

    /// Port for the API endpoint (on 0.0.0.0)
    #[arg(long, default_value=None)]
    serve_port: Option<u16>,

    /// How much history to expire (in minutes), 0 for no history
    #[arg(
        long,
        env = "EXPIRE_AIRCRAFT",
        short = 'x',
        default_value = "15",
        conflicts_with = "no_history_expire"
    )]
    history_expire: Option<u64>,

    /// Disable history expiration
    #[arg(long, conflicts_with = "history_expire")]
    no_history_expire: Option<bool>,

    /// Downlink formats to select for stdout, file output and history in REST API (keep empty to select all)
    #[arg(long, value_name = "DF")]
    df_filter: Option<Vec<u16>>,

    /// Aircraft addresses to select for stdout, file output and history in REST API (keep empty to select all)
    #[arg(long, value_name = "ICAO24")]
    aircraft_filter: Option<Vec<ICAO>>,

    /// Prevent the computer sleeping when decoding is in progress
    #[arg(long, default_value=None)]
    prevent_sleep: bool,

    /// Should we update the reference positions (if the receiver is moving)
    #[arg(short, long, default_value=None)]
    update_position: bool,

    /// When performing deduplication, after how long to dump deduplicated messages (time in ms)
    #[arg(long, default_value = "450")]
    deduplication: Option<u32>,

    #[arg(long)]
    stats: Option<bool>,

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
    /// `host` can be a DNS name, an IP address or `rtlsdr` (for RTL-SDR dongles),
    /// `port` must be a number,
    /// `reference` can be LFPG for major airports, `43.3,1.35` otherwise.
    ///
    /// More details are available at: <https://mode-s.org/jet1090/sources>
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
}

fn expanduser(path: PathBuf) -> PathBuf {
    // Check if the path starts with "~"
    if let Some(stripped) = path.to_str().and_then(|p| p.strip_prefix("~")) {
        if let Some(home_dir) = dirs::home_dir() {
            // Join the home directory with the rest of the path
            return home_dir.join(stripped.trim_start_matches('/'));
        }
    }
    path
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
        let string = fs::read_to_string(cfg_path).await.ok().unwrap();
        options = toml::from_str(&string).unwrap();
    }

    if let Ok(config_file) = std::env::var("JET1090_CONFIG") {
        let path = expanduser(PathBuf::from(config_file));
        let string = fs::read_to_string(path)
            .await
            .expect("Configuration file not found");
        options = toml::from_str(&string).unwrap();
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
    if cli_options.serve_port.is_some() {
        options.serve_port = cli_options.serve_port;
    }
    if cli_options.no_history_expire.is_some_and(|x| x) {
        options.history_expire = None;
    } else if let Some(history_expire) = cli_options.history_expire {
        // If `history_expire` is the default value (15) and `no_history_expire` is true, set to None
        if history_expire == 15
            && cli_options.no_history_expire.is_some_and(|x| x)
        {
            options.history_expire = None;
        } else {
            options.history_expire = Some(history_expire);
        }
    }
    if cli_options.df_filter.is_some() {
        options.df_filter = cli_options.df_filter;
    }
    if cli_options.aircraft_filter.is_some() {
        options.aircraft_filter = cli_options.aircraft_filter;
    }
    if cli_options.prevent_sleep {
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
    if cli_options.stats.is_some() {
        options.stats = cli_options.stats;
    }
    if cli_options.deduplication.is_some() {
        options.deduplication = cli_options.deduplication;
    }
    if options.stats.unwrap_or(false) {
        serialize_config(true);
    }

    options.sources.append(&mut cli_options.sources);

    // example: RUST_LOG=rs1090=DEBUG
    let env_filter = EnvFilter::from_default_env();

    let subscriber = tracing_subscriber::registry().with(env_filter);
    match options.log_file.as_deref() {
        Some("-") if !cli_options.interactive => {
            // when it's interactive, logs will disrupt the display
            subscriber.with(fmt::layer().pretty()).init();
        }
        Some(log_file) if log_file != "-" => {
            let file = std::fs::File::create(log_file).unwrap_or_else(|_| {
                panic!("fail to create log file: {log_file}")
            });
            let file_layer = fmt::layer().with_writer(file).with_ansi(false);
            subscriber.with(file_layer).init();
        }
        _ => {
            subscriber.init(); // no logging
        }
    }

    if options.sources.is_empty() {
        eprintln!(
            "No source of data specified, use --help for more information"
        );
        std::process::exit(1);
    }

    let mut redis_connect = match options
        .redis_url
        .map(|url| redis::Client::open(url).unwrap())
    {
        // map is not possible because of the .await (the async context thing)
        Some(c) => Some(
            c.get_multiplexed_async_connection()
                .await
                .expect("Unable to connect to the Redis server"),
        ),
        None => None,
    };
    let redis_topic = options.redis_topic.unwrap_or("jet1090".to_string());

    let filters = filters::Filters {
        df_filter: options
            .df_filter
            .map(|df| df.into_iter().map(|v| format!("{v}")).collect()),
        aircraft_filter: options.aircraft_filter,
    };

    let mut file = if let Some(output_path) = options.output {
        let output_path = expanduser(PathBuf::from(output_path));
        Some(
            fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(output_path)
                .await?,
        )
    } else {
        None
    };

    let aircraftdb = aircraft::aircraft().await;

    let _awake = match options.prevent_sleep {
        true => Some(
            keepawake::Builder::default()
                .display(false)
                .idle(true)
                .sleep(true)
                .reason("jet1090 decoding in progress")
                .app_name("jet1090")
                .app_reverse_domain("io.github.jet1090")
                .create()?,
        ),
        false => None,
    };

    let mut aircraft: BTreeMap<ICAO, AircraftState> = BTreeMap::new();

    let terminal = if options.interactive {
        Some(tui::init()?)
    } else {
        None
    };
    let width = if let Some(terminal) = &terminal {
        terminal.size()?.width
    } else {
        0
    };

    let mut events = tui::EventHandler::new(width);

    let mut references = BTreeMap::<u64, Option<Position>>::new();
    let mut sensors = BTreeMap::<u64, Sensor>::new();
    for source in options.sources.iter() {
        for sensor in sensor::sensors(source).await {
            references.insert(sensor.serial, sensor.reference);
            sensors.insert(sensor.serial, sensor);
        }
    }
    let app_tui = Arc::new(Mutex::new(Jet1090 {
        sensors,
        items: Vec::new(),
        state: TableState::default().with_selected(0),
        scroll_state: ScrollbarState::new(0),
        should_quit: false,
        should_clear: false,
        state_vectors: BTreeMap::new(),
        sort_key: SortKey::default(),
        sort_asc: false,
        width,
        is_search_mode: false,
        search_query: "".to_string(),
    }));
    let app_dec = app_tui.clone();
    let app_web = app_tui.clone();
    let app_exp = app_tui.clone();

    if let Some(mut terminal) = terminal {
        tokio::spawn(async move {
            loop {
                if let Ok(event) = events.next().await {
                    update(&mut app_tui.lock().await, event)?;
                }
                let mut app = app_tui.lock().await;
                if app.should_quit {
                    break;
                }
                if app.should_clear {
                    terminal.clear()?;
                    app.should_clear = false;
                }
                terminal.draw(|frame| table::build_table(frame, &mut app))?;
            }
            tui::restore()
        });
    }

    if let Some(minutes) = options.history_expire {
        // No need to start this task if we don't store history
        if minutes > 0 {
            tokio::spawn(expire_aircraft(app_exp.clone(), minutes));
        }
    }

    if let Some(port) = options.serve_port {
        tokio::spawn(serve_web_api(app_web, port));
    }

    // I am not sure whether this size calibration is relevant, but let's try...
    // adding one in order to avoid the stupid error when you set a size = 0
    let multiplier = references.len();
    let (tx, rx) = tokio::sync::mpsc::channel(100 * multiplier + 1);
    let (tx_dedup, mut rx_dedup) =
        tokio::sync::mpsc::channel(100 * multiplier + 1);

    for source in options.sources.into_iter() {
        let serial = source.serial();
        let tx_copy = tx.clone();
        let source_name = source.name.clone();
        source.receiver(tx_copy, serial, source_name);
    }

    tokio::spawn(async move {
        dedup::deduplicate_messages(
            rx,
            tx_dedup,
            options.deduplication.unwrap_or(450),
        )
        .await;
    });

    // If we choose to update the reference (only useful for surface positions)
    // then we define the callback (for now, if the altitude is below 5000ft)
    let update_reference = match options.update_position {
        true => Some(Box::new(|pos: &AirbornePosition| {
            pos.alt.is_some_and(|alt| alt < 5000)
        }) as Box<dyn Fn(&AirbornePosition) -> bool>),
        false => None,
    };

    let mut last_reference_update: f64 = 0.0;
    let mut first_msg = true;
    while let Some(mut msg) = rx_dedup.recv().await {
        if first_msg {
            // This workaround results from soapysdr writing directly on stdout.
            // The best thing would be to not write to stdout in the first
            // place. A better workaround would be to condition that clear to
            // the first message received from rtlsdr.

            app_dec.lock().await.should_clear = true;
            first_msg = false;
        }

        // Periodically update all sensor references to lowest aircraft position
        if msg.timestamp - last_reference_update > 300.0 {
            for (_, reference) in references.iter_mut() {
                rs1090::decode::cpr::update_global_reference(
                    &aircraft,
                    reference,
                    msg.timestamp,
                );
            }
            last_reference_update = msg.timestamp;
        }

        if let Some(message) = &mut msg.message {
            match &mut message.df {
                ExtendedSquitterADSB(adsb) => match adsb.message {
                    ME::BDS05 { .. } | ME::BDS06 { .. } => {
                        let serial = msg
                            .metadata
                            .first()
                            .map(|meta| meta.serial)
                            .unwrap();
                        let mut reference = references[&serial];

                        decode_position(
                            &mut adsb.message,
                            msg.timestamp,
                            &adsb.icao24,
                            &mut aircraft,
                            &mut reference,
                            &update_reference,
                        );

                        // References may have been modified.
                        // With static receivers, we don't care.
                        // With dynamic ones, we may want to update the reference position.
                        if options.update_position {
                            for meta in &msg.metadata {
                                let _ =
                                    references.insert(meta.serial, reference);
                            }
                        }
                    }
                    _ => {}
                },
                ExtendedSquitterTisB { cf, .. } => match cf.me {
                    ME::BDS05 { .. } | ME::BDS06 { .. } => {
                        let serial = msg
                            .metadata
                            .first()
                            .map(|meta| meta.serial)
                            .unwrap();

                        let mut reference = references[&serial];

                        decode_position(
                            &mut cf.me,
                            msg.timestamp,
                            &cf.aa,
                            &mut aircraft,
                            &mut reference,
                            &update_reference,
                        )
                    }
                    _ => {}
                },
                _ => {}
            }
        };

        // Sanitize Comm-B messages before updating snapshot
        if let Some(message) = &mut msg.message {
            MessageProcessor::new(message, &aircraft)
                .sanitize_commb()
                .finish();
        }

        snapshot::update_snapshot(&app_dec, &mut msg, &aircraftdb, &aircraft)
            .await;

        let is_in = filters::Filters::is_in(&filters, &msg);

        if let Ok(json) = serde_json::to_string(&msg) {
            if is_in {
                if options.verbose {
                    println!("{json}");
                }

                if let Some(file) = &mut file {
                    file.write_all(json.as_bytes()).await?;
                    file.write_all("\n".as_bytes()).await?;
                }

                if let Some(c) = &mut redis_connect {
                    let _: () = c.publish(redis_topic.clone(), json).await?;
                }
            }
        }

        match options.history_expire {
            Some(0) => (),
            _ => {
                if is_in {
                    snapshot::store_history(&app_dec, msg, &aircraftdb).await
                }
            }
        }

        if app_dec.lock().await.should_quit {
            break;
        }
    }
    Ok(())
}

async fn expire_aircraft(app_expire: Arc<Mutex<Jet1090>>, minutes: u64) {
    loop {
        sleep(Duration::from_secs(60)).await;
        {
            let mut app = app_expire.lock().await;
            let now = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("SystemTime before unix epoch")
                .as_secs();

            let remove_keys = app
                .state_vectors
                .iter()
                .filter(|(_key, value)| now > value.cur.lastseen + minutes * 60)
                .map(|(key, _)| key.to_string())
                .collect::<Vec<String>>();

            for key in remove_keys {
                app.state_vectors.remove(&key);
            }

            let _ = app
                .state_vectors
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
#[derive(Debug, Default)]
pub struct Jet1090 {
    sensors: BTreeMap<u64, Sensor>,
    state: TableState,
    items: Vec<String>,
    scroll_state: ScrollbarState,
    should_quit: bool,
    should_clear: bool,
    state_vectors: BTreeMap<String, snapshot::StateVectors>,
    sort_key: SortKey,
    sort_asc: bool,
    width: u16,
    is_search_mode: bool,
    search_query: String,
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
    jet1090: &mut tokio::sync::MutexGuard<Jet1090>,
    event: Event,
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
                (false, Char('q')) | (false, Esc) => jet1090.should_quit = true,
                (false, Char('a')) => jet1090.sort_key = SortKey::ALTITUDE,
                (false, Char('c')) => jet1090.sort_key = SortKey::CALLSIGN,
                (false, Char('v')) => jet1090.sort_key = SortKey::VRATE,
                (false, Char('.')) => jet1090.sort_key = SortKey::COUNT,
                (false, Char('f')) => jet1090.sort_key = SortKey::FIRST,
                (false, Char('l')) => jet1090.sort_key = SortKey::LAST,
                (false, Char('-')) => jet1090.sort_asc = !jet1090.sort_asc,
                (false, Char('/')) => jet1090.is_search_mode = true,
                _ => {}
            }
        }
        Event::Tick(size) => jet1090.width = size,
        _ => {}
    }
    Ok(())
}

impl Jet1090 {
    pub fn receivers(&mut self) {
        for sensor in self.sensors.values_mut() {
            sensor.aircraft_count = 0;
        }
        for vector in self.state_vectors.values_mut() {
            for sensor in &vector.cur.metadata {
                if let Some(src) = self.sensors.get_mut(&sensor.serial) {
                    src.aircraft_count += 1;
                    src.last_timestamp = vector.cur.lastseen
                }
            }
        }
    }
    pub fn keys(&self) -> Result<impl warp::Reply, std::convert::Infallible> {
        let keys: Vec<_> = self
            .state_vectors
            .keys()
            .map(|key| key.to_string())
            .collect();
        Ok(warp::reply::json(&keys))
    }
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
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
                    self.items.len() - 1
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

    use crate::Options;

    #[test]
    fn test_config() {
        let options: Options = toml::from_str(
            r#"
            verbose = false
            interactive = true
            serve_port = 8080
            no_history_expire = true
            prevent_sleep = false
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
