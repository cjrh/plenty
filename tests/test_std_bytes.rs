//! `std.bytes`: compact owning buffers and checked `&Bytes` range access.

const HEADER: &str = "import std.bytes\nfrom std.bytes import Bytes, BytesError\n";

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}def main() -> Result[(), Failure]:\n{body}    Ok(())\n")
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

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

fn rejected(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    match plenty::check_file(&path, None) {
        Ok(()) => panic!("accepted:\n{source}"),
        Err(error) => error.to_string(),
    }
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

def main() -> Result[(), Failure]:
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
        "cannot read or borrow `buffer` while it is exclusively borrowed",
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
    check(
        "buffer = std.bytes.from_text(\"abc\")?\nprint(buffer[0])?",
        "",
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

def main() -> Result[(), Failure]:
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
