# std.log

`import std.log` provides leveled events with typed fields and a static sink
contract. There is no global logger: a program builds a `Logger` around a sink
it chooses and passes the logger, or the sink, to the code that logs. A library
that logs takes `&mut Logger[S]` as a generic parameter, so its caller decides
the destination, threshold, and clock. The module is checked Plenty over
[`std.io`](io.md), [`std.fmt`](fmt.md), and [`std.time`](time.md), with no
native code of its own. It is available on the supported x86-64 Linux GNU
target.

## Events

| Type | Contents |
|---|---|
| `Level` | `Trace`, `Debug`, `Info`, `Warn`, `Error`, in increasing severity |
| `Value` | `Text(str)`, `Integer(i64)`, `Unsigned(u64)`, `Floating(f64)`, `Boolean(bool)` |
| `Field` | `key: str`, `value: Value` |
| `Event` | `level: Level`, `message: str`, `fields: list[Field]`, `time: Option[Timestamp]` |

All fields are public. `text(key, value)`, `integer`, `unsigned`, `floating`,
and `boolean` build a `Field`; `event(level, message, fields)` builds an
unstamped `Event`. An event is wholly owned, so a sink may keep it or move it to
another thread. Fields keep the order the caller gave.

`level_name(&level)` returns the lowercase name, such as `"warn"`.
`parse_level(&name)` accepts exactly those five names and returns
`UnknownLevel` for anything else, including other capitalizations.

## Loggers

`logger(sink, threshold: Level, clock: Clock) -> Logger[S]` takes ownership of
`sink`. The sink is public as the field `sink`.

| `Logger[S]` | Result | Contract |
|---|---|---|
| `enabled(&level)` | `bool` | Whether an event at `level` would reach the sink |
| `emit(event)` | `Result[(), LogError]` | Filter, stamp, and deliver one event |
| `log(level, message, fields)` | `Result[(), LogError]` | Build and emit an event |
| `debug`, `info`, `warn`, `error` `(message, fields)` | `Result[(), LogError]` | `log` at that level |
| `emitted()`, `filtered()`, `failed()` | `u64` | Events delivered, below the threshold, and rejected by the sink |

An event below the threshold is dropped and counted as filtered; that is
`Ok(())`, not an error. The fields of a filtered event have already been
built, so guard expensive fields with `enabled` first.

| `Clock` | Stamp for an event whose `time` is `Nothing` |
|---|---|
| `Off` | None; `time` stays `Nothing` |
| `Wall` | `std.time.wall_now()` at emission |
| `Fixed(Timestamp)` | That timestamp, for reproducible output and tests |

An event that already has a `time` keeps it, so replayed or forwarded events
are not restamped. Stamping happens after filtering.

A sink error is returned unchanged and counted as failed. The logger does not
retry, buffer, or fall back to another sink. The caller chooses whether a
logging failure matters: propagate it, or `drop` the result and inspect
`failed()` later.

## Sinks

A sink is any `pub` class with this method, resolved at compile time:

| Protocol | Method | Result |
|---|---|---|
| `Sink` | `emit(self: &mut, event: Event)` | `Result[(), LogError]` |

The sink owns the event from then on, even on failure.

| Constructor | Sink | Delivery |
|---|---|---|
| `text_sink(writer: W, max_line: i64)` | `Result[TextSink[W], LogError]` | One logfmt line per event to any [`Writer`](io.md#stream-contracts) |
| `memory_sink(capacity: i64)` | `Result[MemorySink, LogError]` | Keeps up to `capacity` events in the public list `events` |
| `channel_sink(sender: Sender[Event])` | `ChannelSink` | Queues the event without waiting |
| `discard()` | `Discard` | Accepts and drops every event |

### Text lines

`TextSink` writes logfmt: space-separated `key=value` pairs, then a newline.

```text
time=1700000000.000001234 level=info msg="listening" port=8080 host="db" tls=false load=2.5e-1
```

- `time` appears only for a stamped event: seconds since the Unix epoch, a
  dot, and exactly nine digits of nanoseconds. A `time` whose `nanoseconds`
  is 1,000,000,000 or more returns `InvalidSpec` and writes nothing.
- `level` is the lowercase name. `msg` and text values use the
  [`debug_text`](fmt.md#writers-and-strings) form: double quotes, with `"`,
  `\`, LF, CR, TAB, and other control characters escaped, so one event is
  always one line.
- Integers are decimal. Floats use the shortest round-trip scientific form
  of [`std.fmt`](fmt.md). Booleans are `true` and `false`.
- A key must be nonempty ASCII letters, digits, `_`, `.`, or `-`, and must not
  be `time`, `level`, or `msg`. Otherwise `emit` returns `InvalidKey` and
  writes nothing.

`max_line` bounds one line in bytes, newline included; below one is
`InvalidSpec`. An event whose line would be longer returns `TooLarge` and
writes nothing: lines are never truncated. Each line is built in a buffer
reserved at construction, then delivered with one
[`write_all`](io.md#helpers) followed by `flush`. A write error is
`Output(cause)`; part of the line may already have reached the writer.
`TextSink` exposes the writer as the public field `writer`.

For a command-line program, use `text_sink(std.io.stderr(), ...)`. Lines then
share the process buffer with `print`, keep their order relative to it, and
are flushed per event.

### Memory

`MemorySink` reserves room for `capacity` events at construction. Once full,
`emit` returns `Full` and the event is dropped; the list never grows. Tests
inspect `sink.events` directly.

### Channels and threads

`ChannelSink` sends with `send_nowait` on a bounded [channel](channel.md). A
full queue returns `Full` and drops the event, so a slow consumer never blocks
the logging thread. A queue with no receiver returns `Closed`. `share()`
returns another handle on the same queue for another thread's logger.

`drain(receiver: &Receiver[Event], sink: &mut S) -> Drained` delivers queued
events to `sink` until every sender is gone. It waits for events. A sink
error is counted in `Drained.failed` and draining continues;
`Drained.delivered` counts the rest. Run it on one thread to serialize output
from many producers:

```text
def consume(receiver: Receiver[Event]) -> Result[Drained, Failure]:
    mut lines = std.log.text_sink(std.io.stderr(), 4096)?
    Ok(std.log.drain(&receiver, &mut lines))
```

The usual [join rules](../facilities/03-concurrency/channels.md#joins-and-live-endpoints)
apply: a `ChannelSink` holds a sender, so drop every handle before joining
the draining thread.

## Errors

| `LogError` | Meaning |
|---|---|
| `InvalidSpec` | A negative capacity, a line limit below one, or an event `time` with out-of-range nanoseconds |
| `InvalidKey` | A field key `TextSink` cannot write unambiguously |
| `UnknownLevel` | `parse_level` received another name |
| `TooLarge` | The line would exceed `max_line` |
| `Full` | A bounded sink had no room; the event was dropped |
| `Closed` | The channel has no receiver |
| `Format(FormatError)` | `std.fmt` rejected a value |
| `Output(StreamError)` | The writer failed |
| `Allocation(AllocError)` | A sink's storage could not be reserved |

## Ownership and threads

Every emission takes `&mut`, so a logger and its sink have one user at a time.
Threads do not share one logger: each thread builds its own `Logger` around a
`ChannelSink` handle from `share()`, and one thread drains the queue. Events
move through the channel without copying. A `Logger` holding a `TextSink` over
a stream can also be moved into a worker like any owned value.

## Costs

| Allocates | Does not allocate |
|---|---|
| Building fields and the field list, as the caller writes them | `Logger.emit` and the shorthand methods |
| `text_sink`: a 32-byte scratch buffer and a `max_line` line buffer | `TextSink.emit`, apart from the writer's own storage |
| `memory_sink`: a list of `capacity` events | `MemorySink.emit` |
| `channel(...)`: the queue, made by the caller | `ChannelSink.emit`, `share`, `drain` |

`TextSink.emit` is linear in the line length; each byte passes through
`std.fmt` and is copied once into the line buffer. The line then costs one
`write_all` and one `flush`: over `stderr()` that is one buffered write, and
over a `FileStream` at least one system call. Dropping an event frees its
fields.

## Limits

- Only x86-64 Linux GNU is supported.
- There is no JSON output, no sampling or rate limiting, no per-module
  thresholds, and no span or context propagation. Compose these from `Event`
  and a custom sink.
- Sinks are static: a logger cannot switch sinks at run time, and there is no
  sink object that erases the concrete class. Fan-out to two destinations is a
  custom sink that writes to both.
- `LineBuffer` is public only because `std.fmt` writes into it; it is not
  meant for direct use.
