//! `std.secrets`: OS-backed bytes and tokens, with retry and failure policy
//! exercised through an injected scripted source.
mod support;
use support::check_file_error as rejected;

const HEADER: &str = r#"import std.bytes
import std.secrets
from std.bytes import Bytes
from std.io import StreamError
from std.secrets import SecretsError

# Each read takes the next step: a positive count delivers at most that many
# bytes, 0 is end of input, -1 an interrupted call, -2 a device error and -3 a
# count beyond the request. After the script, every read fills its range.
# Delivered bytes count upwards from `value`.
pub class Script:
    pub steps: list[i64]
    pub at: i64
    pub value: u8

    pub def read(self: &mut Script, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        mut step = stop - start
        if self.at < len(self.steps):
            step = self.steps[self.at]
        self.at += 1
        if step == -1:
            return Err(StreamError.Interrupted)
        if step == -2:
            return Err(StreamError.Io(IoError.System(5)))
        if step == -3:
            return Ok(stop - start + 1)
        if step > stop - start:
            step = stop - start
        for offset in range(step):
            drop(buffer.set(start + offset, self.value))
            self.value = self.value.wrapping_add(1u8)
        Ok(step)

"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.secrets.SecretsError]:\n{body}    Ok(())\n")
}

fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.secrets.SecretsError", "E")
        .replace("std.io.StreamError", "S")
        .replace("std.bytes.Bytes", "Bytes")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

#[test]
fn short_and_interrupted_reads_are_retried_until_full() {
    native(
        r#"
mut source = std.secrets.source_from(Script([1, -1, 3, -1, -1, 2]?, 0, 1u8))
token = source.token_bytes(10)?
# Seven reads: six scripted, then one that completes the last four bytes.
print(token.to_list()?, source.inner.at)?
"#,
        "[1, 2, 3, 4, 5, 6, 7, 8, 9, 10] 7",
    );
}

#[test]
fn failures_report_the_cause_and_zero_the_whole_range() {
    native(
        r#"
mut target = std.bytes.empty()?
target.resize(12, 9u8)?

mut failing = std.secrets.source_from(Script([3, -1, 2, -2]?, 0, 1u8))
print(str.repr(failing.fill(&mut target, 2, 10))?, target.to_list()?, failing.inner.at)?

target.fill(0, 12, 9u8)?
mut ended = std.secrets.source_from(Script([4, 0]?, 0, 1u8))
print(str.repr(ended.fill(&mut target, 2, 10))?, target.to_list()?)?

target.fill(0, 12, 9u8)?
mut broken = std.secrets.source_from(Script([2, -3]?, 0, 1u8))
print(str.repr(broken.fill(&mut target, 2, 10))?, target.to_list()?)?

# A failed token is released; the caller never sees a partial one.
mut lost = std.secrets.source_from(Script([5, -2]?, 0, 1u8))
print(str.repr(lost.token_bytes(8))?)?
mut text = std.secrets.source_from(Script([5, -2, 5, 0, -2, 7, -2]?, 0, 1u8))
print(str.repr(text.token_hex(8))?, str.repr(text.token_urlsafe(8))?)?
print(str.repr(text.next_u64())?, str.repr(text.below(10u64))?)?
"#,
        "Result[(), E].Err(E.Entropy(S.Io(IoError.System(5)))) [9, 9, 0, 0, 0, 0, 0, 0, 0, 0, 9, 9] 4
Result[(), E].Err(E.Entropy(S.UnexpectedEof)) [9, 9, 0, 0, 0, 0, 0, 0, 0, 0, 9, 9]
Result[(), E].Err(E.Entropy(S.InvalidProgress)) [9, 9, 0, 0, 0, 0, 0, 0, 0, 0, 9, 9]
Result[Bytes, E].Err(E.Entropy(S.Io(IoError.System(5))))
Result[str, E].Err(E.Entropy(S.Io(IoError.System(5)))) Result[str, E].Err(E.Entropy(S.UnexpectedEof))
Result[u64, E].Err(E.Entropy(S.Io(IoError.System(5)))) Result[u64, E].Err(E.Entropy(S.Io(IoError.System(5))))",
    );
}

#[test]
fn invalid_requests_never_reach_the_source() {
    native(
        r#"
mut target = std.bytes.empty()?
target.resize(4, 9u8)?
mut source = std.secrets.source_from(Script(list[i64].new()?, 0, 1u8))
print(str.repr(source.fill(&mut target, -1, 2))?)?
print(str.repr(source.fill(&mut target, 3, 2))?)?
print(str.repr(source.fill(&mut target, 0, 5))?)?
print(str.repr(source.token_bytes(-1))?)?
print(str.repr(source.token_hex(-1))?)?
print(str.repr(source.token_urlsafe(-1))?)?
print(str.repr(source.below(0u64))?)?
source.fill(&mut target, 2, 2)?
print(source.token_bytes(0)?.len(), source.token_hex(0)? == "", source.token_urlsafe(0)? == "")?
print(target.to_list()?, source.inner.at)?
"#,
        "Result[(), E].Err(E.OutOfRange)
Result[(), E].Err(E.OutOfRange)
Result[(), E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[str, E].Err(E.OutOfRange)
Result[str, E].Err(E.OutOfRange)
Result[u64, E].Err(E.EmptyRange)
0 True True
[9, 9, 9, 9] 0",
    );
}

#[test]
fn tokens_encode_exactly_the_bytes_drawn() {
    // 0x7978..71 is below the rejection threshold for bound 2^63 + 1; the
    // second word, 0x807F..79, is accepted.
    let accepted = 0x807F_7E7D_7C7B_7A79u64 % ((1u64 << 63) + 1);
    let expected = format!(
        "0102030405060708090a0b0c0d0e0f10
-_z9_v8A
578437695752307201
1 9
{accepted} 129"
    );
    native(
        r#"
mut counting = std.secrets.source_from(Script(list[i64].new()?, 0, 1u8))
print(counting.token_hex(16)?)?
mut high = std.secrets.source_from(Script([2, 1]?, 0, 251u8))
print(high.token_urlsafe(6)?)?
mut words = std.secrets.source_from(Script(list[i64].new()?, 0, 1u8))
print(words.next_u64()?)?
mut small = std.secrets.source_from(Script(list[i64].new()?, 0, 1u8))
drawn = small.below(10u64)?
print(drawn, small.inner.value)?
mut retried = std.secrets.source_from(Script(list[i64].new()?, 0, 113u8))
kept = retried.below(9223372036854775809u64)?
print(kept, retried.inner.value)?
"#,
        &expected,
    );
}

#[test]
fn os_tokens_have_the_requested_size_and_differ() {
    native(
        r#"
first = std.secrets.token_bytes(32)?
second = std.secrets.token_bytes(32)?
mut same = True
for index in range(32):
    if first.get(index)? != second.get(index)?:
        same = False
print(first.len(), second.len(), same)?

digits = std.secrets.token_hex(16)?
mut lower_hex = True
for letter in digits:
    if not (letter >= "0" and letter <= "9") and not (letter >= "a" and letter <= "f"):
        lower_hex = False
print(len(digits), lower_hex, digits == std.secrets.token_hex(16)?)?
print(len(std.secrets.token_urlsafe(16)?), len(std.secrets.token_urlsafe(0)?))?
print(std.secrets.token_bytes(0)?.len(), std.secrets.random_u64()? == std.secrets.random_u64()?)?
mut within = True
for _ in range(200):
    if std.secrets.below(6u64)? >= 6u64:
        within = False
print(within, std.secrets.below(1u64)?)?
print(str.repr(std.secrets.token_bytes(-1))?)?
print(str.repr(std.secrets.below(0u64))?)?
"#,
        "32 32 False
32 True False
22 0
0 False
True 0
Result[Bytes, E].Err(E.OutOfRange)
Result[u64, E].Err(E.EmptyRange)",
    );
}

#[test]
fn os_fill_covers_a_large_range_and_nothing_else() {
    // Larger than any single guaranteed getrandom transfer, so the range is
    // only complete if every reported count was honoured.
    native(
        r#"
size = 3000000
mut target = std.bytes.with_capacity(size + 2)?
target.resize(size + 2, 0u8)?
std.secrets.fill(&mut target, 1, size + 1)?
mut tail = 0
for index in range(size - 4096, size + 1):
    if target.get(index)? != 0u8:
        tail += 1
mut head = 0
for index in range(1, 4097):
    if target.get(index)? != 0u8:
        head += 1
print(target.get(0)?, target.get(size + 1)?, head > 3900, tail > 3900)?
print(str.repr(std.secrets.fill(&mut target, 0, size + 3))?)?
mut handle = std.secrets.os_entropy()
print(str.repr(handle.read(&mut target, 0, 16))?)?
"#,
        "0 0 True True
Result[(), E].Err(E.OutOfRange)
Result[i64, S].Ok(16)",
    );
}

#[test]
fn secrets_are_available_in_worker_threads() {
    let source = format!(
        "{HEADER}{}",
        r#"
def work(count: i64) -> ():
    token = std.secrets.token_hex(count).unwrap()
    print(len(token)).unwrap()

def main() -> Result[(), AllocError | IoError | ThreadError]:
    with spawn(work, 24)?:
        pass
    Ok(())
"#
    );
    assert_eq!(output(&source), "48");
}

#[test]
fn the_os_source_cannot_be_forged_and_random_is_a_separate_module() {
    let forged = rejected(&program("handle = std.secrets.OsEntropy(1u8)\n"));
    assert!(forged.contains("private"), "{forged}");
    let seeded = rejected(&program("rng = std.secrets.seeded(1u64)\n"));
    assert!(seeded.contains("seeded"), "{seeded}");
    let unknown = rejected("import std.entropy\n\ndef main() -> ():\n    pass\n");
    assert!(unknown.contains("std.random, std.secrets"), "{unknown}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn filling_an_existing_buffer_does_not_allocate() {
    native(
        r#"
mut target = std.bytes.empty()?
target.resize(64, 0u8)?
mut source = std.secrets.os()
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
std.secrets.fill(&mut target, 0, 64)?
source.fill(&mut target, 8, 56)?
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(target.len())?
"#,
        "64",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_and_releases_partial_tokens() {
    // A token buffer is two allocations (storage, then owner); the text forms
    // add an encoded buffer and the string. Failing each stage in turn must
    // free everything acquired before it, which the exit-time balance check
    // verifies.
    let failure = "E].Err(E.Allocation(AllocError.OutOfMemory))";
    let expected = format!(
        "Result[Bytes, {failure}\n\
         Result[str, {failure}\n\
         Result[str, {failure}\n\
         Result[u64, {failure}\n\
         Result[u64, {failure}\n\
         Result[str, {failure}\n\
         Result[str, {failure}\n\
         Result[str, {failure}\n\
         Result[str, {failure}"
    );
    native(
        r#"
print("__test_fail_allocations_after_0__")?
a = std.secrets.token_bytes(40)
b = std.secrets.token_hex(40)
c = std.secrets.token_urlsafe(40)
d = std.secrets.random_u64()
e = std.secrets.below(10u64)
print("__test_fail_allocations_after_2__")?
f = std.secrets.token_hex(40)
print("__test_fail_allocations_after_2__")?
g = std.secrets.token_urlsafe(40)
print("__test_fail_allocations_after_4__")?
h = std.secrets.token_hex(40)
print("__test_fail_allocations_after_4__")?
i = std.secrets.token_urlsafe(40)
print("__test_restore_allocations__")?
print(str.repr(a)?)?
print(str.repr(b)?)?
print(str.repr(c)?)?
print(str.repr(d)?)?
print(str.repr(e)?)?
print(str.repr(f)?)?
print(str.repr(g)?)?
print(str.repr(h)?)?
print(str.repr(i)?)?
"#,
        &expected,
    );
}

#[test]
fn the_native_primitive_checks_its_range_and_never_resizes() {
    native(
        r#"
mut target = std.bytes.with_capacity(64)?
target.resize(8, 0u8)?
print(str.repr(target.read_os_entropy(2, 2))?)?
print(str.repr(target.read_os_entropy(0, 8))?, target.len())?
print(str.repr(target.read_os_entropy(0, 9))?)?
print(str.repr(target.read_os_entropy(-1, 4))?)?
print(str.repr(target.read_os_entropy(5, 4))?)?
mut handle = std.secrets.os_entropy()
print(str.repr(handle.read(&mut target, 4, 9))?)?
"#,
        "Result[i64, IoError].Ok(0)\n\
         Result[i64, IoError].Ok(8) 8\n\
         Result[i64, IoError].Err(IoError.InvalidInput)\n\
         Result[i64, IoError].Err(IoError.InvalidInput)\n\
         Result[i64, IoError].Err(IoError.InvalidInput)\n\
         Result[i64, S].Err(S.Io(IoError.InvalidInput))",
    );
}
