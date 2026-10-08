mod support;
use support::{check_source, run};

#[test]
fn concrete_generic_enums_construct_match_and_nest() {
    let output = run(r#"
enum Choice[T]:
    Empty
    Value(T)
type ByteChoice = Choice[u8]
enum Envelope:
    Item(Choice[list[i64]])
def main() -> Result[(), Failure]:
    value = ByteChoice.Value(7)?
    match value:
        case Choice[u8].Empty:
            print(0)?
        case Choice[u8].Value(n):
            print(n)?
    nested = Envelope.Item(Choice[list[i64]].Value([2, 4]?)?)?
    print(nested)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "7\nEnvelope.Item(Choice[list[i64]].Value([2, 4]))\n"
    );
}

#[test]
fn generic_enum_instances_remain_nominal_and_owned() {
    for (body, expected) in [
        ("value: Choice[u8] = Choice[i64].Value(1).unwrap()", "expected"),
        ("value: Choice = Choice[u8].Value(1).unwrap()", "requires 1 type arguments"),
        ("value: Choice[&i64] = Choice[&i64].Empty.unwrap()", "cannot contain references"),
        ("value = Choice[list[i64]].Value([1].unwrap()).unwrap()\n    drop(value)\n    drop(value)", "moved"),
    ] {
        let source = format!("enum Choice[T]:\n    Empty\n    Value(T)\ndef main() -> ():\n    {body}\n    pass\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}
