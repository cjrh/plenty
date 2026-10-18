use std::path::Path;

fn compile(dir: &Path, source: &str) -> std::path::PathBuf {
    let path = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{error}\n{source}"));
    executable
}

fn output(source: &str) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(compile(directory.path(), source))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result
}

fn run(source: &str) -> String {
    String::from_utf8(output(source).stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn levels_filter_and_fields_render_deterministically() {
    assert_eq!(
        run(r#"
import std.log
import std.io
import std.bytes
from std.log import Level, Clock
from std.time import Timestamp
def main() -> Result[(), Failure]:
    sink = std.log.text_sink(std.io.cursor(std.bytes.empty()?), 256)?
    mut log = std.log.logger(sink, Level.Info, Clock.Fixed(Timestamp(1700000000, 1234u32)))
    print(log.enabled(&Level.Debug), log.enabled(&Level.Info), log.enabled(&Level.Error))?
    log.info("listening", [std.log.integer("port", 8080), std.log.text("host", "a\"b\né"), std.log.boolean("tls", False), std.log.floating("load", 0.25), std.log.unsigned("n", u64.MAX)]?)?
    log.debug("hidden", []?)?
    log.log(Level.Trace, "hidden", []?)?
    log.warn("slow", [std.log.integer("ms", -3)]?)?
    log.error("failed", []?)?
    print(log.emitted(), log.filtered(), log.failed())?
    print(log.sink.writer.data.to_text()?)?
    print(std.log.level_name(&std.log.parse_level("trace")?), str.repr(std.log.parse_level("Info"))?)?
    Ok(())
"#),
        "False True True\n3 2 0\n\
time=1700000000.000001234 level=info msg=\"listening\" port=8080 host=\"a\\\"b\\né\" tls=false load=2.5e-1 n=18446744073709551615\n\
time=1700000000.000001234 level=warn msg=\"slow\" ms=-3\n\
time=1700000000.000001234 level=error msg=\"failed\"\n\n\
trace Result[std.log.Level, std.log.LogError].Err(std.log.LogError.UnknownLevel)"
    );
}

#[test]
fn caller_timestamps_win_and_wall_clock_stamps_unstamped_events() {
    assert_eq!(
        run(r#"
import std.log
from std.log import Level, Clock
from std.time import Timestamp
def main() -> Result[(), Failure]:
    mut log = std.log.logger(std.log.memory_sink(4)?, Level.Trace, Clock.Wall)
    mut given = std.log.event(Level.Info, "replayed", []?)
    given.time = Some(Timestamp(5, 6u32))
    log.emit(given)?
    log.info("now", []?)?
    mut quiet = std.log.logger(std.log.memory_sink(1)?, Level.Trace, Clock.Off)
    quiet.info("plain", []?)?
    for event in &log.sink.events:
        match &event.time:
            case Some(at):
                print(event.message, at.seconds > 1600000000, at.seconds == 5)?
            case Nothing:
                print("unstamped")?
    for event in &quiet.sink.events:
        match &event.time:
            case Some(_):
                print("stamped")?
            case Nothing:
                print(event.message, "unstamped")?
    Ok(())
"#),
        "replayed False True\nnow True False\nplain unstamped"
    );
}

#[test]
fn invalid_keys_oversized_lines_and_bad_specs_write_nothing() {
    assert_eq!(
        run(r#"
import std.log
import std.io
import std.bytes
from std.log import Level, Clock
def main() -> Result[(), Failure]:
    mut log = std.log.logger(std.log.text_sink(std.io.cursor(std.bytes.empty()?), 40)?, Level.Trace, Clock.Off)
    print(str.repr(log.info("this message is far too long", []?))?)?
    print(str.repr(log.info("k", [std.log.text("bad key", "x")]?))?)?
    print(str.repr(log.info("k", [std.log.text("msg", "x")]?))?)?
    print(str.repr(log.info("k", [std.log.text("", "x")]?))?)?
    log.info("ok", [std.log.boolean("a-b.c_D9", True)]?)?
    print(log.emitted(), log.failed())?
    print(log.sink.writer.data.to_text()?)?
    print(str.repr(std.log.text_sink(std.io.cursor(std.bytes.empty()?), 0))?)?
    print(str.repr(std.log.memory_sink(-1))?)?
    Ok(())
"#),
        "Result[(), std.log.LogError].Err(std.log.LogError.TooLarge)\n\
Result[(), std.log.LogError].Err(std.log.LogError.InvalidKey)\n\
Result[(), std.log.LogError].Err(std.log.LogError.InvalidKey)\n\
Result[(), std.log.LogError].Err(std.log.LogError.InvalidKey)\n\
1 4\nlevel=info msg=\"ok\" a-b.c_D9=true\n\n\
Result[std.log.TextSink[std.io.Cursor], std.log.LogError].Err(std.log.LogError.InvalidSpec)\n\
Result[std.log.MemorySink, std.log.LogError].Err(std.log.LogError.InvalidSpec)"
    );
}

// Issue #267: the nine-digit fraction rendered 5 s + 1.5e9 ns as 5.5 s.
#[test]
fn out_of_range_nanoseconds_write_nothing() {
    assert_eq!(
        run(r#"
import std.log
import std.io
import std.bytes
from std.log import Level, Clock, Event
from std.time import Timestamp
def main() -> Result[(), Failure]:
    mut log = std.log.logger(std.log.text_sink(std.io.cursor(std.bytes.empty()?), 80)?, Level.Info, Clock.Fixed(Timestamp(5, 1500000000u32)))
    print(str.repr(log.info("fixed", []?))?)?
    print(str.repr(log.emit(Event(Level.Info, "given", []?, Some(Timestamp(5, 1000000000u32)))))?)?
    log.emit(Event(Level.Info, "edge", []?, Some(Timestamp(5, 999999999u32))))?
    print(log.emitted(), log.failed())?
    print(log.sink.writer.data.to_text()?)?
    Ok(())
"#),
        "Result[(), std.log.LogError].Err(std.log.LogError.InvalidSpec)\n\
Result[(), std.log.LogError].Err(std.log.LogError.InvalidSpec)\n\
1 2\ntime=5.999999999 level=info msg=\"edge\"\n"
    );
}

#[test]
fn sink_failures_are_counted_and_short_writes_retried() {
    assert_eq!(
        run(r#"
import std.log
from std.bytes import ByteSource, Bytes
from std.io import StreamError
from std.log import Level, Clock
pub class Flaky:
    calls: i64
    accepted: i64
    pub def write[S: ByteSource](self: &mut Flaky, source: &S, start: i64, stop: i64) -> Result[i64, StreamError]:
        self.calls += 1
        if self.calls == 1:
            return Err(StreamError.Interrupted)
        if self.calls >= 6:
            return Err(StreamError.OutOfRange)
        self.accepted += 1
        Ok(1)
    pub def flush(self: &mut Flaky) -> Result[(), StreamError]:
        Ok(())
def main() -> Result[(), Failure]:
    mut log = std.log.logger(std.log.text_sink(Flaky(0, 0), 64)?, Level.Info, Clock.Off)
    print(str.repr(log.info("m", []?))?)?
    print(log.emitted(), log.failed(), log.sink.writer.accepted)?
    print(str.repr(log.info("m", []?))?)?
    print(log.emitted(), log.failed())?
    Ok(())
"#),
        "Result[(), std.log.LogError].Err(std.log.LogError.Output(std.io.StreamError.OutOfRange))\n\
0 1 4\n\
Result[(), std.log.LogError].Err(std.log.LogError.Output(std.io.StreamError.OutOfRange))\n\
0 2"
    );
}

#[test]
fn bounded_memory_discard_and_channel_sinks_drop_instead_of_growing_or_waiting() {
    assert_eq!(
        run(r#"
import std.log
import std.io
import std.bytes
from std.log import Level, Clock, Event
from std.channel import channel
def main() -> Result[(), Failure]:
    mut mem = std.log.logger(std.log.memory_sink(2)?, Level.Trace, Clock.Off)
    for i in range(4):
        drop(mem.info("m", [std.log.integer("i", i)]?))
    print(len(mem.sink.events), mem.emitted(), mem.failed())?
    mut quiet = std.log.logger(std.log.discard(), Level.Trace, Clock.Off)
    quiet.error("gone", []?)?
    print(quiet.emitted())?
    (sender, receiver) = channel[Event](2u64)?
    mut fast = std.log.logger(std.log.channel_sink(sender), Level.Info, Clock.Off)
    for i in range(4):
        print(str.repr(fast.info("q", [std.log.integer("i", i)]?))?)?
    print(fast.emitted(), fast.failed())?
    drop(fast)
    mut lines = std.log.text_sink(std.io.cursor(std.bytes.empty()?), 64)?
    report = std.log.drain(&receiver, &mut lines)
    print(report.delivered, report.failed)?
    print(lines.writer.data.to_text()?)?
    (closed_sender, closed_receiver) = channel[Event](1u64)?
    drop(closed_receiver)
    mut orphan = std.log.logger(std.log.channel_sink(closed_sender), Level.Info, Clock.Off)
    print(str.repr(orphan.info("lost", []?))?)?
    Ok(())
"#),
        "2 2 2\n1\n\
Result[(), std.log.LogError].Ok(())\n\
Result[(), std.log.LogError].Ok(())\n\
Result[(), std.log.LogError].Err(std.log.LogError.Full)\n\
Result[(), std.log.LogError].Err(std.log.LogError.Full)\n\
2 2\n2 0\n\
level=info msg=\"q\" i=0\nlevel=info msg=\"q\" i=1\n\n\
Result[(), std.log.LogError].Err(std.log.LogError.Closed)"
    );
}

#[test]
fn threads_share_a_channel_sink_with_one_writer_thread() {
    let text = run(r#"
import std.log
import std.io
import std.bytes
from std.log import Level, Clock, Event, ChannelSink, Drained, TextSink
from std.io import Cursor
from std.channel import channel
from std.thread import spawn
def produce(sink: ChannelSink, id: i64) -> Result[(u64, u64), Failure]:
    mut log = std.log.logger(sink, Level.Info, Clock.Off)
    for i in range(200):
        drop(log.info("tick", [std.log.integer("worker", id), std.log.integer("i", i)]?))
    Ok((log.emitted(), log.failed()))
def consume(receiver: Receiver[Event]) -> Result[(Drained, TextSink[Cursor]), Failure]:
    mut sink = std.log.text_sink(std.io.cursor(std.bytes.empty()?), 64)?
    report = std.log.drain(&receiver, &mut sink)
    Ok((report, sink))
def main() -> Result[(), Failure]:
    (sender, receiver) = channel[Event](8u64)?
    first = std.log.channel_sink(sender)
    second = first.share()
    with spawn(consume, receiver).unwrap() as reader:
        mut sent = 0u64
        mut dropped = 0u64
        with spawn(produce, first, 1).unwrap() as a, spawn(produce, second, 2).unwrap() as b:
            (a_sent, a_dropped) = a.join().unwrap()
            (b_sent, b_dropped) = b.join().unwrap()
            sent = a_sent + b_sent
            dropped = a_dropped + b_dropped
        (report, sink) = reader.join().unwrap()
        text = sink.writer.data.to_text().unwrap()
        mut lines = 0
        for scalar in text:
            if scalar == "\n":
                lines += 1
        print(sent + dropped, sent == report.delivered, report.failed, i64(sent) == lines).unwrap()
    Ok(())
"#);
    assert_eq!(text, "400 True 0 True");
}

#[test]
fn stderr_sink_serves_command_line_programs() {
    let result = output(
        r#"
import std.log
import std.io
from std.log import Level, Clock
def main() -> Result[(), Failure]:
    threshold = std.log.parse_level("debug")?
    mut log = std.log.logger(std.log.text_sink(std.io.stderr(), 512)?, threshold, Clock.Off)
    log.debug("parsed", [std.log.integer("files", 2)]?)?
    log.log(Level.Trace, "skipped", []?)?
    print("result")?
    Ok(())
"#,
    );
    assert_eq!(String::from_utf8(result.stdout).unwrap(), "result\n");
    assert_eq!(
        String::from_utf8(result.stderr).unwrap(),
        "level=debug msg=\"parsed\" files=2\n"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn emitting_to_prepared_sinks_allocates_nothing_and_allocation_failures_are_values() {
    assert_eq!(
        run(r#"
import std.log
import std.io
import std.bytes
from std.log import Level, Clock, Event
from std.time import Timestamp
def main() -> Result[(), Failure]:
    first = std.log.event(Level.Info, "ready", [std.log.integer("port", 80), std.log.text("host", "example"), std.log.floating("load", 0.5)]?)
    second = std.log.event(Level.Debug, "filtered", []?)
    third = std.log.event(Level.Warn, "kept", []?)
    sink = std.log.text_sink(std.io.cursor(std.bytes.with_capacity(256)?), 128)?
    mut text = std.log.logger(sink, Level.Info, Clock.Fixed(Timestamp(1, 0u32)))
    mut memory = std.log.logger(std.log.memory_sink(1)?, Level.Info, Clock.Wall)
    print("__test_begin_no_allocations__")?
    text.emit(first)?
    text.emit(second)?
    memory.emit(third)?
    print("__test_end_no_allocations__")?
    print(text.sink.writer.data.to_text()?)?
    print("__test_fail_allocations_after_0__")?
    failed_text = std.log.text_sink(std.io.stderr(), 64)
    failed_memory = std.log.memory_sink(4)
    print("__test_restore_allocations__")?
    print(str.repr(failed_text)?)?
    print(str.repr(failed_memory)?)?
    Ok(())
"#),
        "time=1.000000000 level=info msg=\"ready\" port=80 host=\"example\" load=5e-1\n\n\
Result[std.log.TextSink[std.io.Stderr], std.log.LogError].Err(std.log.LogError.Allocation(AllocError.OutOfMemory))\n\
Result[std.log.MemorySink, std.log.LogError].Err(std.log.LogError.Allocation(AllocError.OutOfMemory))"
    );
}

#[test]
fn ownership_and_sink_contracts_remain_checked() {
    for (source, expected) in [
        (
            "import std.log\nfrom std.log import Level, Clock\ndef main() -> Result[(), Failure]:\n    mut log = std.log.logger(5, Level.Info, Clock.Off)\n    Ok(())\n",
            "does not satisfy std.log.Sink",
        ),
        (
            "import std.log\nfrom std.log import Level, Clock, Field\ndef main() -> Result[(), Failure]:\n    fields = list[Field].new()?\n    event = std.log.event(Level.Info, \"m\", fields)\n    drop(fields)\n    Ok(())\n",
            "moved",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.plenty");
        std::fs::write(&path, source).unwrap();
        let error = plenty::check_file(&path, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}
