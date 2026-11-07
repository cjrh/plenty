//! Canonical base64 uses the existing encoding module and typed adapter errors.
mod support;

#[test]
fn owning_and_static_bytes_encode_and_owning_bytes_decode() {
    support::assert_stdout(
        r#"
import std.json
import std.bytes
import std.serial
from std.bytes import Bytes
def main() -> Result[(), IoError | AllocError | std.serial.SerialError[std.json.JsonError]]:
    bounds = std.json.limits()
    options = std.json.codec_options()
    for literal in [b"", b"f", b"fo", b"foo", b"\x00\xff"]?:
        owned = std.bytes.from_bytes(&literal)?
        static_text = std.json.dump(&literal, &bounds)?
        owned_text = std.json.dump(&owned, &bounds)?
        decoded = std.json.load[Bytes](&owned_text, &options)?
        print(static_text == owned_text, decoded == owned)?
    tree = std.json.to_value(&b"\xff", &std.json.tree_limits())?
    print(std.json.from_value[Bytes](tree, &std.json.tree_limits())?)?
    Ok(())
"#,
        "True True\nTrue True\nTrue True\nTrue True\nTrue True\nb\"\\xff\"\n",
    );
}

#[test]
fn invalid_padding_and_noncanonical_input_keep_typed_causes_and_path() {
    support::assert_stdout(
        r#"
import std.json
import std.serial
from std.bytes import Bytes
def main() -> Result[(), IoError | AllocError]:
    for input in [b"[\"Zg\"]", b"[\"Zh==\"]", b"[\"Z===\"]", b"[\"____\"]"]?:
        match std.json.load[list[Bytes]](&input, &std.json.codec_options()):
            case Ok(_):
                print("unexpected")?
            case Err(error):
                match error.cause.fault:
                    case std.json.JsonFault.Serialization(std.serial.Fault.InvalidBase64(_)):
                        print(error.path.unwrap(), error.complete)?
                    case _:
                        print("wrong cause")?
    Ok(())
"#,
        "[std.serial.Step.Index(0)] True\n[std.serial.Step.Index(0)] True\n[std.serial.Step.Index(0)] True\n[std.serial.Step.Index(0)] True\n",
    );
}

#[test]
fn static_byte_storage_cannot_be_created_from_dynamic_input() {
    let error = support::check_error(
        "import std.json\ndef main() -> ():\n    drop(std.json.load[bytes](b\"\\\"Zg==\\\"\", &std.json.codec_options()))\n",
    );
    assert!(
        error.contains("static `bytes` literals cannot be decoded at runtime"),
        "{error}"
    );
}
