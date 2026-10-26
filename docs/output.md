# Serving real-time decoded data

## Output in the terminal

- The `--verbose` option dumps JSON entries to the terminal as messages are received.
- The `--interactive` option is similar to the `dump1090 --interactive` and provides a table view of the visible aircraft. For example the following snapshot can be obtained with a locally connected dongle with `jet1090 --interactive rtlsdr://`

![jet1090 table view](images/jet1090-table.png)

Both options are incompatible and `--interactive` has the priority.

Press `s` or `S` to show or hide the **Sources** panel below the aircraft table.
It uses the existing source names (stable identifiers for unnamed sources) and
shows their current state: `●` connected, `○` connecting, `↻` reconnecting or
`✕` failed. Reconnecting/failed sources also show their most recent retained
diagnostic; it is no longer displayed there after recovery. The panel takes
at most half the terminal height, keeping the aircraft table visible.

A SeRo entry represents a subscription containing multiple sensors. Without
an explicit `name`, it uses an opaque `SeRo-…` subscription label, not a sensor
serial number. It becomes connected once the authenticated stream and sensor
metadata are ready, even while idle; failures are attributed to that entry.
Individual sensor identifiers in decoded messages are unchanged.

The `e` key opens the retained warning/error history; `Esc` closes it. Showing
sources does not acknowledge unread errors. While entering a `/` search, `s`
and `S` remain search text rather than toggling the panel.

In the table view, aircraft can be sorted by:

| key |                                         |
| --- | --------------------------------------- |
| `.` | number of messages (`count`), _default_ |
| `A` | altitude                                |
| `C` | callsign                                |
| `F` | first timestamp                         |
| `L` | last timestamp                          |
| `V` | vertical rate                           |
| `-` | ascending/descending order              |

## Output as a JSON file

The `-o` or `--output` option enables the dumping of decoded messages to a file in the JSON line format.

If the filename ends in `.jsonl.zst`, output is automatically compressed with
Zstandard. All other filenames keep the existing uncompressed behavior.

```sh
jet1090 --output capture.jsonl.zst tcp://127.0.0.1:30005
decode1090 follow capture.jsonl.zst
```

The compressor and file buffers are flushed approximately every second, even
when no new messages arrive, so the file can be followed while it is still
being written. `decode1090 follow` reads from the **first byte**, displaying
existing records before following additions without re-decoding their frames.
It accepts plain JSONL too and stops with Ctrl-C. The file must already exist;
rotation/replacement and truncation are not supported.

A plain `tail -f` cannot start arbitrarily inside a compressed frame. The
`tail -c +1 -f file.jsonl.zst | zstd -dc` pipeline reads from the beginning but
can lag at low traffic rates because the `zstd` CLI buffers piped input.
`decode1090 follow` instead reads incrementally and flushes decoded output
immediately. Further downstream pipes may still add their own buffering.

`jet1090` retains its append behavior: restarting with the same output filename
adds another Zstandard frame. The `zstd` CLI and `decode1090 file` accept these
concatenated frames. Graceful shutdown (including SIGINT/SIGTERM in headless
mode) finalizes the current frame. Forced termination such as SIGKILL or power
loss may leave an unfinished final frame. `decode1090` also supports automatic
`.jsonl.zst` output, but overwrites its output file as before rather than appending.

That format is easily accessible from several libraries across programming languages:

=== "Pandas (Python)"

    ```python
    import pandas as pd

    df = pd.read_json("output.jsonl", lines=True)
    ```

=== "Polars (Python)"

    ```python
    import polars as pl

    df = pl.read_ndjson("output.jsonl")
    ```

=== "Polars (Rust)"

    You will need the `lazy` and `ndjson` (new line delimited JSON) features:

    ```toml
    [dependencies]
    polars = { version = "0.34", features = ["lazy", "ndjson"] }
    ```

    ```rust
    use polars::prelude::*;

    let df = LazyFrame::scan_ndjson("output.jsonl", ScanArgsNdJson::default())?
        .collect()?;
    ```

=== "JavaScript"

    ```javascript
    data = fetch("output.jsonl")
      .then(response => response.text())
      .then(text => text.split("\n").filter(line => line.trim() !== "").map(JSON.parse));
    ```

=== "R"

    ```R
    library(ndjson)

    df <- ndjson::stream_in("output.jsonl")
    ```

## REST API

If a `--serve-port` option is set, a REST API is set on `0.0.0.0` on the port of your choice.

The following endpoint are provided:

- `/`: returns an index page with the available routes
- `/icao24`: returns a list of all visible `icao24` identifiers
- `/all`: returns a list of all state vectors (the last valid field for each aircraft)
- `/track?icao24=xxx`: returns a list of all received messages for a given aircraft.
- `/sensors`: returns information about all sensors
- `/airports?q=xxx`: returns a list of airports matching the query string
- `/stream`: a live feed of decoded messages as [server-sent events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events) (see below)

!!! warning

    By default, all the history remains accessible until the program exits but this behaviour can drain your RAM memory quickly if you see many aircraft. You can use the `--history-expire` parameter to limit the history to a number of minutes before the current time.

    You can also completely deactivate the storing of messages with the `--history-expire 0` option.

## Live stream

The `/stream` endpoint pushes every decoded message as it arrives, in the same JSON format as the `.jsonl` output and the Redis feed. It uses server-sent events, so any browser can consume it without extra libraries:

```javascript
const source = new EventSource("http://localhost:8080/stream");
source.onmessage = (event) => console.log(JSON.parse(event.data));
```

The `--df-filter` and `--aircraft-filter` options apply before the stream, so a message excluded there never reaches any subscriber.

A client that reads too slowly is moved to the live edge of the feed and misses the messages in between. The decoder never waits for a client.

## WebSocket

(not yet supported)

## Redis pub/sub

If the `--redis-url` is set, all messages are sent to the Redis pub/sub instance.

The default topic is `jet1090` but it can be overridden with the `--redis-topic` option.

The main loop pipelines up to 16 messages or 64 KiB of combined JSON payloads,
with a 5 ms collection deadline from the first message. There is no separate
publication task or additional queue. A larger individual payload is sent whole
in its own batch; these limits are not a fixed total process-memory cap.
Flushing runs between messages, with due batches prioritized over new input;
current-message processing, scheduling and network time are additional to the
deadline.

JSON contents and capture timestamps are unchanged. Without transport errors,
publication preserves emission order. The main loop waits for acknowledgements
once per batch, and pauses during flushing or retries. Redis, `/stream`, file
output and history are not an atomic transaction.

`--redis-retry-interval` controls retries (default: 5 seconds). On failure the
publisher reconnects and retries the pending batch before advancing. A connection
failure can leave delivery uncertain: an already-published prefix may be replayed,
and a warning reports this possibility. Setting the interval to `0` skips a
failed batch with a warning instead. Redis Pub/Sub is not durable or exactly-once
subscriber delivery.

Normal shutdown flushes the pending batch. Flushing is bounded to
two seconds; a timeout reports unacknowledged messages, which may already
have reached Redis, and produces an unsuccessful exit. This covers accepted JSON
output, not every upstream radio frame still in a source or deduplication buffer.

You may run a Redis server as a Docker or podman instance:

=== "Docker"

    ```sh
    docker run -d --rm --name redis -p 6379:6379 -p 8001:8001 redis/redis-stack:latest
    ```

=== "podman"

    ```sh
    podman run -d --rm --name redis -p 6379:6379 -p 8001:8001 redis/redis-stack:latest
    ```

You may check messages are incoming through the web interface on `http://localhost:8001` or with a redis CLI client, for instance:

```sh
$ uvx iredis
Installed 11 packages in 51ms
iredis  1.15.0 (Python 3.13.1)
redis-server  7.4.1
Home:   https://iredis.xbin.io/
Issues: https://github.com/laixintao/iredis/issues
127.0.0.1:6379> psubscribe jet1090
# messages should be incoming here
```
