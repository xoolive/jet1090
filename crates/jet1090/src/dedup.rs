use rs1090::prelude::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::time::SystemTime;
use tokio::sync::mpsc;
use tracing::info;

/**
 * A basic message deduplication algorithm.
 *
 * Reads messages from a MPSC and sends deduplicated messages to another one.
 *
 * Identical messages are grouped for a duration of `dedup_threshold`.
 *
 * After deduplication, messages are held in an emission reordering buffer for
 * `reorder_window` milliseconds to ensure chronological emission order. This
 * handles cases where UDP sources batch timestamps, causing messages to expire
 * from the dedup cache in non-chronological order.
 *
 * Future versions should check for average gap between sensors for a better
 * synchronisation.
 */
pub async fn deduplicate_messages(
    mut rx: mpsc::Receiver<TimedMessage>,
    tx: mpsc::Sender<TimedMessage>,
    dedup_threshold: u32,
    reorder_window: u32,
) {
    let mut cache: HashMap<Vec<u8>, Vec<TimedMessage>> = HashMap::new();
    let mut expiration_heap: BinaryHeap<Reverse<(u128, Vec<u8>)>> =
        BinaryHeap::new();

    // Emission reordering buffer to handle UDP timestamp batching
    // Messages are held here for reorder_window ms and sorted before emission
    let mut emission_buffer: Vec<TimedMessage> = Vec::new();
    let reorder_window_enabled = reorder_window > 0;

    while let Some(msg) = rx.recv().await {
        let timestamp_ms = (msg.timestamp * 1e3) as u128;
        let frame = msg.frame.clone();

        // Add message to cache
        cache.entry(frame.clone()).or_default().push(msg);

        // Push the expiration timestamp into the heap
        if cache[&frame].len() == 1 {
            expiration_heap.push(Reverse((
                timestamp_ms + dedup_threshold as u128,
                frame.clone(),
            )));
        }

        // Check and handle expired entries
        // Use peek() to avoid pop-push cycle that breaks with backwards timestamps
        while let Some(Reverse((next_expiration, _))) = expiration_heap.peek() {
            let next_expiration = *next_expiration;

            // If not expired yet, stop processing
            if next_expiration > timestamp_ms {
                break;
            }

            // Pop the expired entry and process it
            let Reverse((_, frame)) = expiration_heap.pop().unwrap();

            // Otherwise clear the cache and process the deduplicated message
            if let Some(mut entries) = cache.remove(&frame) {
                // Sort by timestamp to use earliest, not first-arrived
                // This prevents backwards timestamps when sources have different latencies
                entries.sort_by(|a, b| {
                    a.timestamp.partial_cmp(&b.timestamp).unwrap()
                });

                let merged_metadata: Vec<SensorMetadata> = entries
                    .iter()
                    .flat_map(|entry| entry.metadata.clone())
                    .collect();

                let mut tmsg = entries.remove(0);
                tmsg.metadata = merged_metadata;

                let start = SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("SystemTime before unix epoch")
                    .as_secs_f64();

                if let Ok((_, msg)) = Message::from_bytes((&tmsg.frame, 0)) {
                    tmsg.decode_time = Some(
                        SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .expect("SystemTime before unix epoch")
                            .as_secs_f64()
                            - start,
                    );
                    tmsg.message = Some(msg);

                    // Add to emission buffer instead of sending directly
                    if reorder_window_enabled {
                        emission_buffer.push(tmsg);
                    } else {
                        // If reordering disabled, send immediately (lower latency)
                        if let Err(e) = tx.send(tmsg).await {
                            info!("Failed to send deduplicated entries: {}", e);
                        }
                    }
                }
            }
        }

        // Flush emission buffer: sort and emit messages older than reorder_window
        if reorder_window_enabled && !emission_buffer.is_empty() {
            // Sort by timestamp
            emission_buffer
                .sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap());

            // Emit messages that are older than reorder_window
            // We remove from index 0 repeatedly since older messages are at the front after sorting
            while !emission_buffer.is_empty() {
                let msg_timestamp_ms =
                    (emission_buffer[0].timestamp * 1e3) as u128;
                if timestamp_ms - msg_timestamp_ms > reorder_window as u128 {
                    let tmsg = emission_buffer.remove(0);
                    if let Err(e) = tx.send(tmsg).await {
                        info!("Failed to send reordered message: {}", e);
                    }
                } else {
                    // Messages are sorted, so we can stop once we find one that's not old enough
                    break;
                }
            }
        }
    }

    // Flush any remaining messages in the emission buffer when the channel closes
    if reorder_window_enabled && !emission_buffer.is_empty() {
        emission_buffer
            .sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap());

        for tmsg in emission_buffer {
            if let Err(e) = tx.send(tmsg).await {
                info!("Failed to send final buffered message: {}", e);
            }
        }
    }
}
