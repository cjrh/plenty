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

/// Constructor and default allocation failures use the decoder's allocation
/// domain; `AllocError | Fault` routes each member to its own mapping (#905).
const ALLOCATING: &str = r#"
import std.json
import std.toml
import std.serial
from std.serial import Fault

@serial
class AgentConfig:
    command: str
    args: list[str]
    env: dict[str, str]
    def __init__(self, command: str, args: list[str] = []?, env: dict[str, str] = {}?) -> Result[(), AllocError]:
        self.command = command
        self.args = args
        self.env = env
        Ok(())

def first_port() -> Result[i64, Fault]:
    Err(Fault.OutOfRange)

type Checked = Result[(), Fault | AllocError]

@serial
class Service:
    name: str
    @serial.name("listen-port")
    port: i64
    tags: list[str]
    def __init__(self, name: str, port: i64 = first_port()?, tags: list[str] = ["default tag"]?) -> Checked:
        if port < 1:
            return Err(Fault.OutOfRange)
        self.name = name
        self.port = port
        self.tags = tags
        self.tags.append("decoded")?
        Ok(())

def default_port() -> Result[i64, AllocError]:
    Ok(8)

# The domain mentions a type parameter, so its bridge is chosen when `E` is known.
@serial
class GenericService[E]:
    name: str
    @serial.name("listen-port")
    port: i64
    tags: list[str]
    def __init__(self, name: str, port: i64 = default_port()?, tags: list[str] = ["default tag"]?) -> Result[(), AllocError | E]:
        if port < 1:
            return Err(Fault.OutOfRange)
        self.name = name
        self.port = port
        self.tags = tags
        self.tags.append("decoded")?
        Ok(())
"#;

#[test]
fn allocating_constructors_and_defaults_decode_without_custom_hooks() {
    support::assert_stdout(
        &format!(
            r#"{ALLOCATING}
def main() -> Result[(), IoError | AllocError | std.serial.SerialError[std.json.JsonError] | std.serial.SerialError[std.toml.TomlError]]:
    options = std.json.codec_options()
    mut absent = std.json.load[AgentConfig](b"{{\"command\":\"claude\"}}", &options)?
    other = std.json.load[AgentConfig](b"{{\"command\":\"codex\"}}", &options)?
    absent.args.append("-p")?
    print(absent.args, other.args, other.env)?
    present = std.json.load[AgentConfig](b"{{\"command\":\"claude\",\"args\":[\"-p\"],\"env\":{{\"A\":\"b\"}}}}", &options)?
    print(present.args, present.env.get("A"))?
    toml = std.toml.codec_options()
    text = std.toml.dump(&present, &toml)?
    print(std.toml.load[AgentConfig](&text, &toml)?.env.get("A"))?
    print(std.toml.load[AgentConfig]("command = \"claude\"\n", &toml)?.args)?
    service = std.json.load[Service](b"{{\"name\":\"web\",\"listen-port\":8}}", &options)?
    print(service.port, service.tags)?
    match std.json.load[Service](b"{{\"name\":\"web\"}}", &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
    match std.toml.load[Service]("name = \"web\"\nlisten-port = 0\n", &toml):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path.is_none())?
    Ok(())
"#
        ),
        "[\"-p\"] [] {}\n[\"-p\"] Option[str].Some(\"b\")\nOption[str].Some(\"b\")\n[]\n8 [\"default tag\", \"decoded\"]\nstd.json.JsonFault.Serialization(std.serial.Fault.OutOfRange) [std.serial.Step.Field(\"listen-port\")]\nstd.toml.TomlFault.Serialization(std.serial.Fault.OutOfRange) True\n",
    );
}

#[test]
fn other_constructor_error_domains_still_request_custom_decode() {
    for domain in [
        "IoError",
        "AllocError | IoError",
        "IoError | std.serial.Fault",
    ] {
        let error = support::check_error(&format!(
            r#"
import std.json
import std.serial
@serial
class Config:
    n: i64
    def __init__(self, n: i64) -> Result[(), {domain}]:
        self.n = n
        Ok(())

def main() -> ():
    drop(std.json.load[Config](b"{{\"n\":2}}", &std.json.codec_options()).unwrap())
"#
        ));
        assert!(
            error.contains("AllocError | std.serial.Fault")
                && error.contains("write a private __decode__"),
            "{domain}: {error}"
        );
    }
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
        "std.json.JsonFault.Serialization(std.serial.Fault.WrongLength(1, 0))\nstd.json.JsonFault.Serialization(std.serial.Fault.UnknownVariant(\"Unknown\"))\nstd.json.JsonFault.Serialization(std.serial.Fault.WrongLength(2, 1))\nstd.json.JsonFault.Serialization(std.serial.Fault.WrongLength(1, 2))\nstd.json.JsonFault.Serialization(std.serial.Fault.UnexpectedKind)\nstd.json.JsonFault.Serialization(std.serial.Fault.OutOfRange)\n",
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

/// Every injected allocation failure, in a default, a constructor body or the
/// adapter, reports the format's allocation fault; under `--all-features` the
/// allocation-checks runtime also checks that partial owners are released.
#[cfg(feature = "runtime-checks")]
#[test]
fn allocating_constructor_failures_report_allocation_and_clean_up() {
    support::assert_filtered_stdout(
        &format!(
            r#"{ALLOCATING}
from std.json import JsonError, JsonFault
from std.toml import TomlError, TomlFault
from std.serial import SerialError

# 0 success, 1 allocation fault, 2 validation fault, 3 anything else.
def json_outcome[T](result: Result[T, SerialError[JsonError]]) -> i64:
    match result:
        case Ok(_):
            0
        case Err(error):
            match error.cause.fault:
                case JsonFault.Allocation(_):
                    1
                case JsonFault.Serialization(Fault.OutOfRange):
                    2
                case _:
                    3

def toml_outcome[T](result: Result[T, SerialError[TomlError]]) -> i64:
    match result:
        case Ok(_):
            0
        case Err(error):
            match error.cause.fault:
                case TomlFault.Allocation(_):
                    1
                case TomlFault.Serialization(Fault.OutOfRange):
                    2
                case _:
                    3

def attempt(which: i64) -> i64:
    options = std.json.codec_options()
    toml = std.toml.codec_options()
    if which == 0:
        json_outcome(std.json.load[AgentConfig](b"{{\"command\":\"a command longer than inline storage\"}}", &options))
    elif which == 1:
        json_outcome(std.json.load[AgentConfig](b"{{\"command\":\"claude\",\"args\":[\"an argument longer than inline storage\"],\"env\":{{\"KEY\":\"a value longer than inline storage\"}}}}", &options))
    elif which == 2:
        json_outcome(std.json.load[Service](b"{{\"name\":\"a service name longer than inline storage\",\"listen-port\":8}}", &options))
    elif which == 3:
        json_outcome(std.json.load[Service](b"{{\"name\":\"a service name longer than inline storage\",\"listen-port\":0}}", &options))
    elif which == 4:
        json_outcome(std.json.load[list[AgentConfig]](b"[{{\"command\":\"a command longer than inline storage\"}},{{\"command\":\"another command longer than inline storage\"}}]", &options))
    elif which == 5:
        toml_outcome(std.toml.load[AgentConfig]("command = \"a command longer than inline storage\"\n", &toml))
    elif which == 6:
        toml_outcome(std.toml.load[Service]("name = \"a service name longer than inline storage\"\nlisten-port = 8\n", &toml))
    elif which == 7:
        toml_outcome(std.toml.load[Service]("name = \"a service name longer than inline storage\"\nlisten-port = 0\n", &toml))
    elif which == 8:
        json_outcome(std.json.load[GenericService[Fault]](b"{{\"name\":\"a service name longer than inline storage\"}}", &options))
    else:
        json_outcome(std.json.load[GenericService[Fault]](b"{{\"name\":\"a service name longer than inline storage\",\"listen-port\":0}}", &options))

def main() -> Result[(), IoError | AllocError]:
    mut markers = list[str]()?
    for count in range(128):
        markers.append("".join(&["__test_fail_allocations_after_", str.from(count)?, "__"]?)?)?
    for which in range(10):
        mut seen = [False, False, False, False]?
        for count in range(128):
            print(markers[count])?
            outcome = attempt(which)
            print("__test_restore_allocations__")?
            seen[outcome] = True
        print(seen[1], seen[0], seen[2], seen[3])?
    Ok(())
"#
        ),
        "True True False False\nTrue True False False\nTrue True False False\nTrue False True False\nTrue True False False\nTrue True False False\nTrue True False False\nTrue False True False\nTrue True False False\nTrue False True False",
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

/// A constructor domain that mentions a class type parameter is mapped after
/// specialization, like a concrete one (#914).
#[test]
fn generic_constructor_error_domain_is_mapped_at_specialization() {
    support::assert_stdout(
        &format!(
            r#"{ALLOCATING}
@serial
class Config[E]:
    value: i64
    def __init__(self, value: i64 = 7) -> Result[(), E]:
        self.value = value
        Ok(())

def main() -> Result[(), IoError | AllocError | std.serial.SerialError[std.json.JsonError] | std.serial.SerialError[std.toml.TomlError]]:
    options = std.json.codec_options()
    print(std.json.load[Config[Fault]](b"{{}}", &options)?.value)?
    print(std.json.load[Config[AllocError]](b"{{\"value\":2}}", &options)?.value)?
    print(std.json.load[Config[AllocError | Fault]](b"{{}}", &options)?.value)?
    print(std.json.load[Config[Fault | AllocError]](b"{{\"value\":3}}", &options)?.value)?
    service = std.json.load[GenericService[Fault]](b"{{\"name\":\"web\"}}", &options)?
    print(service.port, service.tags)?
    match std.json.load[GenericService[Fault]](b"{{\"name\":\"web\",\"listen-port\":0}}", &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path.is_none())?
    toml = std.toml.codec_options()
    print(std.toml.load[GenericService[Fault]]("name = \"web\"\n", &toml)?.tags)?
    Ok(())
"#
        ),
        "7\n2\n7\n3\n8 [\"default tag\", \"decoded\"]\nstd.json.JsonFault.Serialization(std.serial.Fault.OutOfRange) True\n[\"default tag\", \"decoded\"]\n",
    );
    for domain in ["IoError", "AllocError | IoError", "i64"] {
        let error = support::check_error(&format!(
            r#"
import std.json
@serial
class Config[E]:
    value: i64
    def __init__(self, value: i64) -> Result[(), E]:
        self.value = value
        Ok(())
def main() -> ():
    drop(std.json.load[Config[{domain}]](b"{{}}", &std.json.codec_options()))
"#
        ));
        assert!(
            error.contains("private __decode__")
                && error.contains(
                    "where E is std.serial.Fault, AllocError or AllocError | std.serial.Fault"
                ),
            "{domain}: {error}"
        );
    }
}

/// `mentions` recurses into unions of several class parameters; equal
/// parameters collapse to one domain (#914).
#[test]
fn generic_constructor_domain_of_two_parameters() {
    support::assert_stdout(
        r#"
import std.json
import std.serial
from std.serial import Fault
@serial
class Pair[A, B]:
    value: i64
    def __init__(self, value: i64 = 5) -> Result[(), A | B]:
        self.value = value
        Ok(())
def main() -> Result[(), IoError | AllocError | std.serial.SerialError[std.json.JsonError]]:
    options = std.json.codec_options()
    print(std.json.load[Pair[AllocError, Fault]](b"{}", &options)?.value)?
    print(std.json.load[Pair[Fault, Fault]](b"{\"value\":6}", &options)?.value)?
    Ok(())
"#,
        "5\n6\n",
    );
}

/// The written domain and fallible defaults resolve in the defining module,
/// not the module that specializes the class (#914).
#[test]
fn imported_generic_constructor_domain_resolves_in_defining_module() {
    let dir = tempfile::tempdir().unwrap();
    support::write_file(
        &dir.path().join("model.plenty"),
        r#"
def default_tags() -> Result[list[str], AllocError]:
    ["remote"]

@serial
pub class Remote[E]:
    pub tags: list[str]
    def __init__(self, tags: list[str] = default_tags()?) -> Result[(), AllocError | E]:
        self.tags = tags
        Ok(())
"#,
    );
    let executable = support::compile_file(
        dir.path(),
        r#"
import std.json
import std.serial
from std.serial import Fault
from model import Remote
def main() -> Result[(), IoError | AllocError | std.serial.SerialError[std.json.JsonError]]:
    options = std.json.codec_options()
    print(std.json.load[Remote[Fault]](b"{}", &options)?.tags)?
    print(std.json.load[Remote[AllocError]](b"{\"tags\":[\"x\"]}", &options)?.tags)?
    Ok(())
"#,
    );
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "[\"remote\"]\n[\"x\"]\n"
    );
}

#[test]
fn wide_recursive_decode_reports_native_stack_limit_before_overflow() {
    for (inline_payload, depth, limit) in [(false, 1026, 1024), (true, 66, 64)] {
        let mut source = String::from("import std.json\nimport std.serial\n");
        if inline_payload {
            source.push_str("@serial\nclass Payload:\n");
            for i in 0..64 {
                source.push_str(&format!("    p{i}: i64\n"));
            }
        }
        source.push_str("@serial\nclass Node:\n    next: Option[Box[Node]]\n");
        for i in 0..63 {
            source.push_str(&format!(
                "    n{i}: {}\n",
                if inline_payload { "Payload" } else { "i64" }
            ));
        }
        let input = format!("{}null{}", "{\"next\":".repeat(depth), "}".repeat(depth));
        source.push_str(&format!(
            r#"
def main() -> Result[(), IoError]:
    mut options = std.json.codec_options()
    options.limits.max_depth = {limit}
    match std.json.load[Node](b{input:?}, &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            match error.cause.fault:
                case std.json.JsonFault.Serialization(std.serial.Fault.StackLimit):
                    print("stack limit", error.complete, len(error.path.unwrap()) > 0)?
                case _:
                    print(error.cause.fault)?
    Ok(())
"#
        ));
        support::assert_stdout(&source, "stack limit True True\n");
    }
}

#[test]
fn serialization_function_values_require_a_direct_wrapper() {
    for entry in [
        "std.serial.decode[i64, std.json.JsonError, std.json.CodecDecoder]",
        "std.serial.encode[i64, std.json.JsonError, std.json.Encoder]",
    ] {
        let error = support::check_error(&format!(
            "import std.serial\nimport std.json\ndef main() -> ():\n    drop({entry})\n"
        ));
        assert!(
            error.contains("direct call") && error.contains("wrapper"),
            "{error}"
        );
    }
}

const WIRE_NAMES: &str = r#"
import std.json
import std.toml
import std.serial
from std.serial import Fault

@serial
enum AgentKind:
    @serial.name("pi")
    Pi
    @serial.name("claude")
    Claude
    Codex
    @serial.name("command")
    Command(str)
    @serial.name("pair")
    Pair(str, u8)

def default_retries() -> i64:
    3

@serial
class Agent:
    @serial.name("agentKind")
    kind: AgentKind
    name: str
    @serial.name("max-retries")
    retries: i64
    def __init__(self, kind: AgentKind, name: str, retries: i64 = default_retries()) -> Result[(), Fault]:
        if retries < 0:
            return Err(Fault.OutOfRange)
        self.kind = kind
        self.name = name
        self.retries = retries
        Ok(())

def show[T](result: Result[T, std.serial.SerialError[std.json.JsonError]]) -> Result[(), IoError]:
    match result:
        case Ok(_):
            print("unexpected")
        case Err(error):
            if error.path.is_none():
                return print(error.cause.fault, "no path")
            print(error.cause.fault, error.path.unwrap())
"#;

#[test]
fn wire_names_roundtrip_json_and_toml_and_keep_constructor_matching() {
    support::assert_stdout(
        &format!(
            r#"{WIRE_NAMES}
def main() -> Result[(), IoError | AllocError]:
    options = std.json.codec_options()
    values = [Agent(AgentKind.Pi, "a", retries=1).unwrap(), Agent(AgentKind.Command("run"), "b").unwrap(), Agent(AgentKind.Pair("x", 4u8), "c").unwrap(), Agent(AgentKind.Codex, "d").unwrap()]?
    bytes = std.json.dump(&values, &std.json.limits()).unwrap()
    print(bytes.to_text().unwrap())?
    print(std.json.load[list[Agent]](&bytes, &options).unwrap() == values)?
    stored = std.json.load[list[AgentKind]](b"[\"pi\",\"claude\",\"Codex\",{{\"command\":\"go\"}}]", &options).unwrap()
    print(stored == [AgentKind.Pi, AgentKind.Claude, AgentKind.Codex, AgentKind.Command("go")]?)?
    defaulted = std.json.load[Agent](b"{{\"name\":\"e\",\"agentKind\":\"claude\"}}", &options).unwrap()
    print(defaulted.retries)?
    toml = std.toml.codec_options()
    for value in &values:
        text = std.toml.dump(value, &toml).unwrap()
        decoded = std.toml.load[Agent](&text, &toml).unwrap()
        print(&decoded == value)?
    print(std.toml.dump(&values[1], &toml).unwrap())?
    Ok(())
"#
        ),
        concat!(
            "[{\"agentKind\":\"pi\",\"name\":\"a\",\"max-retries\":1},",
            "{\"agentKind\":{\"command\":\"run\"},\"name\":\"b\",\"max-retries\":3},",
            "{\"agentKind\":{\"pair\":[\"x\",4]},\"name\":\"c\",\"max-retries\":3},",
            "{\"agentKind\":\"Codex\",\"name\":\"d\",\"max-retries\":3}]\n",
            "True\nTrue\n3\nTrue\nTrue\nTrue\nTrue\n",
            "name = \"b\"\nmax-retries = 3\n\n[agentKind]\ncommand = \"run\"\n\n",
        ),
    );
}

#[test]
fn wire_names_replace_source_names_in_decoding_and_paths() {
    support::assert_stdout(
        &format!(
            r#"{WIRE_NAMES}
def main() -> Result[(), IoError | AllocError]:
    options = std.json.codec_options()
    show(std.json.load[Agent](b"{{\"kind\":\"pi\",\"name\":\"a\"}}", &options))?
    show(std.json.load[Agent](b"{{\"name\":\"a\"}}", &options))?
    show(std.json.load[Agent](b"{{\"agentKind\":\"Pi\",\"name\":\"a\"}}", &options))?
    show(std.json.load[Agent](b"{{\"agentKind\":{{\"Command\":\"x\"}},\"name\":\"a\"}}", &options))?
    show(std.json.load[Agent](b"{{\"agentKind\":{{\"pair\":[\"x\",256]}},\"name\":\"a\"}}", &options))?
    show(std.json.load[Agent](b"{{\"agentKind\":\"pi\",\"name\":\"a\",\"max-retries\":1,\"max-retries\":2}}", &options))?
    show(std.json.load[Agent](b"{{\"agentKind\":\"pi\",\"name\":\"a\",\"max-retries\":-1}}", &options))?
    mut lenient = std.json.codec_options()
    lenient.unknown_fields = std.serial.UnknownFields.Ignore
    print(std.json.load[Agent](b"{{\"retries\":9,\"agentKind\":\"pi\",\"name\":\"a\"}}", &lenient).unwrap().retries)?
    Ok(())
"#
        ),
        concat!(
            "std.json.JsonFault.Serialization(std.serial.Fault.UnknownField(\"kind\")) [std.serial.Step.Field(\"kind\")]\n",
            "std.json.JsonFault.Serialization(std.serial.Fault.MissingField(\"agentKind\")) [std.serial.Step.Field(\"agentKind\")]\n",
            "std.json.JsonFault.Serialization(std.serial.Fault.UnknownVariant(\"Pi\")) [std.serial.Step.Field(\"agentKind\")]\n",
            "std.json.JsonFault.Serialization(std.serial.Fault.UnknownVariant(\"Command\")) [std.serial.Step.Field(\"agentKind\")]\n",
            "std.json.JsonFault.Serialization(std.serial.Fault.OutOfRange) [std.serial.Step.Index(1), std.serial.Step.Variant(\"pair\"), std.serial.Step.Field(\"agentKind\")]\n",
            "std.json.JsonFault.Serialization(std.serial.Fault.DuplicateField(\"max-retries\")) [std.serial.Step.Field(\"max-retries\")]\n",
            "std.json.JsonFault.Serialization(std.serial.Fault.OutOfRange) no path\n",
            "3\n",
        ),
    );
}

#[test]
fn wire_names_apply_to_the_derived_side_of_a_one_sided_hook() {
    support::assert_stdout(
        r#"
import std.json
import std.serial
@serial
class Port:
    @serial.name("port")
    value: i64
    def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), std.serial.SerialError[E]]:
        # A handwritten side spells the wire name itself.
        std.serial.wrap(out.start_object())?
        std.serial.wrap(out.field("port"))?
        std.serial.wrap(out.integer(self.value))?
        std.serial.wrap(out.end_object())

def main() -> Result[(), IoError]:
    bytes = std.json.dump(&Port(8), &std.json.limits()).unwrap()
    print(bytes.to_text().unwrap())?
    print(std.json.load[Port](&bytes, &std.json.codec_options()).unwrap().value)?
    Ok(())
"#,
        "{\"port\":8}\n8\n",
    );
}

#[test]
fn wire_name_diagnostics() {
    for (source, expected) in [
        (
            "@serial\nclass C:\n    @serial.name(\"b\")\n    a: i64\n    b: i64\n",
            "fields `a` and `b` of `C` both serialize as \"b\"; wire names must be distinct",
        ),
        (
            "@serial\nenum E:\n    @serial.name(\"x\")\n    A\n    @serial.name(\"x\")\n    B\n",
            "variants `A` and `B` of `E` both serialize as \"x\"",
        ),
        (
            "@serial\nclass C:\n    @serial.name(\"a\")\n    a: i64\n",
            "`@serial.name(\"a\")` repeats the field's own name; remove it",
        ),
        (
            "class C:\n    @serial.name(\"x\")\n    a: i64\n",
            "`@serial.name` applies only to fields of an `@serial` class and variants of an `@serial` enum",
        ),
        (
            "enum E:\n    @serial.name(\"x\")\n    A\n",
            "`@serial.name` applies only to fields",
        ),
        (
            "@serial\nclass C:\n    a: i64\n    @serial.name(\"x\")\n    def f(self) -> i64:\n        self.a\n",
            "`@serial.name` applies only to fields",
        ),
        (
            "@serial\nenum E:\n    A\n    @serial.name(\"x\")\n    def f(self) -> i64:\n        1\n",
            "`@serial.name` applies only to fields",
        ),
        (
            "@serial.name(\"x\")\nclass C:\n    a: i64\n",
            "`@serial.name` applies only to fields",
        ),
        (
            "@serial\nclass C:\n    @redacted\n    @serial.name(\"x\")\n    a: i64\n",
            "`@redacted` applies only to class and enum declarations, not class members",
        ),
        (
            "@serial\nenum E:\n    @serial.name(\"x\")\n    @copy\n    A\n",
            "`@copy` applies only to class declarations, not enum members",
        ),
        (
            "@serial\nclass C:\n    a: i64\n    @serial.name(\"x\")\n",
            "`@serial.name` applies only to fields",
        ),
        (
            "@serial\nenum E:\n    A\n    @serial.name(\"x\")\n",
            "`@serial.name` applies only to fields",
        ),
        (
            "@serial\nclass C:\n    a: i64\n    @serial.name(\"x\")\nconst N: i64 = 1\n",
            "`@serial.name` applies only to fields",
        ),
        (
            "@serial\nclass C:\n    @serial.name(\"x\")\n    pub def f(self) -> i64:\n        1\n    a: i64\n",
            "`@serial.name` applies only to fields",
        ),
        (
            "@serial\nclass C:\n    @serial.name(\"x\", \"y\")\n    a: i64\n",
            "`@serial.name` takes one string literal",
        ),
        (
            "@serial\nclass C:\n    @serial.name(\"\")\n    a: i64\n",
            "`@serial.name` needs a nonempty name",
        ),
        (
            "@serial\nclass C:\n    @serial.name(\"x\")\n    @serial.name(\"y\")\n    a: i64\n",
            "duplicate `@serial.name`",
        ),
        (
            "@serial\nclass C:\n    @serial.name(x)\n    a: i64\n",
            "`@serial.name` takes one string literal",
        ),
        (
            "@serial\nclass C:\n    @serial.alias(\"x\")\n    a: i64\n",
            "expected `name` after `@serial.`",
        ),
        (
            "@serial\nclass C:\n    @serial.name(\"x\") a: i64\n",
            "alone on the line above the field or variant",
        ),
        (
            "import std.serial\n@serial\nclass C:\n    @serial.name(\"x\")\n    value: i64\n    def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), std.serial.SerialError[E]]:\n        std.serial.wrap(out.integer(self.value))\n    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[C, std.serial.SerialError[E]]:\n        Ok(C(std.serial.wrap(source.integer())?))\n",
            "`@serial.name` has no effect: `C` writes both `__encode__` and `__decode__`",
        ),
    ] {
        let error = support::check_error(&format!("{source}def main() -> ():\n    pass\n"));
        assert!(error.contains(expected), "{source}\n{error}");
    }
}
