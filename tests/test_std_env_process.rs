//! `std.env` and `std.process`: environment reads and owned children, run
//! against real local programs (`sh`, `cat`, `printf`, `sleep`).

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::process::{Command, Stdio};

const PRELUDE: &str = r#"import std.bytes
import std.env
import std.io
import std.path
import std.process
import std.time
from std.bytes import Bytes
from std.io import StreamError
from std.process import Child, Command, ExitStatus, Output, Stdio
from std.time import Instant

def text(bytes: &Bytes) -> Result[str, Failure]:
    Ok(bytes.to_text()?)

# A command with the three streams piped.
def piped(program: &str, arguments: &list[str]) -> Result[Command, Failure]:
    mut built = std.process.command(program)?
    for argument in arguments:
        built.arg(argument)?
    built.stdin(Stdio.Piped)
    built.stdout(Stdio.Piped)
    built.stderr(Stdio.Piped)
    Ok(built)

def sh(script: &str) -> Result[Command, Failure]:
    mut built = std.process.shell(script)?
    built.stdin(Stdio.Piped)
    built.stdout(Stdio.Piped)
    built.stderr(Stdio.Piped)
    Ok(built)

def collected(built: &Command) -> Result[Output, Failure]:
    mut child = built.spawn()?
    Ok(child.output(1000000)?)

def bad[T, E](result: &Result[T, E]) -> bool:
    match result:
        case Ok(_):
            False
        case Err(_):
            True

def absent[T](option: &Option[T]) -> bool:
    match option:
        case Some(_):
            False
        case Nothing:
            True

# Text holding a NUL, which no argument, variable or path can carry.
def nul() -> Result[str, Failure]:
    Ok(decode_utf8([110u8, 0u8, 108u8]?)?)

def out_text(built: &Command) -> Result[str, Failure]:
    output = collected(built)?
    text(&output.stdout)

def out_list(built: &Command) -> Result[list[u8], Failure]:
    output = collected(built)?
    Ok(output.stdout.to_list()?)

def elapsed(start: &Instant) -> u64:
    std.time.monotonic_now().saturating_duration_since(start).as_nanoseconds()

def show(status: &ExitStatus) -> Result[(), Failure]:
    print(status.success(), status.code(), status.signal())?
    Ok(())
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{PRELUDE}\ndef main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

/// Runs the program with a known environment; stdout and stderr are captured.
fn run(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = Command::new(executable)
        .current_dir(dir.path())
        .env("PLENTY_TEST_PLAIN", "plain value")
        .env("PLENTY_TEST_EMPTY", "")
        .env("PLENTY_TEST_RAW", OsStr::from_bytes(b"raw\xff"))
        .env_remove("PLENTY_TEST_ABSENT")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{source}\n{stderr}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end_matches('\n')
        .to_owned()
}

fn native(body: &str, expected: &str) {
    assert_eq!(run(&program(body)), expected.trim_matches('\n'), "{body}");
}

#[test]
fn env_reads_text_bytes_and_absence() {
    native(
        r#"
print(std.env.get("PLENTY_TEST_PLAIN")?)?
print(std.env.get("PLENTY_TEST_EMPTY")?)?
print(std.env.get("PLENTY_TEST_ABSENT")?)?
print(str.repr(std.env.get("PLENTY_TEST_RAW"))?)?
name = encode_utf8("PLENTY_TEST_RAW")?
print(std.env.get_bytes(&name)?)?
"#,
        r#"
Option[str].Some("plain value")
Option[str].Some("")
Option[str].Nothing
Result[Option[str], IoError].Err(IoError.Data(DataError.InvalidUtf8))
Option[list[u8]].Some([114, 97, 119, 255])
"#,
    );
}

#[test]
fn env_rejects_names_no_variable_can_have() {
    native(
        r#"
print(str.repr(std.env.get(""))?)?
print(str.repr(std.env.get("A=B"))?)?
print(str.repr(std.env.get(&nul()?))?)?
"#,
        r#"
Result[Option[str], IoError].Err(IoError.InvalidInput)
Result[Option[str], IoError].Err(IoError.InvalidInput)
Result[Option[str], IoError].Err(IoError.InvalidInput)
"#,
    );
}

#[test]
fn env_vars_snapshot_holds_every_variable_once() {
    native(
        r#"
plain = encode_utf8("PLENTY_TEST_PLAIN")?
raw = encode_utf8("PLENTY_TEST_RAW")?
mut seen = 0
for variable in &std.env.vars()?:
    if variable.name == plain:
        print(&variable.value)?
        seen += 1
    if variable.name == raw:
        print(&variable.value)?
        seen += 1
print(seen)?
"#,
        r#"
[112, 108, 97, 105, 110, 32, 118, 97, 108, 117, 101]
[114, 97, 119, 255]
2
"#,
    );
}

#[test]
fn current_dir_reads_and_changes_the_working_directory() {
    native(
        r#"
before = std.env.current_dir()?
print(before.is_absolute())?
root = std.path.from_text("/")?
std.env.set_current_dir(&root)?
print(std.env.current_dir()?.to_text()?)?
missing = std.path.from_text("/plenty-no-such-directory")?
print(str.repr(std.env.set_current_dir(&missing))?)?
# A child started afterwards inherits the change.
mut built = piped("pwd", &list[str].new()?)?
print(out_text(&built)?)?
"#,
        r#"
True
/
Result[(), IoError].Err(IoError.System(2))
/

"#,
    );
}

#[test]
fn arguments_reach_the_child_without_shell_parsing() {
    native(
        r#"
mut built = piped("printf", &["%s|"]?)?
built.arg("a; echo b")?
built.arg("$HOME `id` $(id)")?
built.arg("x | y && z > /dev/null")?
built.arg("*  two  spaces")?
built.arg("'quoted' \"double\" \\")?
built.arg("")?
built.arg("line\nbreak")?
output = collected(&built)?
print(text(&output.stdout)?)?
show(&output.status)?
"#,
        "a; echo b|$HOME `id` $(id)|x | y && z > /dev/null|*  two  spaces|'quoted' \"double\" \\||line\nbreak|\nTrue Option[i32].Some(0) Option[i32].Nothing",
    );
}

#[test]
fn shell_is_the_explicit_way_to_parse_a_script() {
    native(
        r#"
output = collected(&sh("echo one; echo two | tr a-z A-Z; echo three >&2")?)?
print(text(&output.stdout)?)?
print(text(&output.stderr)?)?
"#,
        "one\nTWO\n\nthree\n",
    );
}

#[test]
fn arguments_and_variables_need_not_be_utf8() {
    native(
        r#"
mut built = piped("printf", &["%s"]?)?
built.arg_bytes(&[255u8, 254u8, 32u8, 195u8]?)?
print(out_list(&built)?)?
mut env = piped("printenv", &["PLENTY_TEST_CHILD"]?)?
env.env_bytes(&encode_utf8("PLENTY_TEST_CHILD")?, &[128u8, 61u8, 129u8]?)?
print(out_list(&env)?)?
# The parent's own non-UTF-8 variable is inherited unchanged.
mut inherited = piped("printenv", &["PLENTY_TEST_RAW"]?)?
print(out_list(&inherited)?)?
"#,
        r#"
[255, 254, 32, 195]
[128, 61, 129, 10]
[114, 97, 119, 255, 10]
"#,
    );
}

#[test]
fn rejected_pieces_leave_the_command_unchanged() {
    native(
        r#"
print(bad(&std.process.command("")))?
print(bad(&std.process.command(&nul()?)))?
mut built = piped("printf", &["%s|"]?)?
print(str.repr(built.arg(&nul()?))?)?
print(str.repr(built.env("", "value"))?)?
print(str.repr(built.env("A=B", "value"))?)?
print(str.repr(built.env("NAME", &nul()?))?)?
print(str.repr(built.env_remove("A=B"))?)?
print(str.repr(built.current_dir(&std.path.from_bytes([47u8, 0u8]?)))?)?
print(str.repr(built.current_dir(&std.path.from_bytes(list[u8].new()?)))?)?
built.arg("kept")?
print(out_text(&built)?)?
"#,
        r#"
True
True
Result[(), IoError].Err(IoError.InvalidInput)
Result[(), IoError].Err(IoError.InvalidInput)
Result[(), IoError].Err(IoError.InvalidInput)
Result[(), IoError].Err(IoError.InvalidInput)
Result[(), IoError].Err(IoError.InvalidInput)
Result[(), IoError].Err(IoError.InvalidInput)
Result[(), IoError].Err(IoError.InvalidInput)
kept|
"#,
    );
}

#[test]
fn child_environment_and_directory_are_per_command() {
    native(
        r#"
script = "echo \"$PLENTY_TEST_PLAIN|$PLENTY_TEST_NEW|${PLENTY_TEST_EMPTY-unset}\"; pwd"
mut built = sh(script)?
built.env("PLENTY_TEST_NEW", "first")?
built.env("PLENTY_TEST_NEW", "second")?
built.env_remove("PLENTY_TEST_EMPTY")?
built.current_dir(&std.path.from_text("/usr")?)?
built.current_dir(&std.path.from_text("/")?)?
print(out_text(&built)?)?
# The parent is untouched.
print(std.env.get("PLENTY_TEST_NEW")?, std.env.get("PLENTY_TEST_EMPTY")?)?
print(std.env.current_dir()?.to_text()? == "/")?
mut bare = piped("/usr/bin/env", &list[str].new()?)?
bare.env("DROPPED", "by the clear")?
bare.env_clear()
bare.env("ONLY", "this")?
print(out_text(&bare)?)?
"#,
        "plain value|second|unset\n/\n\nOption[str].Nothing Option[str].Some(\"\")\nFalse\nONLY=this\n",
    );
}

#[test]
fn spawn_failure_is_an_error_value() {
    native(
        r#"
mut missing = std.process.command("/plenty-no-such-directory/program")?
print(bad(&missing.spawn()))?
match missing.spawn():
    case Ok(_):
        print("spawned")?
    case Err(error):
        print(error)?
mut unnamed = std.process.command("plenty-no-such-program-on-path")?
match unnamed.spawn():
    case Ok(_):
        print("spawned")?
    case Err(error):
        print(error)?
mut data = std.process.command("/etc/passwd")?
match data.spawn():
    case Ok(_):
        print("spawned")?
    case Err(error):
        print(error)?
mut nowhere = std.process.command("true")?
nowhere.current_dir(&std.path.from_text("/plenty-no-such-directory")?)?
match nowhere.spawn():
    case Ok(_):
        print("spawned")?
    case Err(error):
        print(error)?
# A failed spawn leaves nothing behind and the command still works.
nowhere.current_dir(&std.path.from_text("/")?)?
mut child = nowhere.spawn()?
show(&child.wait()?)?
"#,
        r#"
True
IoError.System(2)
IoError.System(2)
IoError.System(13)
IoError.System(2)
True Option[i32].Some(0) Option[i32].Nothing
"#,
    );
}

#[test]
fn exit_status_reports_a_code_or_a_signal() {
    native(
        r#"
mut failing = std.process.shell("exit 7")?
mut child = failing.spawn()?
show(&child.wait()?)?
# The status is kept: waiting again does not touch the OS.
show(&child.wait()?)?
match child.try_wait()?:
    case Some(status):
        show(&status)?
    case Nothing:
        print("running")?
mut high = std.process.shell("exit 255")?
mut last = high.spawn()?
show(&last.wait()?)?
mut suicide = std.process.shell("kill -11 $$")?
mut crashed = suicide.spawn()?
show(&crashed.wait()?)?
"#,
        r#"
False Option[i32].Some(7) Option[i32].Nothing
False Option[i32].Some(7) Option[i32].Nothing
False Option[i32].Some(7) Option[i32].Nothing
False Option[i32].Some(255) Option[i32].Nothing
False Option[i32].Nothing Option[i32].Some(11)
"#,
    );
}

#[test]
fn a_running_child_can_be_polled_terminated_and_killed() {
    native(
        r#"
mut sleeper = std.process.command("sleep")?
sleeper.arg("60")?
mut child = sleeper.spawn()?
print(child.id() > 1)?
match child.try_wait()?:
    case Some(_):
        print("ended")?
    case Nothing:
        print("running")?
child.terminate()?
show(&child.wait()?)?
# After reaping, a signal is a no-op rather than a hazard for a reused pid.
print(str.repr(child.terminate())?, str.repr(child.kill())?, str.repr(child.signal(1))?)?

mut second = sleeper.spawn()?
print(str.repr(second.signal(0))?, str.repr(second.signal(65))?)?
second.kill()?
show(&second.wait()?)?

# SIGTERM can be ignored; SIGKILL cannot.
mut stubborn = sh("trap '' TERM; echo ready; while :; do sleep 1; done")?
mut third = stubborn.spawn()?
mut out = std.io.from_file(third.take_stdout().unwrap())
mut line = std.bytes.empty()?
match std.io.read_up_to(&mut out, &mut line, 6):
    case Ok(count):
        print(count, text(&line)?)?
    case Err(_):
        print("read failed")?
third.terminate()?
print(absent(&third.wait_timeout(&std.time.milliseconds(200u32))?))?
third.kill()?
show(&third.wait()?)?
"#,
        r#"
True
running
False Option[i32].Nothing Option[i32].Some(15)
Result[(), IoError].Ok(()) Result[(), IoError].Ok(()) Result[(), IoError].Ok(())
Result[(), IoError].Err(IoError.InvalidInput) Result[(), IoError].Err(IoError.InvalidInput)
False Option[i32].Nothing Option[i32].Some(9)
6 ready

True
False Option[i32].Nothing Option[i32].Some(9)
"#,
    );
}

#[test]
fn wait_timeout_returns_early_or_gives_up() {
    native(
        r#"
mut quick = std.process.shell("sleep 0.05; exit 3")?
mut child = quick.spawn()?
start = std.time.monotonic_now()
match child.wait_timeout(&std.time.seconds(30u32))?:
    case Some(status):
        show(&status)?
    case Nothing:
        print("timed out")?
print(elapsed(&start) < 10000000000u64)?

mut sleeper = std.process.command("sleep")?
sleeper.arg("60")?
mut slow = sleeper.spawn()?
print(absent(&slow.wait_timeout(&std.time.milliseconds(0u32))?))?
before = std.time.monotonic_now()
print(absent(&slow.wait_timeout(&std.time.milliseconds(120u32))?))?
waited = elapsed(&before)
print(waited >= 120000000u64, waited < 10000000000u64)?
slow.kill()?
show(&slow.wait()?)?
"#,
        r#"
False Option[i32].Some(3) Option[i32].Nothing
True
True
True
True True
False Option[i32].Nothing Option[i32].Some(9)
"#,
    );
}

#[test]
fn large_output_on_both_pipes_does_not_deadlock() {
    // The child copies 1 MiB of stdin to stdout and stderr at once: every
    // pipe (64 KiB each) fills many times over unless all three are served.
    native(
        r#"
mut input = std.bytes.with_capacity(0)?
input.resize(1048576, 120u8)?
input.set(0, 60u8)?
input.set(1048575, 62u8)?
mut child = sh("tee /dev/stderr")?.spawn()?
match child.communicate(&input, 2000000):
    case Ok(output):
        show(&output.status)?
        print(output.stdout.len(), output.stderr.len())?
        print(output.stdout.get(0)?, output.stdout.get(1048575)?, output.stderr.get(0)?, output.stderr.get(1048575)?)?
        print(output.stdout.find(0u8, 0, 1048576)?)?
    case Err(_):
        print("failed")?

# Output only, with nothing fed: stderr first, then stdout.
mut noisy = sh("head -c 300000 /dev/zero >&2; head -c 200000 /dev/zero; exit 4")?.spawn()?
match noisy.output(300000):
    case Ok(output):
        show(&output.status)?
        print(output.stdout.len(), output.stderr.len())?
    case Err(_):
        print("failed")?
"#,
        r#"
True Option[i32].Some(0) Option[i32].Nothing
1048576 1048576
60 62 60 62
Option[i64].Nothing
False Option[i32].Some(4) Option[i32].Nothing
200000 300000
"#,
    );
}

#[test]
fn output_beyond_the_limit_is_refused_and_the_child_stays_owned() {
    native(
        r#"
mut exact = sh("printf 12345")?.spawn()?
match exact.output(5):
    case Ok(output):
        print(text(&output.stdout)?)?
    case Err(_):
        print("failed")?
mut over = sh("printf 123456")?.spawn()?
match over.output(5):
    case Ok(_):
        print("accepted")?
    case Err(error):
        print(error)?
show(&over.wait()?)?
mut negative = sh("true")?.spawn()?
match negative.output(-1):
    case Ok(_):
        print("accepted")?
    case Err(error):
        print(error)?
show(&negative.wait()?)?

# An endless writer: the limit bounds memory, then the caller decides.
mut endless = piped("yes", &list[str].new()?)?.spawn()?
match endless.output(100000):
    case Ok(_):
        print("accepted")?
    case Err(error):
        print(error)?
match endless.try_wait()?:
    case Some(_):
        print("ended")?
    case Nothing:
        print("running")?
endless.kill()?
show(&endless.wait()?)?
"#,
        r#"
12345
std.io.StreamError.OutOfRange
True Option[i32].Some(0) Option[i32].Nothing
std.io.StreamError.OutOfRange
True Option[i32].Some(0) Option[i32].Nothing
std.io.StreamError.OutOfRange
running
False Option[i32].Nothing Option[i32].Some(9)
"#,
    );
}

#[test]
fn taken_pipes_are_files_with_partial_reads_and_writes() {
    native(
        r#"
mut child = piped("cat", &list[str].new()?)?.spawn()?
mut stdin = child.take_stdin().unwrap()
mut stdout = child.take_stdout().unwrap()
print(absent(&child.take_stdin()), absent(&child.take_stdout()))?
source = std.bytes.from_text("partial")?
print(source.write_file(&mut stdin, 0, 4)?)?
mut buffer = std.bytes.with_capacity(0)?
buffer.resize(64, 46u8)?
# A read returns what has arrived, not the 64 bytes asked for.
mut count = buffer.read_file(&mut stdout, 0, 64)?
print(count, text(&buffer.slice(0, count)?)?)?
print(source.write_file(&mut stdin, 4, 7)?)?
count = buffer.read_file(&mut stdout, 0, 64)?
print(count, text(&buffer.slice(0, count)?)?)?
# The pipe ends enforce their direction.
print(str.repr(buffer.read_file(&mut stdin, 0, 1))?, str.repr(source.write_file(&mut stdout, 0, 1))?)?
# Nothing here holds stdin, so the caller closes it to end the child's input.
stdin.close()?
print(buffer.read_file(&mut stdout, 0, 64)?)?
show(&child.wait()?)?
stdout.close()?
"#,
        r#"
True True
4
4 part
3
3 ial
Result[i64, IoError].Err(IoError.NotReadable) Result[i64, IoError].Err(IoError.NotWritable)
0
True Option[i32].Some(0) Option[i32].Nothing
"#,
    );
}

#[test]
fn pipes_work_through_std_io_streams() {
    native(
        r#"
mut child = piped("tr", &["a-z", "A-Z"]?)?.spawn()?
mut writer = std.io.from_file(child.take_stdin().unwrap())
mut reader = std.io.from_file(child.take_stdout().unwrap())
source = std.bytes.from_text("through streams\n")?
match std.io.write_all(&mut writer, &source, 0, source.len()):
    case Ok(_):
        pass
    case Err(_):
        print("write failed")?
match writer.close():
    case Ok(_):
        pass
    case Err(_):
        print("close failed")?
mut out = std.bytes.empty()?
match std.io.read_up_to(&mut reader, &mut out, 100):
    case Ok(count):
        print(count, text(&out)?)?
    case Err(_):
        print("read failed")?
show(&child.wait()?)?
"#,
        "16 THROUGH STREAMS\n\nTrue Option[i32].Some(0) Option[i32].Nothing",
    );
}

#[test]
fn writing_to_a_child_that_has_gone_is_an_error_not_a_signal() {
    native(
        r#"
mut child = piped("true", &list[str].new()?)?.spawn()?
print(absent(&child.wait_timeout(&std.time.seconds(30u32))?))?
mut stdin = child.take_stdin().unwrap()
source = std.bytes.from_text("unread")?
print(str.repr(source.write_file(&mut stdin, 0, 6))?)?
print(str.repr(stdin.write("text"))?)?
stdin.close()?

# communicate stops feeding and still reports the status.
mut input = std.bytes.with_capacity(0)?
input.resize(1048576, 120u8)?
mut gone = sh("exit 9")?.spawn()?
match gone.communicate(&input, 100):
    case Ok(output):
        show(&output.status)?
        print(output.stdout.len(), output.stderr.len())?
    case Err(_):
        print("failed")?
# A reader that takes only part of the input.
mut partial = sh("head -c 10")?.spawn()?
match partial.communicate(&input, 100):
    case Ok(output):
        show(&output.status)?
        print(text(&output.stdout)?)?
    case Err(_):
        print("failed")?
print("alive")?
"#,
        r#"
False
Result[i64, IoError].Err(IoError.System(32))
Result[i64, IoError].Err(IoError.System(32))
False Option[i32].Some(9) Option[i32].Nothing
0 0
True Option[i32].Some(0) Option[i32].Nothing
xxxxxxxxxx
alive
"#,
    );
}

#[test]
fn stdio_modes_select_null_inherited_or_piped_streams() {
    native(
        r#"
mut quiet = std.process.shell("echo hidden; echo hidden >&2; cat")?
quiet.stdin(Stdio.Null)
quiet.stdout(Stdio.Null)
quiet.stderr(Stdio.Null)
mut child = quiet.spawn()?
print(absent(&child.take_stdin()), absent(&child.take_stdout()), absent(&child.take_stderr()))?
# Nothing is piped, so there is nothing to collect.
match child.output(10):
    case Ok(output):
        show(&output.status)?
        print(output.stdout.len(), output.stderr.len())?
    case Err(_):
        print("failed")?
# Inherit is the default: the child writes to this program's stdout.
mut loud = std.process.shell("echo inherited")?
mut second = loud.spawn()?
show(&second.wait()?)?
# A stream switched back from Piped is not captured.
mut mixed = sh("echo out; echo err >&2")?
mixed.stderr(Stdio.Null)
output = collected(&mixed)?
print(text(&output.stdout)?, output.stderr.len())?
"#,
        r#"
True True True
True Option[i32].Some(0) Option[i32].Nothing
0 0
inherited
True Option[i32].Some(0) Option[i32].Nothing
out
 0
"#,
    );
}

#[test]
fn waited_children_are_reaped() {
    // /proc/PID/task/PID/children lists live and zombie children alike.
    native(
        r#"
census = "cat /proc/$PPID/task/$PPID/children | wc -w"
mut quick = std.process.command("true")?
for round in range(40):
    mut child = quick.spawn()?
    if round % 2 == 0:
        child.wait()?
    else:
        child.wait_timeout(&std.time.seconds(30u32))?
print(out_text(&sh(census)?)?)?

"#,
        "1\n",
    );
}

// A checked build reports the record of an unreaped child as a leak at exit,
// so this policy is only observable without the allocation accounting.
#[cfg(not(feature = "runtime-checks"))]
#[test]
fn a_dropped_child_is_neither_stopped_nor_reaped() {
    native(
        r#"
census = "cat /proc/$PPID/task/$PPID/children | wc -w"
mut quick = std.process.command("true")?
mut ids = list[i64].new()?
for round in range(3):
    child = quick.spawn()?
    ids.append(child.id())?
std.time.sleep(&std.time.milliseconds(300u32))
print(out_text(&sh(census)?)?)?
print(ids[0] != ids[1], ids[1] != ids[2])?
"#,
        "4\n\nTrue True",
    );
}

#[test]
fn a_command_is_reusable_and_children_are_independent() {
    native(
        r#"
mut built = sh("read line; echo \"got $line\"")?
mut first = built.spawn()?
mut second = built.spawn()?
print(first.id() != second.id())?
# Answer the second child first: neither blocks the other.
late = second.communicate(&std.bytes.from_text("second\n")?, 100)?
early = first.communicate(&std.bytes.from_text("first\n")?, 100)?
print(text(&late.stdout)?, text(&early.stdout)?)?
"#,
        "True\ngot second\n got first\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_before_anything_is_spawned() {
    // Process exit checks that nothing leaked.
    native(
        r#"
mut built = std.process.command("true")?
name = encode_utf8("an argument long enough to need more storage than the command holds")?
print("__test_fail_allocations_after_0__")?
argument = built.arg_bytes(&name)
variable = built.env_bytes(&name, &name)
started = bad(&built.spawn())
fresh = bad(&std.process.command_bytes(&name))
print("__test_restore_allocations__")?
print(str.repr(argument)?, str.repr(variable)?)?
print(started, fresh)?
mut child = built.spawn()?
show(&child.wait()?)?
"#,
        r#"
Result[(), IoError].Err(IoError.Data(DataError.Allocation(AllocError.OutOfMemory))) Result[(), IoError].Err(IoError.Data(DataError.Allocation(AllocError.OutOfMemory)))
True True
True Option[i32].Some(0) Option[i32].Nothing
"#,
    );
}
