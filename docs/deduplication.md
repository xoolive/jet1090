# Deduplication

When several receivers feed `jet1090` at once, the same aircraft transmission often reaches the application more than once: a network of receivers, an aggregator, or a Beast-to-WebSocket relay may all deliver copies of the same frame. Before decoding, `jet1090` merges these receptions into a single message.

Deduplication is on by default. Pass `--no-deduplication` to decode and forward every reception as received.

## What counts as a duplicate

Two receptions are merged when they carry the exact same frame bytes and their capture timestamps fall within the deduplication threshold. Each reception must also come from a distinct receiver: the same receiver reporting an identical payload twice within the window counts as two transmissions, and so do receptions that could match more than one group in flight. Identical bytes alone are never taken as proof of a single physical transmission.

The merged message keeps every individual reception in its `metadata`: receiver serial and name, signal level, and capture time all remain available to downstream consumers.

## Options

```toml
deduplication = 450   # in milliseconds
reorder_window = 200  # in milliseconds
```

- `--deduplication` (default: 450 ms) is the maximum capture-time distance between two receptions of the same frame. Set it to 0 to pass messages through without merging.
- `--reorder-window` (default: 200 ms) is an extra lookahead used to emit messages in capture-time order. It extends the collection delay, then gives the output stage up to the same duration again for ordering.

With the defaults, a message received once on a quiet feed leaves the deduplication stage after at most 850 ms: 650 ms of collection (450 + 200), then up to 200 ms of ordering. On a busy feed the ordering stage usually releases messages earlier, close to the 650 ms collection deadline. Setting `--reorder-window 0` lowers the delay to the bare threshold, at the cost of occasional backwards timestamps on feeds that deliver in bursts. With `--deduplication 0`, messages are emitted immediately and the reorder window has no effect.

Ordering is best effort. A source delivering arbitrarily stale events can still produce out-of-order timestamps, since no capture time is ever clamped or rewritten. When the input ends, pending messages are flushed. Internal buffers are bounded, and if a burst of traffic exceeds them, `jet1090` releases the oldest messages early with a warning rather than dropping receptions.

## Timestamps

Each reception carries its original capture time, and the merged message keeps the earliest of them. One exception: some Beast receivers report GPS time on a coarse grid of about 134 ms, while other receivers of the same group (a SeRo feed, for instance) report the same transmission with a much finer GNSS timestamp. When `jet1090` has confirmed a receiver's clock is coarse — which takes about half a minute of observations after connecting — and the group holds a finer measured reception, that reception's timestamp is selected instead.

`jet1090` never rewrites capture times beyond this selection. No offset is added, no midpoint is computed, and an unreliable clock does not become a precise measurement just because it was observed.

## Timing diagnostics

To check how well the clocks of your receivers agree, enable the opt-in timing reports:

```sh
RUST_LOG='warn,rs1090::source::beast::timing=info,jet1090::dedup::timing=debug' \
  jet1090 --log-file timing.log
```

Every 30 seconds, the log lists, for each pair of receivers, how often identical position messages (DF 17, BDS 0,5) were merged into one group or kept apart, together with the mean difference between their capture times and arrival times. A large, systematic capture difference on a pair points to a receiver clock worth investigating. These reports are observations only: they change no timestamp, no matching rule, and no decoding.

!!! tip

    Log files are only written in non-interactive mode: the interactive table view ignores `--log-file`.
