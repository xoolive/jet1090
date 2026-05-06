#![doc = include_str!("../readme.md")]

mod bds;
mod beast;
mod cat48;
mod error;
mod file;

use beast::{bds_code_from_decoded, extract_beast_frame};
use clap::{Parser, Subcommand};
use deku::DekuContainerRead;
use rs1090::decode::bds::infer_bds;
use rs1090::decode::cpr::Position;
use rs1090::prelude::*;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Parser)]
#[command(
    name = "decode1090",
    version,
    author = "xoolive",
    about = "Decode Mode S demodulated raw messages to JSON format"
)]
struct Options {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Output file instead of stdout
    #[arg(long, short, default_value=None)]
    output: Option<String>,

    /// Individual messages to decode
    msgs: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Decode ASTERIX CAT48 files (.raw, .da1) to JSON
    Cat48 {
        /// Input files or glob patterns (e.g., "*.da1", "data/*.raw")
        #[arg(required = true)]
        inputs: Vec<String>,

        /// Output file instead of stdout (JSONL format)
        #[arg(long, short)]
        output: Option<String>,

        /// Output one JSON array instead of JSONL (one record per line)
        #[arg(long)]
        array: bool,

        /// Filter: only include rollcall records (typ field)
        #[arg(long)]
        only_rollcall: bool,

        /// Filter: only include records with BDS payloads
        #[arg(long)]
        with_bds: bool,

        /// Filter: exclude records with all-zero BDS payload
        #[arg(long)]
        exclude_zero: bool,

        /// Filter: only include records with specific declared BDS codes
        /// Comma-separated hex codes, e.g. "44,45" or "40,50,60"
        #[arg(long)]
        filter_bds: Option<String>,
    },
    /// Decode BDS payload (7 bytes)
    Bds {
        /// BDS payload as hexadecimal string (56 bits = 7 bytes)
        #[arg(required = true)]
        payload: String,

        /// BDS code (optional, e.g., "40", "50", "60")
        /// If provided, decode strictly as this BDS code.
        /// If not provided, infer all possible BDS codes.
        #[arg(long, short)]
        bds: Option<String>,
    },
    /// Decode Mode S messages from files (JSONL, CSV)
    File {
        /// Input files (JSONL or CSV Beast format)
        #[arg(required = true)]
        inputs: Vec<String>,

        /// Reference coordinates for the decoding
        ///  (e.g. --reference LFPG for major airports,
        ///   --reference 43.3,1.35 or --reference ' -34,18.6' if negative)
        #[arg(long, short)]
        reference: Option<Position>,

        /// Specify input format explicitly (jsonl, csv)
        /// If not provided, format is auto-detected
        #[arg(long)]
        format: Option<String>,

        /// Output file instead of stdout
        #[arg(long, short)]
        output: Option<String>,

        /// Deduplication threshold (in ms)
        #[arg(long, default_value = "400")]
        deduplication: u128,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse();

    // Handle subcommands
    match options.command {
        Some(Commands::Cat48 {
            inputs,
            output,
            array,
            only_rollcall,
            with_bds,
            exclude_zero,
            filter_bds,
        }) => {
            return cat48::process_cat48(
                inputs,
                output,
                array,
                only_rollcall,
                with_bds,
                exclude_zero,
                filter_bds,
            )
            .await;
        }
        Some(Commands::Bds { payload, bds }) => {
            return bds::process_bds_decode(&payload, bds);
        }
        Some(Commands::File {
            inputs,
            reference,
            format,
            output,
            deduplication,
        }) => {
            return file::process_file_decode(
                inputs,
                reference,
                format,
                output,
                deduplication,
            )
            .await;
        }
        None => {}
    }

    // Default behavior: decode raw Mode S / BDS messages
    let mut output_file = if let Some(output_path) = options.output {
        Some(
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(output_path)
                .await?,
        )
    } else {
        None
    };

    if !options.msgs.is_empty() {
        for msg in options.msgs {
            let bytes = hex::decode(&msg).unwrap();
            let json = match bytes.len() {
                // 7 bytes: try Mode S short first, fallback to BDS inference
                7 => {
                    if let Ok(message) = Message::try_from(bytes.as_slice()) {
                        serde_json::to_string(&message).unwrap()
                    } else {
                        // BDS inference fallback
                        let inferred = infer_bds(&bytes, None);
                        let mut decoded_map = serde_json::json!({});
                        for decoded in inferred.iter() {
                            let bds_code = bds_code_from_decoded(decoded);
                            decoded_map[format!("{:02x}", bds_code)] =
                                serde_json::to_value(decoded)
                                    .unwrap_or(serde_json::json!(null));
                        }
                        serde_json::to_string(&serde_json::json!({
                            "payload": hex::encode(&bytes),
                            "mode": "infer",
                            "decoded": decoded_map
                        }))
                        .unwrap()
                    }
                }
                // 14 bytes: Mode S long message
                14 => {
                    let message = Message::try_from(bytes.as_slice()).unwrap();
                    serde_json::to_string(&message).unwrap()
                }
                // 16 bytes: Beast Mode S short (1a32 + 6 ts + 1 signal + 7 payload)
                // 23 bytes: Beast Mode S long (1a33 + 6 ts + 1 signal + 14 payload)
                16 | 23
                    if bytes[0] == 0x1a
                        && (bytes[1] == 0x32 || bytes[1] == 0x33) =>
                {
                    if let Some((timestamp, frame)) =
                        extract_beast_frame(&bytes)
                    {
                        let message = Message::from_bytes((&frame, 0))
                            .ok()
                            .map(|(_, m)| m);
                        let tmsg = TimedMessage {
                            timestamp,
                            frame,
                            message,
                            metadata: vec![],
                            decode_time: None,
                        };
                        serde_json::to_string(&tmsg).unwrap()
                    } else {
                        continue;
                    }
                }
                _ => {
                    // Try generic Mode S decode for other lengths
                    if let Ok(message) = Message::try_from(bytes.as_slice()) {
                        serde_json::to_string(&message).unwrap()
                    } else {
                        continue;
                    }
                }
            };
            if let Some(file) = &mut output_file {
                file.write_all(json.as_bytes()).await?;
                file.write_all("\n".as_bytes()).await?;
            } else {
                println!("{json}");
            }
        }
    }

    Ok(())
}
