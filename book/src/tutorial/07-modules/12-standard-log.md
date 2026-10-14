# Log structured events

`std.log` records events: a level, a message, and typed fields. Where the
events go is up to the program. There is no global logger; you build one
around a *sink* and pass it to the code that logs.

## A logger and a sink

A text sink writes each event as one line of `key=value` pairs to any
[`std.io`](../../design/stdlib/io.md) writer. This one writes to standard
output so the lines appear below; a command-line tool would use
`std.io.stderr()` instead:

```plenty
import std.io
import std.log
from std.log import Level, Clock

def main() -> Result[(), Failure]:
    sink = std.log.text_sink(std.io.stdout(), 1024)?
    mut log = std.log.logger(sink, Level.Info, Clock.Off)
    log.info("listening", [std.log.integer("port", 8080), std.log.text("host", "db.local")]?)?
    log.debug("not shown", []?)?
    log.warn("slow request", [std.log.floating("seconds", 1.5), std.log.boolean("retried", True)]?)?
    print(log.emitted(), log.filtered())?
    Ok(())
```
```output
level=info msg="listening" port=8080 host="db.local"
level=warn msg="slow request" seconds=1.5e0 retried=true
2 1
```

The threshold `Level.Info` drops the debug event before it reaches the sink.
Text values are quoted and escaped, so one event is always one line.
`Clock.Off` leaves out the time; `Clock.Wall` stamps each event with the
current wall-clock time.

The fields of a dropped event were still built. When a field is costly to
compute, ask first with `log.enabled(&Level.Debug)`.

## Passing a logger to a library

Code that logs takes the logger as a parameter. Making it generic over the
sink lets each caller pick the destination:

```plenty
import std.log
from std.log import Level, Clock, Logger

def connect[S](log: &mut Logger[S], attempts: i64) -> Result[(), Failure]:
    for attempt in range(attempts):
        log.warn("retrying", [std.log.integer("attempt", attempt + 1)]?)?
    log.info("connected", []?)?
    Ok(())

def main() -> Result[(), Failure]:
    mut log = std.log.logger(std.log.memory_sink(10)?, Level.Trace, Clock.Off)
    connect(&mut log, 2)?
    for event in &log.sink.events:
        print(std.log.level_name(&event.level), event.message, len(event.fields))?
    Ok(())
```
```output
warn retrying 1
warn retrying 1
info connected 0
```

A memory sink keeps events in a list, which suits tests. It holds at most the
capacity it was given; after that, logging returns the error `Full` and the
event is dropped.

## Threads

Threads do not share one logger. Each builds its own around a channel sink, and
one thread writes the queued events out. A channel sink never waits: when the
queue is full, the event is dropped with `Full`, so slow output cannot stall the
rest of the program. See the [log reference](../../design/stdlib/log.md#channels-and-threads)
for `channel_sink`, `share`, and `drain`.
