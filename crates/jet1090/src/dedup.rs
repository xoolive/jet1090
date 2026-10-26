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

        let mut pending = Some(msg);
        // Expire existing groups before inserting this reception: identical
        // payloads outside the window belong to separate transmissions.
        loop {
            let expired = expiration_heap.peek().is_some_and(
                |Reverse((expiration, _))| *expiration <= timestamp_ms,
            );
            if !expired {
                let Some(msg) = pending.take() else {
                    break;
                };
                cache.entry(frame.clone()).or_default().push(msg);
                if cache[&frame].len() == 1 {
                    expiration_heap.push(Reverse((
                        timestamp_ms + u128::from(dedup_threshold),
                        frame.clone(),
                    )));
                }
                // Recheck after insertion so a zero threshold still emits
                // immediately, including when no further packet arrives.
                continue;
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
                if timestamp_ms
                    .checked_sub(msg_timestamp_ms)
                    .is_some_and(|age| age > u128::from(reorder_window))
                {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn reception(timestamp: f64, serial: u64) -> TimedMessage {
        TimedMessage {
            timestamp,
            frame: hex::decode("8d40621d58c382d690c8ac2863a7").unwrap(),
            message: None,
            metadata: vec![SensorMetadata {
                system_timestamp: timestamp + 0.01,
                gnss_timestamp: Some(timestamp),
                nanoseconds: None,
                rssi: None,
                serial,
                name: None,
            }],
            decode_time: None,
        }
    }

    #[tokio::test]
    async fn identical_payloads_respect_dedup_expiry_boundary() {
        for (gap_ms, expected_groups) in [(250, 1), (450, 2), (1000, 2)] {
            let (tx_in, rx_in) = mpsc::channel(3);
            let (tx_out, mut rx_out) = mpsc::channel(3);
            let task =
                tokio::spawn(deduplicate_messages(rx_in, tx_out, 450, 0));
            let second_timestamp = (1_000_000 + gap_ms) as f64 / 1000.;
            tx_in.send(reception(1000., 1)).await.unwrap();
            tx_in.send(reception(second_timestamp, 2)).await.unwrap();
            // Advance event time to expire the second group. Pending cache
            // draining at EOF is a separate issue, not part of this test.
            let mut sentinel = reception(1002., 3);
            sentinel.frame = vec![0; 14];
            tx_in.send(sentinel).await.unwrap();
            drop(tx_in);
            task.await.unwrap();
            let mut groups = Vec::new();
            while let Some(msg) = rx_out.recv().await {
                groups.push(msg);
            }
            assert_eq!(groups.len(), expected_groups, "gap={gap_ms}ms");
            assert_eq!(groups[0].timestamp, 1000.);
            if expected_groups == 1 {
                assert_eq!(groups[0].metadata.len(), 2);
            } else {
                assert_eq!(groups[1].timestamp, second_timestamp);
                assert_eq!(groups[0].metadata[0].serial, 1);
                assert_eq!(groups[1].metadata[0].serial, 2);
                assert_eq!(groups[0].metadata.len(), 1);
                assert_eq!(groups[1].metadata.len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn zero_dedup_threshold_emits_without_another_packet() {
        let (tx_in, rx_in) = mpsc::channel(1);
        let (tx_out, mut rx_out) = mpsc::channel(1);
        let task = tokio::spawn(deduplicate_messages(rx_in, tx_out, 0, 0));
        tx_in.send(reception(1000., 1)).await.unwrap();
        let message = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            rx_out.recv(),
        )
        .await
        .expect("zero threshold must not wait for another input")
        .unwrap();
        assert_eq!(message.timestamp, 1000.);
        drop(tx_in);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn late_packet_does_not_underflow_reordering_age() {
        let (tx_in, rx_in) = mpsc::channel(3);
        let (tx_out, mut rx_out) = mpsc::channel(3);
        let task = tokio::spawn(deduplicate_messages(rx_in, tx_out, 450, 2000));
        for (timestamp, frame) in [
            (1000.0, hex::decode("8d40621d58c382d690c8ac2863a7").unwrap()),
            (1000.5, vec![0; 14]),
            (999.0, vec![1; 14]),
        ] {
            tx_in
                .send(TimedMessage {
                    timestamp,
                    frame,
                    message: None,
                    metadata: Vec::new(),
                    decode_time: None,
                })
                .await
                .unwrap();
        }
        drop(tx_in);
        task.await.unwrap();
        // The buffered event time is not clamped to the later arrival's clock.
        assert_eq!(rx_out.recv().await.unwrap().timestamp, 1000.0);
    }
}
