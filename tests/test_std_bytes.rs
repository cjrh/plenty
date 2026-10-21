//! `std.bytes`: compact owning buffers and checked `&Bytes` range access.
mod support;
use support::check_file_error as rejected;

const HEADER: &str = "import std.bytes\nfrom std.bytes import Bytes, BytesError\n";

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{HEADER}def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | RangeError]:\n{body}    Ok(())\n"
    )
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
}

/// Exit status and stderr of a program expected to stop at a trap.
fn failure(source: &str) -> (Option<i32>, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

const OUT_OF_RANGE: &str = "std.bytes.BytesError].Err(std.bytes.BytesError.OutOfRange)";

#[test]
fn buffers_grow_shrink_and_report_explicit_capacity() {
    native(
        r#"
mut buffer = std.bytes.with_capacity(16)?
print(buffer.len(), buffer.capacity(), buffer.is_empty())?
for n in range(16):
    buffer.push(u8.try_from(n * 16)?)?
print(buffer.len(), buffer.capacity(), buffer.get(15)?)?
buffer.push(255u8)?
print(buffer.len(), buffer.capacity() >= 17)?
print(buffer.pop(), buffer.pop())?
buffer.truncate(2)?
buffer.resize(4, 9u8)?
buffer.set(0, 7u8)?
print(buffer.to_list()?)?
buffer.reserve(100)?
print(buffer.capacity() >= buffer.len() + 100, buffer.len())?
buffer.clear()
print(buffer.len(), buffer.pop(), buffer.capacity() >= 104)?
empty = std.bytes.empty()?
print(empty.len(), empty.capacity())?
"#,
        r#"
0 16 True
16 16 240
17 True
Option[u8].Some(255) Option[u8].Some(240)
[7, 16, 9, 9]
True 4
0 Option[u8].Nothing True
0 0
"#,
    );
}

#[test]
fn every_byte_value_survives_including_nul_and_invalid_utf8() {
    native(
        r#"
mut buffer = std.bytes.from_text("a\0é")?
print(buffer.len(), buffer.get(1)?, buffer.to_text()? == "a\0é")?
buffer.push(255u8)?
print(str.repr(buffer.to_text())?)?
print(buffer)?
print(buffer.to_list()?)?
values = [0u8, 128u8, 255u8, 10u8]?
packed = std.bytes.from_list(&values)?
print(packed.to_list()? == values, packed.len(), len(values))?
truncated = std.bytes.from_text("é")?.slice(0, 1)?
print(str.repr(truncated.to_text())?)?
"#,
        r#"
4 0 True
Result[str, std.bytes.BytesError].Err(std.bytes.BytesError.InvalidUtf8)
std.bytes.Bytes(_owner=b"a\x00\xc3\xa9\xff")
[97, 0, 195, 169, 255]
True 4 4
Result[str, std.bytes.BytesError].Err(std.bytes.BytesError.InvalidUtf8)
"#,
    );
}

#[test]
fn ranges_follow_one_rule_without_clamping_or_negative_offsets() {
    let expected = format!(
        "0 0 True Option[i64].Nothing\n\
         Result[u8, {OUT_OF_RANGE}\n\
         Result[u8, {OUT_OF_RANGE}\n\
         Result[(), {OUT_OF_RANGE}\n\
         Result[std.bytes.Bytes, {OUT_OF_RANGE}\n\
         Result[std.bytes.Bytes, {OUT_OF_RANGE}\n\
         Result[std.bytes.Bytes, {OUT_OF_RANGE}\n\
         Result[Option[i64], {OUT_OF_RANGE}\n\
         Result[(), {OUT_OF_RANGE}\n\
         Result[(), {OUT_OF_RANGE}\n\
         Result[(), {OUT_OF_RANGE}\n\
         Result[(), {OUT_OF_RANGE}\n\
         Result[(), {OUT_OF_RANGE}\n\
         Result[(), {OUT_OF_RANGE}\n\
         Result[(), {OUT_OF_RANGE}\n\
         Result[std.bytes.Bytes, {OUT_OF_RANGE}\n\
         [97, 98, 99] 3"
    );
    native(
        r#"
mut buffer = std.bytes.from_text("abc")?
other = std.bytes.from_text("xy")?
tail = buffer.slice(3, 3)?
print(tail.len(), tail.capacity(), tail.to_text()? == "", buffer.find(97u8, 1, 1)?)?
print(str.repr(buffer.get(3))?)?
print(str.repr(buffer.get(-1))?)?
print(str.repr(buffer.set(3, 0u8))?)?
print(str.repr(buffer.slice(-1, 2))?)?
print(str.repr(buffer.slice(2, 1))?)?
print(str.repr(buffer.slice(0, 4))?)?
print(str.repr(buffer.find(0u8, 0, 4))?)?
print(str.repr(buffer.fill(1, 4, 0u8))?)?
print(str.repr(buffer.extend_range(&other, 1, 3))?)?
print(str.repr(buffer.overwrite(2, &other, 0, 2))?)?
print(str.repr(buffer.overwrite(-1, &other, 0, 1))?)?
print(str.repr(buffer.truncate(4))?)?
print(str.repr(buffer.resize(-1, 0u8))?)?
print(str.repr(buffer.reserve(-1))?)?
print(str.repr(std.bytes.with_capacity(-1))?)?
print(buffer.to_list()?, buffer.len())?
"#,
        &expected,
    );
}

#[test]
fn explicit_copies_are_independent_and_compare_by_content() {
    native(
        r#"
mut buffer = std.bytes.from_text("hello world")?
mut word = buffer.slice(6, 11)?
mut whole = buffer.copy()?
print(whole == buffer, word == buffer, word.capacity())?
word.set(0, 87u8)?
whole.fill(0, 5, 45u8)?
print(buffer.to_text()?, word.to_text()?, whole.to_text()?)?
buffer.overwrite(0, &word, 0, 5)?
buffer.extend(&word)?
buffer.extend_range(&word, 0, 1)?
buffer.extend_range(&word, 5, 5)?
print(buffer.to_text()?, buffer.find(87u8, 1, buffer.len())?, buffer.find(0u8, 0, buffer.len())?)?
"#,
        r#"
True False 5
hello world World ----- world
World worldWorldW Option[i64].Some(11) Option[i64].Nothing
"#,
    );
}

#[test]
fn shared_views_read_ranges_and_exclusive_views_write_in_place() {
    let source = format!(
        "{HEADER}{}",
        r#"
def checksum(view: &Bytes, start: i64, stop: i64) -> Result[i64, BytesError]:
    mut total = 0
    for index in range(start, stop):
        total += i64(view.get(index)?)
    Ok(total)

def upper(view: &mut Bytes, start: i64, stop: i64) -> Result[(), BytesError]:
    for index in range(start, stop):
        value = view.get(index)?
        if value >= 97u8 and value <= 122u8:
            view.set(index, value - 32u8)?
    Ok(())

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut buffer = std.bytes.from_text("abc def")?
    view = &buffer
    print(checksum(view, 0, 3)?, checksum(view, 3, 3)?, str.repr(checksum(view, 4, 8))?)?
    upper(&mut buffer, 4, 7)?
    print(buffer.to_text()?)?
    Ok(())
"#
    );
    assert_eq!(
        output(&source),
        "294 0 Result[i64, std.bytes.BytesError].Err(std.bytes.BytesError.OutOfRange)\nabc DEF"
    );
}

#[test]
fn growth_mutation_and_movement_are_rejected_while_a_view_is_live() {
    let check = |body: &str, expected: &str| {
        let source = format!(
            "{HEADER}def take(buffer: Bytes) -> i64:\n    buffer.len()\n{}",
            program(body).strip_prefix(HEADER).unwrap()
        );
        let error = rejected(&source);
        assert!(error.contains(expected), "{body}\n{error}");
    };
    let modify = "cannot modify or exclusively borrow `buffer` while it is borrowed";
    for relocation in [
        "buffer.push(1u8)?",
        "buffer.reserve(64)?",
        "buffer.resize(64, 0u8)?",
        "buffer.set(0, 1u8)?",
        "buffer.clear()",
        "buffer.truncate(0)?",
    ] {
        check(
            &format!(
                "mut buffer = std.bytes.from_text(\"abc\")?\nview = &buffer\n{relocation}\nprint(view.len())?"
            ),
            modify,
        );
    }
    check(
        "mut buffer = std.bytes.from_text(\"abc\")?\nview = &buffer\ncount = take(buffer)\nprint(view.len(), count)?",
        "cannot move `buffer` while it is borrowed",
    );
    check(
        "mut buffer = std.bytes.from_text(\"abc\")?\nview = &mut buffer\ncount = buffer.len()\nview.push(1u8)?\nprint(count)?",
        "cannot read or borrow `buffer` while it is exclusively borrowed",
    );
    check(
        "mut buffer = std.bytes.from_text(\"abc\")?\nbuffer.extend(&buffer)?",
        modify,
    );
    check(
        "buffer = std.bytes.from_text(\"abc\")?\nbuffer.push(1u8)?",
        "mutable borrowing requires a mut binding",
    );
}

#[test]
fn buffers_are_resources_with_a_private_owner() {
    let check = |body: &str, expected: &str| {
        let error = rejected(&program(body));
        assert!(error.contains(expected), "{body}\n{error}");
    };
    check(
        "buffer = std.bytes.empty()?\nother = copy(buffer)?",
        "this resource cannot be copied",
    );
    check(
        "buffer = std.bytes.empty()?\nother = buffer\nprint(buffer.len())?",
        "moved",
    );
    check(
        "buffer = std.bytes.empty()?\nprint(buffer._owner)?",
        "private",
    );
    check("buffer = Bytes(__bytes_new(0)?)", "private");
}

#[test]
fn index_syntax_reads_and_writes_with_list_rules() {
    native(
        r#"
mut buffer = std.bytes.from_text("abcd")?
print(buffer[0], buffer[-1], buffer[-4])?
buffer[1] = 90u8
buffer[-1] = 33u8
buffer[0] += 1u8
print(buffer.to_text()?)?
mut total = 0
for index in range(buffer.len()):
    total += i64(buffer[index])
print(total)?
print(std.bytes.from_text("xyz")?[-1])?
"#,
        "97 100 97\nbZc!\n320\n122",
    );
}

#[test]
fn index_syntax_works_through_references_and_fields() {
    let source = format!(
        "{HEADER}{}",
        r#"
class Frame:
    data: Bytes

def poke(frame: &mut Frame, index: i64, value: u8) -> ():
    frame.data[index] = value

def first(buffer: &Bytes) -> u8:
    buffer[0]

def bump(buffer: &mut Bytes) -> ():
    buffer[-1] += 1u8

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:
    mut frame = Frame(std.bytes.from_text("xyz")?)
    poke(&mut frame, -2, 65u8)
    bump(&mut frame.data)
    print(first(&frame.data), frame.data[1], frame.data[2])?
    Ok(())
"#
    );
    assert_eq!(output(&source), "120 65 123");
}

#[test]
fn missing_byte_index_traps_with_the_list_message() {
    for (statement, message) in [
        (
            "print(buffer[3])?",
            "index out of bounds: index 3, length 3",
        ),
        (
            "buffer[-4] = 0u8",
            "index out of bounds: index -4, length 3",
        ),
        ("buffer[5] += 1u8", "index out of bounds: index 5, length 3"),
    ] {
        let source = program(&format!(
            "mut buffer = std.bytes.from_text(\"abc\")?\n{statement}"
        ));
        let (status, stderr) = failure(&source);
        assert_eq!(status, Some(1), "{statement}\n{stderr}");
        assert!(stderr.contains(message), "{statement}\n{stderr}");
        assert!(stderr.contains("main.plenty:5:"), "{statement}\n{stderr}");
    }
}

#[test]
fn iteration_yields_bytes_and_references_to_copies() {
    native(
        r#"
buffer = std.bytes.from_text("abc")?
mut seen = list[u8]()?
for value in &buffer:
    seen.append(*value)?
for value in reversed(&buffer):
    seen.append(*value)?
print(seen)?
mut total = 0
for value in buffer:
    total += i64(value)
print(total)?
"#,
        "[97, 98, 99, 99, 98, 97]\n294",
    );
}

#[test]
fn byte_index_and_iteration_misuse_is_rejected() {
    let check = |body: &str, expected: &str| {
        let error = rejected(&program(body));
        assert!(error.contains(expected), "{body}\n{error}");
    };
    check(
        "mut buffer = std.bytes.from_text(\"abc\")?\nfor value in &mut buffer:\n    pass",
        "mutable iteration over Bytes is not supported; assign through an index with `b[i] = value`",
    );
    check(
        "buffer = std.bytes.from_text(\"abc\")?\nbuffer[0] = 1u8",
        "immutable binding",
    );
    check(
        "mut buffer = std.bytes.from_text(\"abc\")?\nbuffer[0] = 300",
        "out of range for u8",
    );
    check(
        "buffer = std.bytes.from_text(\"abc\")?\nprint(buffer[\"a\"])?",
        "expected i64, got str",
    );
    check(
        "mut buffer = std.bytes.from_text(\"abc\")?\nfor value in &buffer:\n    buffer[0] = 1u8",
        "conflicting borrow",
    );
}

#[test]
fn fill_writes_a_validated_range_natively() {
    native(
        r#"
mut buffer = std.bytes.from_text("abcdef")?
buffer.fill(1, 4, 46u8)?
buffer.fill(6, 6, 0u8)?
print(buffer.to_text()?)?
print(str.repr(buffer.fill(4, 7, 0u8))?)?
print(str.repr(buffer.fill(3, 2, 0u8))?)?
"#,
        &format!("a...ef\nResult[(), {OUT_OF_RANGE}\nResult[(), {OUT_OF_RANGE}"),
    );
}

#[test]
fn transport_still_uses_list_u8_through_explicit_conversions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("payload.bin");
    let body = format!(
        r#"
path = encode_utf8("{}")?
mut payload = std.bytes.from_list(&[0u8, 255u8, 10u8]?)?
payload.extend(&std.bytes.from_text("é")?)?
write_bytes(&path, &payload.to_list()?)?
stored = std.bytes.from_list(&read_bytes(&path)?)?
print(stored == payload, stored.len())?
print(decode_utf8(&stored.slice(3, 5)?.to_list()?)?)?
encoded = std.bytes.from_list(&encode_utf8("naïve")?)?
print(encoded == std.bytes.from_text("naïve")?)?
"#,
        path.display()
    );
    native(&body, "True 5\né\nTrue");
    assert_eq!(std::fs::read(path).unwrap(), [0, 255, 10, 0xc3, 0xa9]);
}

#[test]
fn buffers_move_across_threads_channels_and_collections() {
    let source = format!(
        "{HEADER}{}",
        r#"
def work(data: Bytes) -> ():
    print(data.len(), data.get(3).unwrap()).unwrap()

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | ChannelError | SendError[std.bytes.Bytes] | RecvError | ThreadError]:
    mut buffer = std.bytes.from_text("abc")?
    buffer.push(200u8)?
    sender, receiver = channel[Bytes](1)?
    sender.send(buffer.copy()?)?
    received = receiver.recv()?
    print(received == buffer)?
    with spawn(work, buffer)?:
        pass
    items = [std.bytes.empty()?, std.bytes.from_text("x")?]?
    print(len(items), items[1].len())?
    Ok(())
"#
    );
    assert_eq!(output(&source), "True\n4 200\n2 1");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn in_place_operations_within_capacity_do_not_allocate() {
    native(
        r#"
mut buffer = std.bytes.with_capacity(32)?
source = std.bytes.from_text("abcd")?
buffer.extend(&source)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
buffer.push(1u8)?
buffer.extend(&source)?
buffer.extend_range(&source, 1, 3)?
buffer.resize(20, 0u8)?
buffer.overwrite(12, &source, 0, 4)?
buffer.fill(16, 20, 7u8)?
buffer.set(0, 65u8)?
buffer.truncate(18)?
first = buffer.get(0)?
last = buffer.pop()
found = buffer.find(100u8, 0, buffer.len())?
same = buffer == source
count = buffer.len()
room = buffer.capacity()
buffer.clear()
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(first, last, found, same, count, room, buffer.capacity())?
"#,
        "65 Option[u8].Some(7) Option[i64].Some(3) False 17 32 32",
    );
}

// Issue #298: float spellings were built in Rust `String`s, so a refused
// allocation aborted the program.
#[cfg(feature = "runtime-checks")]
#[test]
fn float_spellings_do_not_allocate() {
    native(
        r#"
mut buffer = std.bytes.with_capacity(4500)?
buffer.resize(4500, 0u8)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
fixed = buffer.write_float_spec(-1.7976931348623157e308, 17, 4096, 0)?
exponent = buffer.write_float_spec(1234.5, 2, 2, 0)?
general = buffer.write_float_spec(0.00001, 3, 6, 0)?
point = buffer.write_float_spec(12.0, 4, 3, 0)?
display = buffer.write_float_spec(0.1, 5, 0, 0)?
single = buffer.write_float_spec(0.1, 6, 0, 0)?
short = buffer.write_float_spec(1.5, 1, 4096, 4490)?
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(fixed, exponent, general, point, display, single, short)?
"#,
        "4407 8 5 4 3 3 -8588",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_and_leaves_buffers_unchanged() {
    let failure =
        "std.bytes.BytesError].Err(std.bytes.BytesError.Allocation(AllocError.OutOfMemory))";
    let expected = format!(
        "Result[(), {failure}\n\
         Result[(), {failure}\n\
         Result[(), {failure}\n\
         Result[(), {failure}\n\
         Result[std.bytes.Bytes, {failure}\n\
         Result[std.bytes.Bytes, {failure}\n\
         Result[list[u8], {failure}\n\
         Result[str, {failure}\n\
         Result[std.bytes.Bytes, {failure}\n\
         Result[std.bytes.Bytes, {failure}\n\
         Result[std.bytes.Bytes, {failure}\n\
         Result[std.bytes.Bytes, {failure}\n\
         4 4 abcd"
    );
    // Results are bound while injection is active and rendered afterwards,
    // because rendering allocates. Process exit checks that nothing leaked.
    native(
        r#"
mut buffer = std.bytes.with_capacity(4)?
source = std.bytes.from_text("abcd")?
long = std.bytes.from_text("a text too long for inline string storage")?
values = [1u8, 2u8]?
buffer.extend(&source)?
print("__test_fail_allocations_after_0__")?
a = buffer.push(1u8)
b = buffer.extend(&source)
c = buffer.reserve(64)
d = buffer.resize(64, 0u8)
e = buffer.slice(1, 3)
f = buffer.copy()
g = buffer.to_list()
h = long.to_text()
i = std.bytes.with_capacity(8)
j = std.bytes.empty()
k = std.bytes.from_list(&values)
l = std.bytes.from_text("text")
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
print(str.repr(j)?)?
print(str.repr(k)?)?
print(str.repr(l)?)?
print(buffer.len(), buffer.capacity(), buffer.to_text()?)?
"#,
        &expected,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn second_allocation_failure_releases_the_first() {
    // Storage takes two allocations (buffer, then owner); `to_text` and
    // `to_list` likewise build in steps. Failing each later step must free
    // the earlier ones, which the exit-time balance check verifies.
    let failure =
        "std.bytes.BytesError].Err(std.bytes.BytesError.Allocation(AllocError.OutOfMemory))";
    let expected = format!(
        "Result[std.bytes.Bytes, {failure}\n\
         Result[std.bytes.Bytes, {failure}\n\
         Result[std.bytes.Bytes, {failure}\n\
         Result[list[u8], {failure}\n\
         Result[std.bytes.Bytes, {failure}"
    );
    native(
        r#"
buffer = std.bytes.from_text("abcd")?
values = [1u8, 2u8]?
print("__test_fail_allocations_after_1__")?
a = std.bytes.with_capacity(8)
print("__test_fail_allocations_after_1__")?
b = buffer.slice(0, 4)
print("__test_fail_allocations_after_1__")?
c = std.bytes.from_list(&values)
print("__test_fail_allocations_after_1__")?
d = buffer.to_list()
print("__test_fail_allocations_after_1__")?
e = std.bytes.from_text("text")
print("__test_restore_allocations__")?
print(str.repr(a)?)?
print(str.repr(b)?)?
print(str.repr(c)?)?
print(str.repr(d)?)?
print(str.repr(e)?)?
"#,
        &expected,
    );
}

#[test]
fn impossible_sizes_are_allocation_errors_and_change_nothing() {
    let error = "std.bytes.BytesError].Err(std.bytes.BytesError.Allocation(AllocError";
    // One byte short of the address-space limit is a refused request; a total
    // beyond it cannot be expressed as a size at all.
    native(
        r#"
mut buffer = std.bytes.from_text("ab")?
room = buffer.capacity()
print(str.repr(std.bytes.with_capacity(9223372036854775807))?)?
print(str.repr(buffer.resize(9223372036854775807, 0u8))?)?
print(str.repr(buffer.reserve(9223372036854775807))?)?
print(buffer.len(), buffer.capacity() == room, buffer.to_text()?)?
"#,
        &format!(
            "Result[std.bytes.Bytes, {error}.OutOfMemory))\n\
             Result[(), {error}.OutOfMemory))\n\
             Result[(), {error}.CapacityOverflow))\n\
             2 True ab"
        ),
    );
}

#[test]
fn transport_moves_one_checked_range_per_call() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    let body = format!(
        r#"
mut file = open("{}", "w+")?
source = std.bytes.from_list(&[0u8, 255u8, 13u8, 10u8, 65u8]?)?
print(source.write_file(&mut file, 1, 5)?, source.write_file(&mut file, 2, 2)?)?
file.seek(0)?
mut buffer = std.bytes.with_capacity(8)?
buffer.resize(8, 46u8)?
print(buffer.read_file(&mut file, 2, 8)?, buffer.to_list()?, buffer.capacity())?
print(buffer.read_file(&mut file, 0, 8)?, buffer.read_file(&mut file, 3, 3)?)?
print(str.repr(buffer.read_file(&mut file, 0, 9))?)?
print(str.repr(source.write_file(&mut file, 3, 2))?)?
print(str.repr(buffer.read_stdin(-1, 2))?, str.repr(source.write_stdout(0, 6))?)?
print(str.repr(source.write_stderr(4, 3))?, buffer.read_stdin(0, 8)?)?
print(source.write_stdout(4, 5)?)?
file.close()?
print(str.repr(buffer.read_file(&mut file, 0, 8))?)?
print(str.repr(source.write_file(&mut file, 0, 5))?)?
"#,
        path.display()
    );
    let invalid = "Result[i64, IoError].Err(IoError.InvalidInput)";
    let closed = "Result[i64, IoError].Err(IoError.Closed)";
    native(
        &body,
        &format!(
            "4 0\n4 [46, 46, 255, 13, 10, 65, 46, 46] 8\n0 0\n{invalid}\n{invalid}\n\
             {invalid} {invalid}\n{invalid} 0\nA1\n{closed}\n{closed}"
        ),
    );
    assert_eq!(std::fs::read(path).unwrap(), [255, 13, 10, 65]);
}

#[test]
fn scans_find_bytes_sets_and_subsequences_at_absolute_offsets() {
    native(
        r#"
data = std.bytes.from_text("key = \"a,b\"; next\r\nrest")?
count = data.len()
print(str.repr(data.find(61u8, 0, count)?)?, str.repr(data.find(61u8, 5, count)?)?)?
print(str.repr(data.find(61u8, 0, 4)?)?, str.repr(data.find(116u8, 16, count)?)?)?
stops = std.bytes.from_text(",;\"")?
print(str.repr(data.find_in(&stops, 0, count)?)?, str.repr(data.find_in(&stops, 7, count)?)?)?
print(str.repr(data.find_in(&stops, 12, count)?)?, str.repr(data.find_in(&stops, 0, 6)?)?)?
blank = std.bytes.from_text(" \r\n")?
print(str.repr(data.find_not_in(&blank, 3, count)?)?, str.repr(data.find_not_in(&blank, 18, count)?)?)?
print(str.repr(data.find_not_in(&blank, 17, 19)?)?)?
none = std.bytes.empty()?
print(str.repr(data.find_in(&none, 0, count)?)?, str.repr(data.find_not_in(&none, 4, count)?)?)?
print(str.repr(data.find_in(&data, 9, count)?)?, str.repr(data.find_not_in(&data, 0, count)?)?)?
line = std.bytes.from_text("\r\n")?
print(str.repr(data.find_bytes(&line, 0, count)?)?, str.repr(data.find_bytes(&line, 0, 18)?)?)?
print(str.repr(data.find_bytes(&none, 7, count)?)?, str.repr(data.find_bytes(&data, 0, count)?)?)?
print(str.repr(none.find_bytes(&none, 0, 0)?)?, str.repr(none.find_bytes(&line, 0, 0)?)?)?
"#,
        "Option[i64].Some(4) Option[i64].Nothing\n\
         Option[i64].Nothing Option[i64].Some(16)\n\
         Option[i64].Some(6) Option[i64].Some(8)\n\
         Option[i64].Nothing Option[i64].Nothing\n\
         Option[i64].Some(4) Option[i64].Some(19)\n\
         Option[i64].Nothing\n\
         Option[i64].Nothing Option[i64].Some(4)\n\
         Option[i64].Some(9) Option[i64].Nothing\n\
         Option[i64].Some(17) Option[i64].Nothing\n\
         Option[i64].Some(7) Option[i64].Some(0)\n\
         Option[i64].Some(0) Option[i64].Nothing",
    );
}

#[test]
fn scans_agree_with_a_byte_loop_on_every_range_of_a_long_buffer() {
    // Longer than the native eight-byte step, with every byte value present.
    native(
        r#"
mut data = std.bytes.with_capacity(300)?
for n in range(300):
    data.push(u8((n * 7) % 256))?
set = std.bytes.from_list(&[0u8, 255u8, 128u8, 44u8, 45u8]?)?
mut wrong = 0
for start in range(0, 300, 13):
    for stop in range(start, 300, 17):
        mut first = -1
        mut inside = -1
        mut outside = -1
        mut total = 0
        for index in range(start, stop):
            value = data.get(index)?
            member = value == 0u8 or value == 255u8 or value == 128u8 or value == 44u8 or value == 45u8
            if value == 44u8:
                total += 1
                if first < 0:
                    first = index
            if member and inside < 0:
                inside = index
            if not member and outside < 0:
                outside = index
        match data.find(44u8, start, stop)?:
            case Some(index):
                if index != first:
                    wrong += 1
            case Nothing:
                if first >= 0:
                    wrong += 1
        match data.find_in(&set, start, stop)?:
            case Some(index):
                if index != inside:
                    wrong += 1
            case Nothing:
                if inside >= 0:
                    wrong += 1
        match data.find_not_in(&set, start, stop)?:
            case Some(index):
                if index != outside:
                    wrong += 1
            case Nothing:
                if outside >= 0:
                    wrong += 1
        if data.count(44u8, start, stop)? != total:
            wrong += 1
print(wrong, data.count(44u8, 0, 300)?, data.count(1u8, 0, 0)?)?
"#,
        "0 1 0",
    );
}

#[test]
fn ranges_are_compared_with_bytes_and_text_without_copying() {
    native(
        r#"
data = std.bytes.from_text("true,false,né")?
word = std.bytes.from_text("xfalsey")?
print(data.matches(5, &word, 1, 6)?, data.matches(4, &word, 1, 6)?, data.matches(5, &word, 1, 7)?)?
print(data.matches(0, &word, 3, 3)?, data.matches(data.len(), &word, 3, 3)?)?
print(data.matches(11, &word, 1, 6)?, data.matches(data.len(), &word, 1, 2)?, data.matches(0, &data, 0, data.len())?)?
print(data.matches_text(0, "true")?, data.matches_text(0, "true,")?, data.matches_text(1, "true")?)?
print(data.matches_text(11, "né")?, data.matches_text(11, "néx")?, data.matches_text(data.len(), "")?)?
print(data.matches_text(data.len(), "x")?)?
"#,
        "True False False\nTrue True\nFalse False True\nTrue True False\nTrue False True\nFalse",
    );
}

#[test]
fn runs_stop_at_the_first_non_ascii_or_invalid_byte() {
    native(
        r#"
mut data = std.bytes.from_text("plain text, then né and 日本")?
count = data.len()
print(count, data.ascii_run(0, count)?, data.ascii_run(18, count)?, data.ascii_run(19, count)?)?
print(data.ascii_run(5, 9)?, data.ascii_run(count, count)?)?
print(data.utf8_run(0, count)?, data.utf8_run(0, 18)?, data.utf8_run(0, 19)?, data.utf8_run(18, count)?)?
print(data.utf8_run(19, count)?, data.utf8_run(0, count - 1)?)?
data.set(3, 255u8)?
print(data.utf8_run(0, count)?, data.utf8_run(4, count)?, data.ascii_run(0, count)?)?
"#,
        "31 18 18 19\n9 31\n31 18 18 31\n19 28\n3 31 3",
    );
}

#[test]
fn scans_reject_ranges_outside_the_buffer() {
    native(
        r#"
data = std.bytes.from_text("abcd")?
set = std.bytes.from_text("b")?
print(str.repr(data.find(98u8, -1, 2))?, str.repr(data.find(98u8, 3, 2))?)?
print(str.repr(data.find_in(&set, 0, 5))?, str.repr(data.find_not_in(&set, 5, 5))?)?
print(str.repr(data.find_bytes(&set, 2, 1))?)?
print(str.repr(data.count(98u8, 0, 5))?, str.repr(data.ascii_run(-1, 0))?, str.repr(data.utf8_run(4, 5))?)?
print(str.repr(data.matches(-1, &set, 0, 1))?, str.repr(data.matches(5, &set, 0, 0))?)?
print(str.repr(data.matches(0, &set, 0, 2))?, str.repr(data.matches(0, &set, 1, 0))?)?
print(str.repr(data.matches_text(-1, "a"))?, str.repr(data.matches_text(5, ""))?)?
"#,
        &{
            let found = format!("Result[Option[i64], {OUT_OF_RANGE}");
            let counted = format!("Result[i64, {OUT_OF_RANGE}");
            let matched = format!("Result[bool, {OUT_OF_RANGE}");
            format!(
                "{found} {found}\n{found} {found}\n{found}\n{counted} {counted} {counted}\n\
                 {matched} {matched}\n{matched} {matched}\n{matched} {matched}"
            )
        },
    );
}
