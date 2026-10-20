use std::process::Command;

const FIXTURES: &str = r#"
import std.bytes
from std.bytes import Bytes
from streams import StepError, Partial, ReadFailure, WriteFailure, CopyFailure, read_exact, write_all, copy_all
from http_body import FixedBody, FramingError, fixed_body
pub enum Transport:
    Tls(i64, str)
    Cancelled
    Allocation(AllocError)
pub enum Storage:
    Full(i64)
def scratch() -> Result[Bytes, AllocError | std.bytes.BytesError]:
    mut buffer = std.bytes.with_capacity(4)?
    buffer.resize(4, 0u8)?
    Ok(buffer)
pub class Source:
    mode: i64
    calls: i64
    pub def read(self: &mut Source, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StepError[Transport]]:
        self.calls += 1
        if self.calls == 1:
            return Err(StepError[Transport].Interrupted)
        if self.calls == 2:
            mut count = stop - start
            if count > 2:
                count = 2
            mut at = start
            while at < start + count:
                buffer.set(at, 65u8).unwrap()
                at += 1
            return Ok(count)
        if self.mode == 1:
            return Err(StepError[Transport].Source(Transport.Tls(47, "handshake")))
        if self.mode == 2:
            return Err(StepError[Transport].Source(Transport.Cancelled))
        if self.mode == 3:
            return Ok(stop - start + 1)
        if self.mode == 4:
            return Ok(-1)
        if self.mode == 5:
            return Err(StepError[Transport].Source(Transport.Allocation(AllocError.OutOfMemory)))
        Ok(0)
pub class Sink:
    mode: i64
    calls: i64
    delivered: i64
    pub def write(self: &mut Sink, buffer: &Bytes, start: i64, stop: i64) -> Result[i64, StepError[Storage]]:
        self.calls += 1
        if self.calls == 1:
            return Err(StepError[Storage].Interrupted)
        if self.calls > 2 and self.mode == 1:
            return Err(StepError[Storage].Source(Storage.Full(28)))
        if self.mode == 2:
            return Ok(0)
        if self.mode == 3:
            return Ok(-1)
        if self.mode == 4:
            return Ok(stop - start + 1)
        self.delivered += 1
        Ok(1)
"#;

fn fixture(source: &str) -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    for module in ["streams", "http_body"] {
        std::fs::copy(
            format!(
                "{}/examples/error_composition/{module}.plenty",
                env!("CARGO_MANIFEST_DIR")
            ),
            workspace.path().join(format!("{module}.plenty")),
        )
        .unwrap();
    }
    let file = workspace.path().join("main.plenty");
    std::fs::write(&file, source).unwrap();
    workspace
}

fn execute(body: &str, expected: &str) {
    execute_modules(body, expected, false);
    execute_modules(body, expected, true);
}

fn execute_modules(body: &str, expected: &str, standard: bool) {
    let source = format!("{FIXTURES}\n{body}");
    let source = if standard {
        source
            .replace("from streams import", "from std.io.typed import")
            .replace("from http_body import", "from std.http.typed import")
    } else {
        source
    };
    let workspace = fixture(&source);
    let file = workspace.path().join("main.plenty");
    let binary = workspace.path().join("program");
    plenty::compile_file_to_executable(&file, &binary, None).unwrap();
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible: String = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(visible, expected);
}

#[test]
fn incompatible_protocol_error_is_diagnosed() {
    let source = format!("{FIXTURES}\ndef main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:\n    mut source = Source(0, 0)\n    mut buffer = scratch()?\n    drop(read_exact[Storage, Source](&mut source, &mut buffer, 0, 4))\n    Ok(())\n");
    let workspace = fixture(&source);
    let error = plenty::check_file(&workspace.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("signature"), "{error}");
}

#[test]
fn production_bridges_copy_fixed_and_close_delimited_bodies() {
    execute_modules(
        r#"
import std.io
import std.io.typed
from std.http.typed import close_body
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.io.StreamError | StepError[FramingError[std.io.StreamError]] | FramingError[std.io.StreamError] | CopyFailure[FramingError[std.io.StreamError], std.io.StreamError]]:
    mut data = scratch()?
    data.set(0, 65u8)?
    data.set(1, 66u8)?
    mut buffer = scratch()?
    mut fixed = fixed_body(std.io.typed.reader(std.io.cursor(data)), 2)?
    mut destination = std.io.typed.writer(std.io.cursor(std.bytes.with_capacity(4)?))
    print(copy_all(&mut fixed, &mut destination, &mut buffer)?)?
    mut cursor = std.io.typed.unwrap_writer(destination)
    print(cursor.position())?
    cursor.seek(std.io.SeekFrom.Start(0))?
    mut closed = close_body(std.io.typed.reader(cursor))
    mut output = std.io.typed.writer(std.io.cursor(std.bytes.with_capacity(4)?))
    print(copy_all(&mut closed, &mut output, &mut buffer)?)?
    print(closed.read(&mut buffer, 0, 4)?)?
    mut output_cursor = std.io.typed.unwrap_writer(output)
    output_cursor.seek(std.io.SeekFrom.Start(0))?
    print(output_cursor.read(&mut buffer, 0, 4)?)?
    print(buffer.get(0).unwrap())?
    print(buffer.get(1).unwrap())?
    Ok(())
"#,
        "2\n2\n2\n0\n2\n65\n66\n",
        true,
    );
}

#[test]
fn close_body_preserves_causes_progress_and_terminal_state() {
    execute_modules(
        r#"
from std.http.typed import close_body
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | StepError[FramingError[Transport]]]:
    mut buffer = scratch()?
    mut body = close_body(Source(1, 0))
    match body.read(&mut buffer, -1, 0):
        case Err(StepError.Source(FramingError.OutOfRange)):
            print("range")?
        case _:
            print("wrong")?
    print(body.read(&mut buffer, 0, 0)?)?
    mut sink = Sink(0, 0, 0)
    match copy_all(&mut body, &mut sink, &mut buffer):
        case Err(CopyFailure.Read(count, ReadFailure.Source(FramingError.Transport(Transport.Tls(code, message))))):
            print(count)?
            print(code)?
            print(message)?
        case _:
            print("wrong")?
    match body.read(&mut buffer, 0, 4):
        case Err(StepError.Source(FramingError.Broken)):
            print("broken")?
        case _:
            print("wrong")?
    mut invalid = close_body(Source(3, 2))
    match invalid.read(&mut buffer, 0, 4):
        case Err(StepError.Source(FramingError.InvalidProgress)):
            print("progress")?
        case _:
            print("wrong")?
    mut cancelled = close_body(Source(2, 2))
    match cancelled.read(&mut buffer, 0, 4):
        case Err(StepError.Source(FramingError.Transport(Transport.Cancelled))):
            print("cancelled")?
        case _:
            print("wrong")?
    Ok(())
"#,
        "range\n0\n2\n47\nhandshake\nbroken\nprogress\ncancelled\n",
        true,
    );
}

#[test]
fn close_body_empty_reads_do_not_consume_input_and_eof_is_sticky() {
    execute_modules(
        r#"
from std.http.typed import close_body
pub class Ends:
    calls: i64
    pub def read(self: &mut Ends, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StepError[Transport]]:
        self.calls += 1
        print(self.calls).unwrap()
        if self.calls == 1:
            return Ok(0)
        Err(StepError[Transport].Source(Transport.Cancelled))
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | StepError[FramingError[Transport]]]:
    mut body = close_body(Ends(0))
    mut buffer = scratch()?
    print(body.read(&mut buffer, 2, 2)?)?
    print(body.read(&mut buffer, 0, 4)?)?
    print(body.read(&mut buffer, 0, 4)?)?
    Ok(())
"#,
        "0\n1\n0\n0\n",
        true,
    );
}

#[test]
fn legacy_bridges_retry_only_interruption_and_preserve_io_errors() {
    execute_modules(
        r#"
import std.io
import std.io.typed
pub class Legacy:
    calls: i64
    pub def read(self: &mut Legacy, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, std.io.StreamError]:
        self.calls += 1
        if self.calls == 1:
            return Err(std.io.StreamError.Interrupted)
        if self.calls == 2:
            return Ok(1)
        Err(std.io.StreamError.Io(IoError.Other))
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut reader = std.io.typed.reader(Legacy(0))
    mut buffer = scratch()?
    match read_exact(&mut reader, &mut buffer, 0, 4):
        case Err(partial):
            print(partial.completed)?
            match partial.cause:
                case ReadFailure.Source(std.io.StreamError.Io(IoError.Other)):
                    print("io")?
                case _:
                    print("wrong")?
        case Ok(_):
            print("wrong")?
    original = std.io.typed.unwrap_reader(reader)
    print(original.calls)?
    Ok(())
"#,
        "1\nio\n3\n",
        true,
    );
}

#[test]
fn protocol_bounds_infer_errors_and_nested_framing_without_explicit_types() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | Partial[ReadFailure[Transport]] | FramingError[Transport] | CopyFailure[FramingError[Transport], Storage]]:
    mut buffer = scratch()?
    mut source = Source(0, 0)
    read_exact(&mut source, &mut buffer, 0, 2)?
    mut body = fixed_body(Source(0, 0), 2)?
    mut sink = Sink(0, 0, 0)
    print(copy_all(&mut body, &mut sink, &mut buffer)?)?
    Ok(())
"#,
        "2\n",
    );
}

#[test]
fn standalone_example_keeps_the_original_transport_code() {
    let workspace = tempfile::tempdir().unwrap();
    let binary = workspace.path().join("program");
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/error_composition/main.plenty");
    plenty::compile_file_to_executable(&file, &binary, None).unwrap();
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"1\n104\n");
}

#[test]
fn empty_and_invalid_ranges_do_not_touch_transports() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | Partial[WriteFailure[Storage]]]:
    mut source = Source(0, 0)
    mut sink = Sink(0, 0, 0)
    mut buffer = scratch()?
    write_all[Storage, Sink](&mut sink, &buffer, 2, 2)?
    match write_all[Storage, Sink](&mut sink, &buffer, 3, 5):
        case Err(partial):
            print(partial.completed == 0)?
            match partial.cause:
                case WriteFailure.OutOfRange:
                    print("range")?
                case _:
                    print("wrong")?
        case _:
            print("wrong")?
    buffer.clear()
    match copy_all[Transport, Storage, Source, Sink](&mut source, &mut sink, &mut buffer):
        case Err(CopyFailure.Read(0, ReadFailure.OutOfRange)):
            print("empty")?
        case _:
            print("wrong")?
    print(source.calls)?
    print(sink.calls)?
    match fixed_body[Transport, Source](Source(0, 0), -1):
        case Err(FramingError.OutOfRange):
            print("length")?
        case _:
            print("wrong")?
    Ok(())
"#,
        "True\nrange\nempty\n0\n0\nlength\n",
    );
}

#[test]
fn exact_read_retains_partial_progress_and_full_transport_cause() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut source = Source(1, 0)
    mut buffer = scratch()?
    match read_exact[Transport, Source](&mut source, &mut buffer, 0, 4):
        case Ok(_):
            print("unexpected")?
        case Err(partial):
            print(partial.completed)?
            match partial.cause:
                case ReadFailure.Source(Transport.Tls(code, phase)):
                    print(code)?
                    print(phase)?
                case _:
                    print("wrong cause")?
    print(buffer[0])?
    print(source.calls)?
    Ok(())
"#,
        "2\n47\nhandshake\n65\n3\n",
    );
}

#[test]
fn read_helpers_distinguish_eof_cancellation_invalid_progress_and_allocation() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut buffer = scratch()?
    for mode in range(6):
        mut source = Source(mode, 0)
        match read_exact[Transport, Source](&mut source, &mut buffer, 0, 4):
            case Ok(_):
                print("unexpected")?
            case Err(partial):
                print(partial.completed)?
                match partial.cause:
                    case ReadFailure.UnexpectedEof:
                        print("eof")?
                    case ReadFailure.InvalidProgress:
                        print("invalid")?
                    case ReadFailure.Source(Transport.Cancelled):
                        print("cancelled")?
                    case ReadFailure.Source(Transport.Allocation(AllocError.OutOfMemory)):
                        print("allocation")?
                    case _:
                        print("tls")?
    mut source = Source(0, 0)
    drop(read_exact[Transport, Source](&mut source, &mut buffer, 2, 2))
    drop(read_exact[Transport, Source](&mut source, &mut buffer, -1, 2))
    print(source.calls)?
    Ok(())
"#,
        "2\neof\n2\ntls\n2\ncancelled\n2\ninvalid\n2\ninvalid\n2\nallocation\n0\n",
    );
}

#[test]
fn copy_distinguishes_write_failure_and_partial_delivery() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut buffer = scratch()?
    for mode in range(5):
        mut source = Source(0, 0)
        mut sink = Sink(mode, 0, 0)
        match copy_all[Transport, Storage, Source, Sink](&mut source, &mut sink, &mut buffer):
            case Ok(count):
                print(count)?
            case Err(CopyFailure.Write(count, reason)):
                print(count)?
                match reason:
                    case WriteFailure.Source(Storage.Full(code)):
                        print(code)?
                    case WriteFailure.WriteZero:
                        print("zero")?
                    case WriteFailure.InvalidProgress:
                        print("invalid")?
                    case _:
                        print("wrong")?
            case _:
                print("wrong side")?
        print(sink.delivered)?
    Ok(())
"#,
        "2\n2\n1\n28\n1\n0\nzero\n0\n0\ninvalid\n0\n0\ninvalid\n0\n",
    );
}

#[test]
fn http_body_copy_has_one_error_path_and_preserves_transport_cause() {
    execute(r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | FramingError[Transport] | CopyFailure[FramingError[Transport], Storage]]:
    mut buffer = scratch()?
    for mode in range(6):
        mut body = fixed_body[Transport, Source](Source(mode, 0), 4)?
        mut sink = Sink(0, 0, 0)
        match copy_all[FramingError[Transport], Storage, FixedBody[Transport, Source], Sink](&mut body, &mut sink, &mut buffer):
            case Err(CopyFailure.Read(count, ReadFailure.Source(cause))):
                print(count)?
                match cause:
                    case FramingError.Transport(Transport.Tls(code, phase)):
                        print(code)?
                        print(phase)?
                    case FramingError.Transport(Transport.Cancelled):
                        print("cancelled")?
                    case FramingError.Transport(Transport.Allocation(AllocError.OutOfMemory)):
                        print("allocation")?
                    case FramingError.Truncated:
                        print("truncated")?
                    case FramingError.InvalidProgress:
                        print("invalid")?
                    case _:
                        print("wrong")?
            case _:
                print("wrong path")?
        match body.read(&mut buffer, 0, 4):
            case Err(StepError.Source(FramingError.Broken)):
                print("broken")?
            case _:
                print("reused")?
    mut finished = fixed_body[Transport, Source](Source(1, 0), 2)?
    mut sink = Sink(0, 0, 0)
    print(copy_all[FramingError[Transport], Storage, FixedBody[Transport, Source], Sink](&mut finished, &mut sink, &mut buffer)?)?
    Ok(())
"#, "2\ntruncated\nbroken\n2\n47\nhandshake\nbroken\n2\ncancelled\nbroken\n2\ninvalid\nbroken\n2\ninvalid\nbroken\n2\nallocation\nbroken\n2\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn typed_stream_path_runs_without_allocating() {
    execute(
        r#"
def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut buffer = scratch()?
    mut body = fixed_body[Transport, Source](Source(0, 0), 2)?
    mut sink = Sink(0, 0, 0)
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    result = copy_all[FramingError[Transport], Storage, FixedBody[Transport, Source], Sink](&mut body, &mut sink, &mut buffer)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(result?)?
    Ok(())
"#,
        "2\n",
    );
}
