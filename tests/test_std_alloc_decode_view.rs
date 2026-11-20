use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;

#[test]
fn decode_checks_only_the_view_and_retains_independent_selected_text() {
    assert_stdout(
        &format!(
            r#"{POLICY}
import std.bytes as binary
from std.bytes import BytesError

def report(value: Result[str, BytesError]) -> Result[(), IoError]:
    match value:
        case Err(error):
            print(error)
        case Ok(text):
            print(text)

def main() -> Result[(), BytesError | AllocError | IoError]:
    mut frame = binary.from_bytes(b"\xff12345\xc3\xa9!\xfe")?
    none = budget("none", 0, AllocError.CapacityOverflow)
    print(none.decode_text(&frame[1:8])?)?
    report(none.decode_text(&frame[1:7]))?
    report(none.decode_text(&frame[7:9]))?
    report(none.decode_text(&frame[1:9]))?
    selected = budget("field", 1)
    text = selected.decode_text(&frame[1:9])?
    drop(selected)
    frame.clear()
    print(len(none.decode_text(&frame)?))?
    drop(frame)
    print(text)?
    drop(text)
    Ok(())
"#
        ),
        "12345é\nstd.bytes.BytesError.InvalidUtf8\nstd.bytes.BytesError.InvalidUtf8\nrefused none\nstd.bytes.BytesError.Allocation(AllocError.CapacityOverflow)\n0\n12345é!\nfield 0 1\nnone 0 0\n",
    );
}

#[test]
fn lossy_decode_matches_ordinary_grouping_and_allocates_only_final_text() {
    assert_stdout(
        &format!(
            r#"{POLICY}
import std.bytes as binary

def main() -> Result[(), AllocError | IoError]:
    mut frame = binary.from_bytes(b"[ab\xff\xc3\xa9\xe2\x82\xac]")?
    none = budget("none", 0, AllocError.CapacityOverflow)
    print(none.decode_text_lossy(b"")?, none.decode_text_lossy(b"1234567")?)?
    print(none.decode_text_lossy(b"a\xff\xff")?)?
    out = budget("field", 1)
    field = out.decode_text_lossy(&frame[1:8])?
    print(field == frame.to_text_lossy_range(1, 8)?, len(field))?
    match none.decode_text_lossy(&frame[1:8]):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    print(frame.get(3))?
    drop(out)
    frame.clear()
    drop(frame)
    print(field)?
    drop(field)
    Ok(())
"#
        ),
        " 1234567\na��\nTrue 5\nrefused none\nAllocError.CapacityOverflow\n255\nab�é�\nfield 0 1\nnone 0 0\n",
    );
}
