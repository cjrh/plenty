use std::process::Command;

const FIXTURES: &str = r#"
import std.bytes
from std.bytes import Bytes
from streams import StepError, ReadFailure, WriteFailure, CopyFailure, read_exact, write_all, copy_all
from http_body import FixedBody, FramingError, fixed_body
pub enum Transport:
    Tls(i64, str)
    Cancelled
    Allocation(AllocError)
pub enum Storage:
    Full(i64)
def scratch() -> Result[Bytes, Failure]:
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
    let workspace = fixture(&format!("{FIXTURES}\n{body}"));
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
    let source = format!("{FIXTURES}\ndef main() -> Result[(), Failure]:\n    mut source = Source(0, 0)\n    mut buffer = scratch()?\n    drop(read_exact[Storage, Source](&mut source, &mut buffer, 0, 4))\n    Ok(())\n");
    let workspace = fixture(&source);
    let error = plenty::check_file(&workspace.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("signature"), "{error}");
}

#[test]
fn protocol_bounds_infer_errors_and_nested_framing_without_explicit_types() {
    execute(
        r#"
def main() -> Result[(), Failure]:
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
def main() -> Result[(), Failure]:
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
def main() -> Result[(), Failure]:
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
def main() -> Result[(), Failure]:
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
def main() -> Result[(), Failure]:
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
def main() -> Result[(), Failure]:
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
def main() -> Result[(), Failure]:
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
