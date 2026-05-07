/// CAT48 subcommand — decode ASTERIX CAT48 files to JSON
use crate::error::{ErrorCode, ErrorResponse};
use deku::DekuContainerRead;
use glob::glob;
use rayon::prelude::*;
use rs1090::decode::cat48::Cat48Record;
use rs1090::decode::cpr::Position;
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Helper function to filter out zero-payload BDS records from a CAT48 record.
/// This modifies the record in-place to remove BDS records with all-zero payloads.
fn filter_zero_bds_records(mut record: Cat48Record) -> Cat48Record {
    if let Some(mut mb_data) = record.mode_s_mb_data.take() {
        // Filter out BDS records that have all-zero payloads
        mb_data
            .records
            .retain(|bds_rec| !bds_rec.payload.iter().all(|&b| b == 0));

        // Update count and put back if we have remaining records
        if !mb_data.records.is_empty() {
            mb_data.count = mb_data.records.len() as u8;
            record.mode_s_mb_data = Some(mb_data);
        }
    }
    record
}

/// Parse ASTERIX data from raw bytes, returning parsed records and error count.
fn parse_asterix_data(data: &[u8]) -> (Vec<Cat48Record>, usize) {
    let mut records = Vec::new();
    let mut errors = 0usize;
    let mut offset = 0;

    while offset < data.len() {
        // Need at least 3 bytes for CAT + LEN
        if offset + 3 > data.len() {
            break;
        }

        let cat = data[offset];
        let len =
            u16::from_be_bytes([data[offset + 1], data[offset + 2]]) as usize;

        // Validate length
        if len < 3 || offset + len > data.len() {
            eprintln!(
                "Invalid record length {} at offset {}, skipping",
                len, offset
            );
            errors += 1;
            offset += 1; // Try to recover by advancing 1 byte
            continue;
        }

        // Only process CAT48 records
        if cat != 48 {
            // Skip non-CAT48 records silently
            offset += len;
            continue;
        }

        // Parse the record
        let record_data = &data[offset..offset + len];
        match Cat48Record::from_bytes((record_data, 0)) {
            Ok((_, record)) => {
                records.push(record);
            }
            Err(e) => {
                eprintln!(
                    "Failed to parse CAT48 record at offset {}: {:?}",
                    offset, e
                );
                errors += 1;
            }
        }

        offset += len;
    }

    (records, errors)
}

/// Process ASTERIX CAT48 files and output JSON with optional filtering.
#[allow(clippy::too_many_arguments)]
pub async fn process_cat48(
    inputs: Vec<String>,
    output: Option<String>,
    array: bool,
    only_rollcall: bool,
    with_bds: bool,
    exclude_zero: bool,
    filter_bds: Option<String>,
    radar: Option<Position>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Parse BDS filter codes
    let bds_filter: Option<Vec<u8>> = filter_bds.map(|s| {
        s.split(',')
            .filter_map(|code| u8::from_str_radix(code.trim(), 16).ok())
            .collect()
    });
    // Expand glob patterns and collect all file paths
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for pattern in &inputs {
        let matches: Vec<_> = glob(pattern)?.filter_map(Result::ok).collect();
        if matches.is_empty() {
            // If no glob match, treat as literal path
            let path = std::path::PathBuf::from(pattern);
            if path.exists() {
                files.push(path);
            } else {
                let err = ErrorResponse::new(
                    ErrorCode::FileNotFound,
                    format!("File not found: {}", pattern),
                );
                eprintln!("{}", err.to_json()?);
                std::process::exit(1);
            }
        } else {
            files.extend(matches);
        }
    }

    if files.is_empty() {
        let err = ErrorResponse::new(
            ErrorCode::InvalidFilePath,
            "No input files found matching patterns",
        );
        eprintln!("{}", err.to_json()?);
        std::process::exit(1);
    }

    // Open output file if specified
    let mut output_file = if let Some(output_path) = output {
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

    // Process files in parallel using rayon
    let bds_filter_ref = &bds_filter;
    let processed_data: Vec<_> = files
        .par_iter()
        .map(|file_path| {
            eprintln!("Processing: {}", file_path.display());
            let data = std::fs::read(file_path).unwrap_or_default();
            let (records, errors) = parse_asterix_data(&data);
            let record_count = records.len();

            let mut filtered = 0usize;

            // Apply filters
            let filtered_records: Vec<Cat48Record> = records
                .into_iter()
                // Apply exclude_zero early so BDS filter sees only non-zero records
                .map(|record| {
                    if exclude_zero {
                        filter_zero_bds_records(record)
                    } else {
                        record
                    }
                })
                .filter(|record| {
                    // Filter: with_bds
                    if with_bds && !record.has_bds_data() {
                        filtered += 1;
                        return false;
                    }

                    // Filter: only_rollcall
                    if only_rollcall {
                        match record.target_type() {
                            Some(
                                rs1090::decode::cat48::DetectionType::ModeSRollCall,
                            )
                            | Some(
                                rs1090::decode::cat48::DetectionType::ModeSRollCallPsr,
                            ) => {
                                // Keep this record
                            }
                            _ => {
                                filtered += 1;
                                return false;
                            }
                        }
                    }

                    // Filter: BDS code filter
                    if let Some(ref codes) = bds_filter_ref {
                        let has_match = record
                            .mode_s_mb_data
                            .as_ref()
                            .map(|mb| {
                                mb.records
                                    .iter()
                                    .any(|r| codes.contains(&r.bds_code))
                            })
                            .unwrap_or(false);
                        if !has_match {
                            filtered += 1;
                            return false;
                        }
                    }

                    true
                })
                .collect();

            (filtered_records, record_count, errors, filtered)
        })
        .collect();

    // Aggregate statistics and collect all records
    let mut all_records: Vec<Cat48Record> = Vec::new();
    let mut total_recs = 0usize;
    let mut total_filt = 0usize;
    let mut total_errs = 0usize;

    for (records, record_count, errors, filtered) in processed_data {
        all_records.extend(records);
        total_recs += record_count;
        total_errs += errors;
        total_filt += filtered;
    }

    // Sort by time_of_day if we have records
    if !all_records.is_empty() {
        all_records.sort_by(|a, b| {
            let a_time = a.time_of_day.unwrap_or(0.0);
            let b_time = b.time_of_day.unwrap_or(0.0);
            a_time
                .partial_cmp(&b_time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    rs1090::decode::cat48::refine_inferred_bds(&mut all_records, radar);

    if array {
        // Output as single JSON array
        let json = serde_json::to_string(&all_records)?;
        if let Some(file) = &mut output_file {
            file.write_all(json.as_bytes()).await?;
            file.write_all(b"\n").await?;
        } else {
            println!("{json}");
        }
    } else {
        // Output each record as JSONL
        for record in all_records {
            let json = serde_json::to_string(&record)?;
            if let Some(file) = &mut output_file {
                file.write_all(json.as_bytes()).await?;
                file.write_all(b"\n").await?;
            } else {
                println!("{json}");
            }
        }
    }

    eprintln!(
        "Done: {} records parsed, {} filtered from {} files ({} parse errors)",
        total_recs - total_filt,
        total_filt,
        files.len(),
        total_errs
    );

    Ok(())
}
