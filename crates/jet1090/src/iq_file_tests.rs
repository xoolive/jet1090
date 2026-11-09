//! Hardware-free source and real-CLI regressions. Build jet1090, then run the
//! ignored `cli_iq_` tests with JET1090_TEST_BINARY set to its absolute path.
//! Every child gets an isolated environment/config and a tiny aircraft cache;
//! no user cache, network download, Redis or physical receiver is needed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::{health, source::Source};

const CAPTURE: &str = "gqrx_20261109_120000_1090000000_6000000_fc.raw";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "jet1090-iq-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        // Stop dotenv's ancestor search even if a parent contains a .env file.
        std::fs::write(path.join(".env"), "").unwrap();
        Self(path)
    }

    fn capture(
        &self,
        directory: &str,
        bytes: &[u8],
        compressed: bool,
    ) -> PathBuf {
        let directory = self.0.join(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(if compressed {
            format!("{CAPTURE}.zst")
        } else {
            CAPTURE.to_owned()
        });
        let bytes = if compressed {
            zstd::stream::encode_all(bytes, 1).unwrap()
        } else {
            bytes.to_vec()
        };
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn command(&self, inputs: &[&Path]) -> Command {
        // Seed a minimal, valid BaseStation cache for startup. This deliberately
        // does not read the developer's cache or download the aircraft database.
        let sql_path = self.0.join("fixture.sqb");
        let connection = rusqlite::Connection::open(&sql_path).unwrap();
        connection
            .execute_batch("CREATE TABLE Aircraft (ModeS TEXT, Registration TEXT, ICAOTypeCode TEXT);")
            .unwrap();
        drop(connection);
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("fixture.sqb", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&std::fs::read(sql_path).unwrap()).unwrap();
        let zip = zip.finish().unwrap().into_inner();
        let home = self.0.join("home");
        for cache in [self.0.join("cache"), home.join("Library/Caches")] {
            let cache = cache.join("jet1090");
            std::fs::create_dir_all(&cache).unwrap();
            std::fs::write(cache.join("basestation.zip"), &zip).unwrap();
        }
        let mut command = Command::new(
            std::env::var_os("JET1090_TEST_BINARY")
                .expect("set JET1090_TEST_BINARY to the freshly built jet1090"),
        );
        command
            .env_clear()
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_CACHE_HOME", self.0.join("cache"))
            // Input failures must remain failures even if tracing is disabled.
            .env("RUST_LOG", "off")
            .current_dir(&self.0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .arg("--verbose")
            .args(inputs.iter().map(|path| {
                url::Url::from_file_path(path).unwrap().to_string()
            }));
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(mut command: Command) -> Output {
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("IQ CLI did not terminate: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

fn frames(output: &Output, success: bool, count: usize) {
    assert_eq!(output.status.success(), success, "{output:?}");
    let lines: Vec<serde_json::Value> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), count, "{output:?}");
    for message in lines {
        assert_eq!(message["frame"], "8d40621d58c382d690c8ac2863a7");
    }
    if !success {
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("IQ file"),
            "{output:?}"
        );
    }
}

fn iq_bytes() -> Vec<u8> {
    let frame = hex::decode("8d40621d58c382d690c8ac2863a7").unwrap();
    let mut samples = vec![0.0_f32; 2 * 3400];
    for start in [200, 1200, 2500] {
        for offset in [0, 6, 21, 27] {
            for i in start + offset..start + offset + 3 {
                samples[2 * i] = 0.9;
            }
        }
        for bit in 0..112 {
            let half = if frame[bit / 8] & (1 << (7 - bit % 8)) != 0 {
                0
            } else {
                3
            };
            let offset = start + 48 + bit * 6 + half;
            for i in offset..offset + 3 {
                samples[2 * i] = 0.9;
            }
        }
    }
    samples.into_iter().flat_map(f32::to_le_bytes).collect()
}

#[test]
#[ignore = "requires JET1090_TEST_BINARY; exercised by Rust CI after build"]
fn cli_iq_empty_eof_with_and_without_dedup() {
    for passthrough in [false, true] {
        let fixture = Fixture::new();
        let mut path = fixture.capture("empty", &[], false);
        if passthrough {
            // Unknown name: also cover the default 2.4 MS/s/cu8 and
            // approximate file-time origin rather than filename metadata.
            let default_path = fixture.0.join("empty.iq");
            std::fs::rename(&path, &default_path).unwrap();
            path = default_path;
        }
        let mut command = fixture.command(&[&path]);
        if passthrough {
            command.arg("--no-deduplication");
        }
        frames(&run(command), true, 0);
    }
}

#[test]
#[ignore = "requires JET1090_TEST_BINARY; exercised by Rust CI after build"]
fn cli_iq_raw_and_zstd_drain_and_finalize_output() {
    for compressed in [false, true] {
        let fixture = Fixture::new();
        let path = fixture.capture("valid", &iq_bytes(), compressed);
        let output_path = fixture.0.join("output.jsonl.zst");
        let mut command = fixture.command(&[&path]);
        command.arg("--output").arg(&output_path);
        let output = run(command);
        frames(&output, true, 3);
        let written =
            zstd::stream::decode_all(std::fs::File::open(output_path).unwrap())
                .expect(
                    "the output's compressed stream must be finalized at EOF",
                );
        assert_eq!(written, output.stdout);
    }
}

#[test]
#[ignore = "requires JET1090_TEST_BINARY; exercised by Rust CI after build"]
fn cli_iq_truncated_raw_keeps_accepted_frames_and_returns_failure() {
    let fixture = Fixture::new();
    let mut bytes = iq_bytes();
    bytes.push(0); // An incomplete cf32 pair after three complete packets.
    let path = fixture.capture("truncated", &bytes, false);
    let output_path = fixture.0.join("accepted.jsonl.zst");
    let mut command = fixture.command(&[&path]);
    command.arg("--output").arg(&output_path);
    let output = run(command);
    frames(&output, false, 3);
    assert_eq!(
        zstd::stream::decode_all(std::fs::File::open(output_path).unwrap())
            .unwrap(),
        output.stdout
    );
}

#[test]
#[ignore = "requires JET1090_TEST_BINARY; exercised by Rust CI after build"]
fn cli_iq_missing_and_corrupt_compressed_files_fail() {
    for missing in [false, true] {
        let fixture = Fixture::new();
        let path = fixture.capture("bad", &[], true);
        if missing {
            std::fs::remove_file(&path).unwrap();
        } else {
            std::fs::write(&path, b"not a zstd stream").unwrap();
        }
        let output = run(fixture.command(&[&path]));
        frames(&output, false, 0);
        assert!(String::from_utf8_lossy(&output.stderr).contains(CAPTURE));
    }
}

#[test]
#[ignore = "requires JET1090_TEST_BINARY; exercised by Rust CI after build"]
fn cli_iq_multiple_inputs_drain_even_if_one_fails() {
    for missing in [false, true] {
        let fixture = Fixture::new();
        // Exactly one identical packet per receiver: with dedup accidentally
        // enabled these must merge, independent of producer scheduling.
        let bytes = iq_bytes();
        let first = fixture.capture("first", &bytes[..1100 * 8], false);
        let second = fixture.capture("second", &bytes[..1100 * 8], false);
        if missing {
            std::fs::remove_file(&first).unwrap();
        }
        let mut command = fixture.command(&[&first, &second]);
        command.arg("--no-deduplication");
        frames(&run(command), !missing, if missing { 1 } else { 2 });
    }
}

#[test]
#[ignore = "requires JET1090_TEST_BINARY; exercised by Rust CI after build"]
fn cli_iq_truncated_zstd_returns_failure() {
    let fixture = Fixture::new();
    let path = fixture.capture("truncated-zstd", &iq_bytes(), true);
    let mut compressed = std::fs::read(&path).unwrap();
    compressed.pop();
    std::fs::write(&path, compressed).unwrap();
    let output = run(fixture.command(&[&path]));
    // Codec buffering determines which samples have been delivered before the
    // error. Check the error, not an assumed recoverable prefix length.
    let delivered =
        std::str::from_utf8(&output.stdout).unwrap().lines().count();
    frames(&output, false, delivered);
}

#[test]
#[ignore = "requires JET1090_TEST_BINARY; exercised by Rust CI after build"]
fn cli_iq_invalid_file_parameters_fail_without_panic() {
    for setting in ["sample_rate = 1000000", "iq_format = 'invalid'"] {
        let fixture = Fixture::new();
        let path = fixture.capture("invalid-parameters", &[], false);
        let config_dir = fixture.0.join("config/jet1090");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                "[[sources]]\nfile = {}\n{setting}\n",
                toml::Value::String(path.to_str().unwrap().to_owned())
            ),
        )
        .unwrap();
        let output = run(fixture.command(&[]));
        frames(&output, false, 0);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}

#[tokio::test]
#[ignore = "requires JET1090_TEST_BINARY; exercised by Rust CI after build"]
async fn cli_iq_web_session_remains_open_until_signalled() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let fixture = Fixture::new();
    let path = fixture.capture("web", &iq_bytes(), false);
    let mut command = tokio::process::Command::from(fixture.command(&[&path]));
    let mut child = command
        .arg("--serve-port")
        .arg("0")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first =
        tokio::time::timeout(Duration::from_secs(10), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&first).is_ok());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        child.try_wait().unwrap().is_none(),
        "web session closed at EOF"
    );
    assert!(Command::new("/bin/kill")
        .arg("-TERM")
        .arg(child.id().unwrap().to_string())
        .status()
        .unwrap()
        .success());
    assert!(tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

#[tokio::test]
async fn file_setup_cancellation_is_not_an_input_failure() {
    let fixture = Fixture::new();
    let path = fixture.0.join(CAPTURE); // Never opened: already cancelled.
    let source: Source = url::Url::from_file_path(path)
        .unwrap()
        .as_str()
        .parse()
        .unwrap();
    let health = health::new_health();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let (shutdown, shutdown_rx) = tokio::sync::broadcast::channel(1);
    shutdown.send(()).unwrap();
    let task = source.receiver(
        tx,
        source.serial(),
        None,
        shutdown_rx,
        health.clone(),
        crate::dedup::ClockGrids::default(),
    );
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert!(rx.recv().await.is_none());
    assert!(health::input_result(&health).is_ok());
}
