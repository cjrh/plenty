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

#[test]
fn the_same_byte_codec_composes_with_toml_tables() {
    support::assert_stdout(
        r#"
import std.toml
import std.bytes
import std.serial
from std.bytes import Bytes
def main() -> Result[(), IoError | AllocError | std.serial.SerialError[std.toml.TomlError]]:
    value = {"blob": std.bytes.from_bytes(b"\x00\xff")?}?
    options = std.toml.codec_options()
    text = std.toml.dump(&value, &options)?
    print(std.toml.load[dict[str, Bytes]](&text, &options)? == value)?
    match std.toml.load[dict[str, Bytes]]("blob = 'Zh=='", &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            match error.cause.fault:
                case std.toml.TomlFault.Serialization(std.serial.Fault.InvalidBase64(_)):
                    print(error.path.unwrap(), error.complete)?
                case _:
                    print("wrong cause")?
    Ok(())
"#,
        "True\n[std.serial.Step.Key(\"blob\")] True\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn byte_codec_allocation_failures_keep_the_adapter_cause() {
    support::assert_filtered_stdout(
        r#"
import std.json
import std.bytes
import std.serial
from std.bytes import Bytes
from std.json import Json, JsonError
from std.serial import SerialError
def attempt(which: i64, value: &Bytes, source: &Bytes, tree: Json) -> Result[(), SerialError[JsonError]]:
    if which == 0:
        drop(std.json.dump(value, &std.json.limits())?)
    elif which == 1:
        drop(std.json.load[Bytes](&*source, &std.json.codec_options())?)
    elif which == 2:
        drop(std.json.to_value(value, &std.json.tree_limits())?)
    else:
        drop(std.json.from_value[Bytes](tree, &std.json.tree_limits())?)
    Ok(())
def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    value = std.bytes.from_text("an owning byte buffer longer than inline string storage")?
    source = std.json.dump(&value, &std.json.limits())?
    mut markers = list[str]()?
    for count in range(96):
        markers.append("".join(&["__test_fail_allocations_after_", str.from(count)?, "__"]?)?)?
    for which in range(4):
        mut refused = False
        mut succeeded = False
        mut other = False
        for count in range(96):
            tree = std.json.to_value(&value, &std.json.tree_limits())?
            print(markers[count])?
            result = attempt(which, &value, &source, tree)
            print("__test_restore_allocations__")?
            match result:
                case Ok(_):
                    succeeded = True
                case Err(error):
                    match error.cause.fault:
                        case std.json.JsonFault.Allocation(_):
                            refused = True
                        case _:
                            other = True
        print(refused, succeeded, other)?
    Ok(())
"#,
        "True True False\nTrue True False\nTrue True False\nTrue True False",
    );
}
