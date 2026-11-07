//! Ordinary checked hooks emitted by @serial.
mod support;

#[test]
fn private_generic_fields_aliases_options_and_hidden_import() {
    support::assert_stdout(
        r#"
import std.json

type Maybe = Option[u16]
@serial
class Hidden[T]:
    source: T
    out: Maybe

@serial
class Empty:
    pass

def main() -> Result[(), AllocError | IoError]:
    options = std.json.codec_options()
    value = std.json.load[Hidden[list[i64]]](b"{\"source\":[1,2]}", &options).unwrap()
    print(std.json.dump(&value, &std.json.limits()).unwrap().to_text().unwrap())?
    print(std.json.dump(&Empty(), &std.json.limits()).unwrap().to_text().unwrap())?
    print(std.json.load[Empty](b"{}", &options).is_ok())?
    Ok(())
"#,
        "{\"source\":[1,2],\"out\":null}\n{}\nTrue\n",
    );
}

const DEFAULTS: &str = r#"
import std.json
import std.serial
from std.serial import Fault

def source() -> Result[i64, Fault]:
    print("b default").unwrap()
    Ok(7)

def another() -> Option[i64]:
    print("a default").unwrap()
    Some(3)

@serial
class Defaults:
    a: Option[i64]
    b: i64
    def __init__(self, b: i64 = source()?, a: Option[i64] = another()) -> Result[(), Fault]:
        if b < 0:
            return Err(Fault.OutOfRange)
        self.a = a
        self.b = b
        Ok(())
"#;

#[test]
fn constructor_defaults_keep_order_scope_and_explicit_nothing() {
    support::assert_stdout(
        &format!(
            r#"{DEFAULTS}
def main() -> Result[(), IoError]:
    options = std.json.codec_options()
    value = std.json.load[Defaults](b"{{}}", &options).unwrap()
    print(value.a, value.b)?
    explicit = std.json.load[Defaults](b"{{\"a\":null,\"b\":11}}", &options).unwrap()
    print(explicit.a, explicit.b)?
    match std.json.load[Defaults](b"{{\"b\":-1,\"a\":null}}", &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault)?
    Ok(())
"#
        ),
        "b default\na default\nOption[i64].Some(3) 7\nOption[i64].Nothing 11\nstd.json.JsonFault.Serialization(std.serial.Fault.OutOfRange)\n",
    );
}

#[test]
fn failed_default_keeps_typed_cause_and_field_path() {
    support::assert_stdout(
        r#"
import std.json
from std.serial import Fault

def source() -> Result[i64, Fault]:
    Err(Fault.OutOfRange)

@serial
class Config:
    value: i64
    def __init__(self, value: i64 = source()?) -> Result[(), Fault]:
        self.value = value
        Ok(())

def main() -> Result[(), IoError]:
    match std.json.load[Config](b"{}", &std.json.codec_options()):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
    Ok(())
"#,
        "std.json.JsonFault.Serialization(std.serial.Fault.OutOfRange) [std.serial.Step.Field(\"value\")]\n",
    );
}

#[test]
fn class_missing_duplicate_unknown_and_nested_paths() {
    support::assert_stdout(
        r#"
import std.json
import std.serial
@serial
class Config:
    port: u8

def main() -> Result[(), IoError | AllocError]:
    mut options = std.json.codec_options()
    for text in [b"{}", b"{\"port\":1,\"port\":2}", b"{\"port\":1,\"other\":[]}", b"{\"port\":256}"]?:
        match std.json.load[Config](&text, &options):
            case Ok(_):
                print("unexpected")?
            case Err(error):
                print(error.cause.fault, error.path.unwrap())?
    options.unknown_fields = std.serial.UnknownFields.Ignore
    print(std.json.load[Config](b"{\"other\":[{}],\"port\":80}", &options).unwrap().port)?
    Ok(())
"#,
        "std.json.JsonFault.Serialization(std.serial.Fault.MissingField(\"port\")) [std.serial.Step.Field(\"port\")]\nstd.json.JsonFault.Serialization(std.serial.Fault.DuplicateField(\"port\")) [std.serial.Step.Field(\"port\")]\nstd.json.JsonFault.Serialization(std.serial.Fault.UnknownField(\"other\")) [std.serial.Step.Field(\"other\")]\nstd.json.JsonFault.Serialization(std.serial.Fault.OutOfRange) [std.serial.Step.Field(\"port\")]\n80\n",
    );
}

const ENUMS: &str = r#"
import std.json
import std.serial
@serial
enum Mode:
    Idle
    Value(i64)
    Pair(str, u8)

@serial
enum Choice[T]:
    Empty
    Item(T)
"#;

#[test]
fn enum_tags_payloads_generic_and_tree_roundtrip() {
    support::assert_stdout(
        &format!(
            r#"{ENUMS}
def main() -> Result[(), IoError | AllocError]:
    options = std.json.codec_options()
    values = [Mode.Idle, Mode.Value(9), Mode.Pair("x", 3u8)]?
    bytes = std.json.dump(&values, &std.json.limits()).unwrap()
    print(bytes.to_text().unwrap())?
    print(std.json.load[list[Mode]](&bytes, &options).unwrap() == values)?
    value = std.json.load[Choice[i64]](b"{{\"Item\":7}}", &options).unwrap()
    tree = std.json.to_value(&value, &std.json.tree_limits()).unwrap()
    print(std.json.from_value[Choice[i64]](tree, &std.json.tree_limits()).unwrap() == value)?
    print(std.json.load[Choice[i64]](b"\"Empty\"", &options).is_ok())?
    Ok(())
"#
        ),
        "[\"Idle\",{{\"Value\":9}},{{\"Pair\":[\"x\",3]}}]\nTrue\nTrue\nTrue\n"
            .replace("{{", "{")
            .replace("}}", "}")
            .as_str(),
    );
}

#[test]
fn enum_rejects_tag_shape_cardinality_and_payload_arity() {
    support::assert_stdout(
        &format!(
            r#"{ENUMS}
def main() -> Result[(), IoError | AllocError]:
    options = std.json.codec_options()
    for text in [b"{{}}", b"\"Unknown\"", b"{{\"Pair\":[\"x\"]}}", b"{{\"Value\":1,\"Value\":2}}", b"{{\"Idle\":null}}", b"{{\"Pair\":[\"x\",256]}}"]?:
        match std.json.load[Mode](&text, &options):
            case Ok(_):
                print("unexpected")?
            case Err(error):
                print(error.cause.fault)?
    Ok(())
"#
        ),
        "std.json.JsonFault.Serialization(std.serial.Fault.UnexpectedKind)\nstd.json.JsonFault.Serialization(std.serial.Fault.UnknownVariant(\"Unknown\"))\nstd.json.JsonFault.Serialization(std.serial.Fault.WrongLength(2, 1))\nstd.json.JsonFault.Serialization(std.serial.Fault.WrongLength(1, 2))\nstd.json.JsonFault.Serialization(std.serial.Fault.UnexpectedKind)\nstd.json.JsonFault.Serialization(std.serial.Fault.OutOfRange)\n",
    );
}

#[test]
fn explicit_hooks_override_each_generated_direction() {
    support::assert_stdout(
        r#"
import std.json
import std.serial
@serial
class Custom:
    value: i64
    def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), std.serial.SerialError[E]]:
        std.serial.wrap(out.integer(self.value))
    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[Custom, std.serial.SerialError[E]]:
        Ok(Custom(std.serial.wrap(source.integer())?))

def main() -> Result[(), IoError]:
    value = std.json.load[Custom](b"4", &std.json.codec_options()).unwrap()
    print(std.json.dump(&value, &std.json.limits()).unwrap().to_text().unwrap())?
    Ok(())
"#,
        "4\n",
    );
}

#[test]
fn wide_record_and_recursive_custom_composition_work_in_worker() {
    let fields = (0..64)
        .map(|i| format!("    f{i}: i64\n"))
        .collect::<String>();
    let args = (0..64).map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    support::assert_stdout(
        &format!(
            r#"
import std.json
@serial
class Wide:
{fields}
@serial
class Node[T]:
    value: T
    children: list[Node[T]]

def work() -> ():
    options = std.json.codec_options()
    original = Wide({args})
    text = std.json.dump(&original, &std.json.limits()).unwrap()
    print(std.json.load[Wide](&text, &options).unwrap().f63).unwrap()
    tree = std.json.load[Node[i64]](b"{{\"value\":1,\"children\":[{{\"value\":2,\"children\":[]}}]}}", &options).unwrap()
    print(tree.children[0].value).unwrap()

def main() -> Result[(), SpawnError[()]]:
    with spawn(work).drop_job()?:
        pass
    Ok(())
"#
        ),
        "63\n2\n",
    );
}

#[test]
fn substitution_names_and_constructor_result_aliases_are_hygienic() {
    support::assert_stdout(
        r#"
import std.json
import std.serial

type Done = ()
type Checked = Result[(), std.serial.Fault]
type Int = i64

@serial
class Aliased[OwnerType, FieldType0, Payload0Type0]:
    source: OwnerType
    value: FieldType0
    slots: Payload0Type0

@serial
class UnitAlias:
    n: Int
    def __init__(self, n: i64 = 4) -> Done:
        self.n = n

@serial
class ResultAlias:
    n: Int
    def __init__(self, n: i64 = 8) -> Checked:
        self.n = n
        Ok(())

def main() -> Result[(), IoError]:
    options = std.json.codec_options()
    value = std.json.load[Aliased[i64,str,bool]](b"{\"source\":1,\"value\":\"s\",\"slots\":true}", &options).unwrap()
    print(value.source, value.value, value.slots)?
    print(std.json.load[UnitAlias](b"{}", &options).unwrap().n)?
    print(std.json.load[ResultAlias](b"{}", &options).unwrap().n)?
    Ok(())
"#,
        "1 s True\n4\n8\n",
    );
}

#[test]
fn unsuitable_constructor_keeps_encode_only_and_requests_custom_decode() {
    let declarations = r#"
import std.json
@serial
class Wider:
    n: i64
    def __init__(self, n: i64 | str) -> ():
        self.n = 3
"#;
    support::assert_stdout(
        &format!(
            r#"{declarations}
def main() -> Result[(), IoError]:
    print(std.json.dump(&Wider(2), &std.json.limits()).unwrap().to_text().unwrap())?
    Ok(())
"#
        ),
        "{\"n\":3}\n",
    );
    let error = support::check_error(&format!(
        r#"{declarations}
def main() -> ():
    drop(std.json.load[Wider](b"{{\"n\":2}}", &std.json.codec_options()).unwrap())
"#
    ));
    assert!(error.contains("write a private __decode__"), "{error}");
}

#[test]
fn derived_toml_optional_fields_and_enum_payloads_roundtrip() {
    support::assert_stdout(
        r#"
import std.toml
@serial
enum Status:
    Idle
    Count(i64)
@serial
class Config:
    status: Status
    optional: Option[i64]

def main() -> Result[(), IoError]:
    options = std.toml.codec_options()
    original = Config(Status.Count(8), Nothing)
    mut text = std.toml.dump(&original, &options).unwrap()
    decoded = std.toml.load[Config](&text, &options).unwrap()
    print(decoded == original)?
    idle = Config(Status.Idle, Some(4))
    text = std.toml.dump(&idle, &options).unwrap()
    print(std.toml.load[Config](&text, &options).unwrap() == idle)?
    Ok(())
"#,
        "True\nTrue\n",
    );
}

#[test]
fn failed_derived_decode_drops_owned_slots_without_constructing_parent() {
    support::assert_stdout(
        r#"
import std.json
@serial
class Child:
    n: i64
    def __del__(self: &mut Child) -> ():
        print("child dropped").unwrap()
@serial
class Parent:
    first: Child
    second: u8
    def __del__(self: &mut Parent) -> ():
        print("parent dropped").unwrap()

def main() -> Result[(), IoError]:
    match std.json.load[Parent](b"{\"first\":{\"n\":1},\"second\":256}", &std.json.codec_options()):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.path.unwrap())?
    Ok(())
"#,
        "child dropped\n[std.serial.Step.Field(\"second\")]\n",
    );
}

#[test]
fn one_sided_overrides_leave_other_direction_derived() {
    support::assert_stdout(
        r#"
import std.json
import std.serial
@serial
class EncodeOnly:
    value: i64
    def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), std.serial.SerialError[E]]:
        std.serial.wrap(out.integer(self.value))
@serial
class DecodeOnly:
    value: i64
    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[DecodeOnly, std.serial.SerialError[E]]:
        Ok(DecodeOnly(std.serial.wrap(source.integer())?))

def main() -> Result[(), IoError]:
    options = std.json.codec_options()
    a = std.json.load[EncodeOnly](b"{\"value\":4}", &options).unwrap()
    print(std.json.dump(&a, &std.json.limits()).unwrap().to_text().unwrap())?
    b = std.json.load[DecodeOnly](b"5", &options).unwrap()
    print(std.json.dump(&b, &std.json.limits()).unwrap().to_text().unwrap())?
    Ok(())
"#,
        "4\n{\"value\":5}\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn derived_allocations_retain_causes_and_clean_partial_owners() {
    support::assert_filtered_stdout(
        r#"
import std.json
from std.bytes import Bytes
from std.json import JsonError
from std.serial import SerialError
@serial
class Batch:
    entries: list[str]
    optional: Option[str]

def attempt(which: i64, value: &Batch, source: &Bytes) -> Result[(), SerialError[JsonError]]:
    if which == 0:
        drop(std.json.dump(value, &std.json.limits())?)
    else:
        drop(std.json.load[Batch](&*source, &std.json.codec_options())?)
    Ok(())

def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    value = Batch(["a string longer than inline storage", "another string longer than inline storage"]?, Some("optional long string also owns storage"))
    source = std.json.dump(&value, &std.json.limits())?
    mut markers = list[str]()?
    for count in range(128):
        markers.append("".join(&["__test_fail_allocations_after_", str.from(count)?, "__"]?)?)?
    for which in range(2):
        mut refused = False
        mut succeeded = False
        mut other = False
        for count in range(128):
            print(markers[count])?
            result = attempt(which, &value, &source)
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
        "True True False\nTrue True False",
    );
}

#[test]
fn enum_payload_errors_keep_variant_and_positional_paths() {
    support::assert_stdout(
        &format!(
            r#"{ENUMS}
def main() -> Result[(), IoError]:
    match std.json.load[list[Mode]](b"[{{\"Pair\":[\"x\",256]}}]", &std.json.codec_options()):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.path.unwrap())?
    match std.json.load[Mode](b"{{\"Value\":1,\"Value\":2}}", &std.json.codec_options()):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.path.unwrap())?
    Ok(())
"#
        ),
        "[std.serial.Step.Index(1), std.serial.Step.Variant(\"Pair\"), std.serial.Step.Index(0)]\n[std.serial.Step.Variant(\"Value\")]\n",
    );
}

#[test]
fn constructor_const_arguments_compare_resolved_values() {
    support::assert_stdout(
        r#"
import std.json
const maximum: i64 = 4 * 4
@serial
class Tag[const N: i64]:
    pass
@serial
class Holder:
    value: Tag[maximum]
    def __init__(self, value: Tag[16]) -> ():
        self.value = value

def main() -> Result[(), IoError]:
    value = std.json.load[Holder](b"{\"value\":{}}", &std.json.codec_options()).unwrap()
    print(std.json.dump(&value, &std.json.limits()).unwrap().to_text().unwrap())?
    Ok(())
"#,
        "{\"value\":{}}\n",
    );
}

#[test]
fn recursive_enum_multi_payload_moves_affine_owners() {
    support::assert_stdout(
        r#"
import std.json
@serial
enum Node[T]:
    Empty
    Pair(T, Box[Node[T]])

def main() -> Result[(), IoError]:
    options = std.json.codec_options()
    value = std.json.load[Node[list[str]]](b"{\"Pair\":[[\"owned string\"],{\"Pair\":[[],\"Empty\"]}]}", &options).unwrap()
    tree = std.json.to_value(&value, &std.json.tree_limits()).unwrap()
    restored = std.json.from_value[Node[list[str]]](tree, &std.json.tree_limits()).unwrap()
    print(std.json.dump(&restored, &std.json.limits()).unwrap().to_text().unwrap())?
    Ok(())
"#,
        "{\"Pair\":[[\"owned string\"],{\"Pair\":[[],\"Empty\"]}]}\n",
    );
}

#[test]
fn unsupported_field_diagnostic_names_the_offending_field() {
    let source = r#"
import std.json
@serial
class Config:
    secret: Result[i64, AllocError]
def main() -> ():
    drop(std.json.dump(&Config(Ok(1)), &std.json.limits()))
"#;
    let error = support::check_error(source);
    assert!(error.contains("Config.secret"), "{error}");
    let error = support::check_error(
        r#"
import std.json
@serial
class Config:
    secret: Result[i64, AllocError]
def main() -> ():
    drop(std.json.load[Config](b"{}", &std.json.codec_options()))
"#,
    );
    assert!(error.contains("Config.secret"), "{error}");
}

#[test]
fn imported_constant_constructor_arguments_use_actual_type_identity() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("model.plenty"),
        r#"
pub const maximum: i64 = 4 * 4
@serial
pub class Tag[const N: i64]:
    pass
"#,
    )
    .unwrap();
    let executable = support::compile_file(
        dir.path(),
        r#"
import std.json
from model import Tag, maximum as limit
@serial
class Holder:
    value: Tag[limit]
    def __init__(self, value: Tag[16]) -> ():
        self.value = value

def main() -> Result[(), IoError]:
    value=std.json.load[Holder](b"{\"value\":{}}", &std.json.codec_options()).unwrap()
    print(std.json.dump(&value, &std.json.limits()).unwrap().to_text().unwrap())?
    Ok(())
"#,
    );
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"{\"value\":{}}\n");
}

#[test]
fn missing_nested_hook_diagnostic_names_the_offending_field() {
    for operation in [
        "drop(std.json.dump(&Config(Plain()), &std.json.limits()))",
        "drop(std.json.load[Config](b\"{}\", &std.json.codec_options()))",
    ] {
        let source = format!(
            "import std.json\nclass Plain:\n    pass\n@serial\nclass Config:\n    secret: Plain\ndef main() -> ():\n    {operation}\n"
        );
        let error = support::check_error(&source);
        assert!(error.contains("Config.secret"), "{error}");
    }
}

#[test]
fn generic_constructor_error_domain_is_checked_at_specialization() {
    support::assert_stdout(
        r#"
import std.json
import std.serial
@serial
class Config[E]:
    value: i64
    def __init__(self, value: i64 = 7) -> Result[(), E]:
        self.value = value
        Ok(())
def main() -> Result[(), AllocError | IoError]:
    options = std.json.codec_options()
    value = std.json.load[Config[std.serial.Fault]](b"{}", &options).unwrap()
    print(value.value)?
    Ok(())
"#,
        "7\n",
    );
    let error = support::check_error(
        r#"
import std.json
@serial
class Config[E]:
    value: i64
    def __init__(self, value: i64) -> Result[(), E]:
        self.value = value
        Ok(())
def main() -> ():
    drop(std.json.load[Config[AllocError]](b"{}", &std.json.codec_options()))
"#,
    );
    assert!(error.contains("private __decode__"), "{error}");
}
