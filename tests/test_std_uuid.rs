//! UUID wire formats and generation policy, including RFC 9562 test vectors.
mod support;

const HEADER: &str = r#"import std.uuid as uuid
import std.bytes
import std.secrets
import std.time
from std.bytes import Bytes, BytesError
from std.secrets import SecretsError
from std.io import StreamError

def uuid_key(value: &uuid.Uuid) -> (u64, u64):
    value.as_u64_pair()

pub class Entropy:
    data: bytes
    at: i64
    calls: i64
    fail: bool

    pub def read(self: &mut Entropy, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        self.calls += 1
        if self.fail:
            return Err(StreamError.Io(IoError.System(5)))
        count = stop - start
        for index in range(count):
            drop(buffer.set(start + index, self.data[self.at + index]))
        self.at += count
        Ok(count)

"#;

fn native(body: &str, expected: &str) {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    let source = format!(
        "{HEADER}def main() -> Result[(), AllocError | IoError | ParseError | BytesError | SecretsError | RangeError]:\n{body}    Ok(())\n"
    );
    let actual = support::run_file_stdout(&source);
    let actual = actual
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(actual, expected.trim_matches('\n'), "{source}");
}

#[test]
fn canonical_text_network_bytes_and_comparison() {
    native(
        r#"
first = uuid.parse("F81D4FAE-7dec-11D0-a765-00A0C91e6bf6")?
print(first.to_string()?)?
raw = first.to_bytes()?
print(raw.to_list()?)?
same = uuid.from_bytes(&raw)?
literal = uuid.from_bytes(&b"\xf8\x1d\x4f\xae\x7d\xec\x11\xd0\xa7\x65\x00\xa0\xc9\x1e\x6b\xf6")?
print(first == same, first == literal, first.compare(&same), first.version())?
(high, low) = first.as_u64_pair()
print(uuid.from_u64_pair(high, low) == first)?
zero = uuid.nil()
last = uuid.parse("ffffffff-ffff-ffff-ffff-ffffffffffff")?
print(zero.is_nil(), zero.to_string()?, last.to_string()?)?
print(zero.compare(&first), first.compare(&zero), zero.is_rfc_variant(), last.is_rfc_variant())?
print(zero.version(), last.version(), first.unix_milliseconds())?
mut ordered = [uuid.from_u64_pair(1u64, 0u64), uuid.from_u64_pair(0u64, u64.MAX), uuid.nil()]?
ordered.sort_by_key(uuid_key)
print(ordered[0].to_string()?, ordered[1].to_string()?, ordered[2].to_string()?)?
"#,
        "f81d4fae-7dec-11d0-a765-00a0c91e6bf6
[248, 29, 79, 174, 125, 236, 17, 208, 167, 101, 0, 160, 201, 30, 107, 246]
True True 0 Option[u8].Some(1)
True
True 00000000-0000-0000-0000-000000000000 ffffffff-ffff-ffff-ffff-ffffffffffff
-1 1 False False
Option[u8].Nothing Option[u8].Nothing Option[u64].Nothing
00000000-0000-0000-0000-000000000000 00000000-0000-0000-ffff-ffffffffffff 00000000-0000-0001-0000-000000000000",
    );
}

#[test]
fn malformed_input_is_rejected_without_normalization() {
    native(
        r#"
for text in ["", "00000000000000000000000000000000", "{00000000-0000-0000-0000-000000000000}", "urn:uuid:00000000-0000-0000-0000-000000000000", "00000000_0000-0000-0000-000000000000", "00000000-0000-0000-0000-00000000000g", " 00000000-0000-0000-0000-000000000000", "00000000-0000-0000-0000-000000000000\n", "00000000-0000-0000-0000-0000000000é", "00000000-0000-0000-0000-00000000000\0"]?:
    match uuid.parse(text):
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error)?
for count in [0, 15, 17]?:
    mut data = std.bytes.empty()?
    data.resize(count, 0u8)?
    match uuid.from_bytes(&data):
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error)?
"#,
        &vec!["ParseError.Invalid"; 13].join("\n"),
    );
}

#[test]
fn rfc_vectors_and_version_variant_masks() {
    native(
        r#"
# RFC 9562 Appendix A.3 and A.6. Version/variant bits of input entropy
# deliberately differ, so accidental preservation cannot pass the test.
mut fourth = std.secrets.source_from(Entropy(b"\x91\x91\x08\xf7\x52\xd1\xf3\x20\x1b\xac\xf8\x47\xdb\x41\x48\xa8", 0, 0, False))
a = uuid.v4_from(&mut fourth)?
print(a.to_string()?, a.version(), a.is_rfc_variant(), fourth.inner.at)?
mut seventh = std.secrets.source_from(Entropy(b"\xfc\xc3\xd8\xc4\xdc\x0c\x0c\x07\x39\x8f", 0, 0, False))
b = uuid.v7_from(&std.time.Timestamp(1645557742, 999999u32), &mut seventh)?
print(b.to_string()?, b.version(), b.unix_milliseconds(), seventh.inner.at)?
mut ones = std.secrets.source_from(Entropy(b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff", 0, 0, False))
print(uuid.v4_from(&mut ones)?.to_string()?)?
mut zeros = std.secrets.source_from(Entropy(b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0", 0, 0, False))
print(uuid.v4_from(&mut zeros)?.to_string()?)?
"#,
        "919108f7-52d1-4320-9bac-f847db4148a8 Option[u8].Some(4) True 16
017f22e2-79b0-7cc3-98c4-dc0c0c07398f Option[u8].Some(7) Option[u64].Some(1645557742000) 10
ffffffff-ffff-4fff-bfff-ffffffffffff
00000000-0000-4000-8000-000000000000",
    );
}

#[test]
fn timestamp_boundaries_validate_before_entropy_and_floor_submilliseconds() {
    native(
        r#"
mut source = std.secrets.source_from(Entropy(b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0", 0, 0, False))
epoch = uuid.v7_from(&std.time.Timestamp(0, 999999u32), &mut source)?
next = uuid.v7_from(&std.time.Timestamp(0, 1000000u32), &mut source)?
last = uuid.v7_from(&std.time.Timestamp(281474976710, 655999999u32), &mut source)?
print(epoch.unix_milliseconds(), next.unix_milliseconds(), last.unix_milliseconds())?
print(epoch.compare(&next), last.to_string()?)?
for at in [std.time.Timestamp(-1, 0u32), std.time.Timestamp(0, 1000000000u32), std.time.Timestamp(281474976710, 656000000u32), std.time.Timestamp(281474976711, 0u32), std.time.Timestamp(i64.MAX, 0u32)]?:
    match uuid.v7_from(at, &mut source):
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error)?
print(source.inner.calls)?
"#,
        "Option[u64].Some(0) Option[u64].Some(1) Option[u64].Some(281474976710655)
-1 ffffffff-ffff-7000-8000-000000000000
RangeError.OutOfRange
RangeError.OutOfRange
RangeError.OutOfRange
RangeError.OutOfRange
RangeError.OutOfRange
3",
    );
}

#[test]
fn entropy_failure_is_preserved() {
    native(
        r#"
mut source = std.secrets.source_from(Entropy(b"", 0, 0, True))
match uuid.v4_from(&mut source):
    case Ok(_):
        print("unexpected success")?
    case Err(error):
        print(error)?
match uuid.v7_from(&std.time.Timestamp(0, 0u32), &mut source):
    case Ok(_):
        print("unexpected success")?
    case Err(error):
        print(error)?
print(source.inner.calls)?
"#,
        "std.secrets.SecretsError.Entropy(std.io.StreamError.Io(IoError.System(5)))
std.secrets.SecretsError.Entropy(std.io.StreamError.Io(IoError.System(5)))
2",
    );
}

#[test]
fn writes_are_bounded_and_do_not_change_surrounding_bytes() {
    native(
        r#"
id = uuid.parse("00112233-4455-6677-8899-aabbccddeeff")?
mut target = std.bytes.empty()?
target.resize(18, 90u8)?
for offset in [-1, 3, i64.MAX]?:
    match id.write_bytes(&mut target, offset):
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error)?
print(target.count(90u8, 0, 18)?)?
id.write_bytes(&mut target, 1)?
print(target.to_list()?)?
"#,
        "std.bytes.BytesError.OutOfRange
std.bytes.BytesError.OutOfRange
std.bytes.BytesError.OutOfRange
18
[90, 0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 90]",
    );
}

#[test]
fn os_generation_has_correct_fields_and_current_timestamp() {
    native(
        r#"
a = uuid.v4()?
b = uuid.v4()?
c = uuid.v7()?
print(a.version(), b.version(), c.version(), a.is_rfc_variant(), c.is_rfc_variant())?
print(uuid.parse(&a.to_string()?)? == a, uuid.from_bytes(&c.to_bytes()?)? == c)?
# Read around generation but allow clock adjustments: correctness is a
# representable epoch millisecond, not a monotonicity promise.
print(c.unix_milliseconds().unwrap() > 0u64)?
"#,
        "Option[u8].Some(4) Option[u8].Some(4) Option[u8].Some(7) True True
True True
True",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_free_operations_and_fallible_outputs() {
    native(
        r#"
mut target = std.bytes.empty()?
target.resize(16, 0u8)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
id = uuid.parse("00112233-4455-6677-8899-aabbccddeeff")?
same = uuid.from_bytes(&b"\x00\x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa\xbb\xcc\xdd\xee\xff")?
id.write_bytes(&mut target, 0)?
comparison = id.compare(&same)
key = id.as_u64_pair()
text = id.to_string()
data = id.to_bytes()
random = uuid.v4()
timed = uuid.v7()
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(comparison, uuid.from_u64_pair(key[0], key[1]) == id)?
match text:
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
match data:
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
match random:
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
match timed:
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
"#,
        "0 True
std.bytes.BytesError.Allocation(AllocError.OutOfMemory)
std.bytes.BytesError.Allocation(AllocError.OutOfMemory)
std.secrets.SecretsError.Allocation(AllocError.OutOfMemory)
std.secrets.SecretsError.Allocation(AllocError.OutOfMemory)",
    );
}
