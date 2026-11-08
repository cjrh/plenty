//! Handwritten format-neutral codecs and direct JSON tree conversion.
mod support;

const IMPORTS: &str = r#"
import std.json
import std.serial
from std.json import Json, JsonError, Member
from std.serial import SerialError, Fault, Step
"#;

#[test]
fn builtins_roundtrip_and_range_paths() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), AllocError | IoError | SerialError[JsonError]]:
    bounds = std.json.limits()
    options = std.json.codec_options()
    original = [("a", Some(255u8)), ("b", Nothing)]?
    encoded = std.json.dump(&original, &bounds)?
    decoded = std.json.load[list[(str, Option[u8])]](&encoded, &options)?
    print(decoded == original)?
    match std.json.load[list[u8]](b"[1,256]", &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path.unwrap(), error.complete)?
    Ok(())
"#
        ),
        "True\nstd.json.JsonFault.Serialization(std.serial.Fault.OutOfRange) [std.serial.Step.Index(1)] True\n",
    );
}

const SERVER: &str = r#"
class Server:
    port: u16
    label: str

    def __init__(self, port: u16, label: str = "local") -> Result[(), Fault]:
        if port == 0u16:
            return Err(Fault.OutOfRange)
        self.port = port
        self.label = label
        Ok(())

    def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), SerialError[E]]:
        std.serial.wrap(out.start_object())?
        std.serial.wrap(out.key("port"))?
        std.serial.at(std.serial.encode[u16, E, S](&self.port, out), Step.Field("port"))?
        std.serial.wrap(out.key("label"))?
        std.serial.at(std.serial.encode[str, E, S](&self.label, out), Step.Field("label"))?
        std.serial.wrap(out.end_object())

    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[Server, SerialError[E]]:
        std.serial.wrap(source.start_object())?
        mut port: Option[u16] = Nothing
        mut label: Option[str] = Nothing
        while True:
            match std.serial.wrap(source.next_key())?:
                case Some(name):
                    match name:
                        case "port":
                            if port.is_some():
                                return Err(std.serial.error(source.reject(Fault.DuplicateField(name))))
                            port = Some(std.serial.at(std.serial.decode[u16, E, D](source), Step.Field("port"))?)
                        case "label":
                            if label.is_some():
                                return Err(std.serial.error(source.reject(Fault.DuplicateField(name))))
                            label = Some(std.serial.at(std.serial.decode[str, E, D](source), Step.Field("label"))?)
                        case _:
                            std.serial.at(std.serial.wrap(source.unknown_key(&name)), Step.Field(name))?
                case Nothing:
                    break
        match port:
            case Some(value):
                match Server(value, label.unwrap_or("local")):
                    case Ok(server):
                        Ok(server)
                    case Err(cause):
                        Err(std.serial.error(source.reject(cause)))
            case Nothing:
                Err(std.serial.error(source.reject(Fault.MissingField("port"))))
"#;

#[test]
fn private_handwritten_hooks_compose_with_tree_and_text() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}{SERVER}
def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    options = std.json.codec_options()
    bounds = std.json.limits()
    tree_bounds = std.json.tree_limits()
    servers = std.json.load[list[Server]](b"[{{\"port\":8080}}]", &options)?
    tree = std.json.to_value(&servers, &tree_bounds)?
    print(std.json.encode_text(&tree, &bounds).unwrap())?
    decoded = std.json.from_value[list[Server]](tree, &tree_bounds)?
    print(decoded == servers)?
    Ok(())
"#
        ),
        "[{\"port\":8080,\"label\":\"local\"}]\nTrue\n",
    );
}

#[test]
fn missing_unknown_duplicate_and_constructor_rejections() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}{SERVER}
def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    mut options = std.json.codec_options()
    for input in [b"{{}}", b"{{\"port\":0}}", b"{{\"port\":80,\"extra\":[]}}"]?:
        match std.json.load[Server](&input, &options):
            case Ok(_):
                print("unexpected")?
            case Err(error):
                print(error.cause.fault)?
    options.unknown_fields = std.serial.UnknownFields.Ignore
    server = std.json.load[Server](b"{{\"ignored\":{{\"nested\":[1,2]}},\"port\":81}}", &options)?
    print(server.port)?
    options.limits.duplicate_keys = std.json.DuplicateKeys.Keep
    match std.json.load[Server](b"{{\"port\":80,\"port\":81}}", &options):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault)?
    Ok(())
"#
        ),
        "std.json.JsonFault.Serialization(std.serial.Fault.MissingField(\"port\"))\nstd.json.JsonFault.Serialization(std.serial.Fault.OutOfRange)\nstd.json.JsonFault.Serialization(std.serial.Fault.UnknownField(\"extra\"))\n81\nstd.json.JsonFault.Serialization(std.serial.Fault.DuplicateField(\"port\"))\n",
    );
}

#[test]
fn tree_conversion_limits_and_unsigned_precision() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), IoError | AllocError]:
    mut limits = std.json.tree_limits()
    limits.max_nodes = 2
    match std.json.to_value(&[1,2]?, &limits):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault)?
    limits = std.json.tree_limits()
    match std.json.to_value(&18446744073709551615u64, &limits):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault)?
    Ok(())
"#
        ),
        "std.json.JsonFault.OutputTooLarge\nstd.json.JsonFault.Serialization(std.serial.Fault.Unrepresentable)\n",
    );
}

#[test]
fn ambiguous_option_and_unsupported_types_are_diagnostics() {
    let error = support::check_error(&format!(
        r#"{IMPORTS}
def main() -> Result[(), SerialError[JsonError]]:
    bounds = std.json.limits()
    value: Option[Option[i64]] = Some(Nothing)
    drop(std.json.dump(&value, &bounds)?)
    Ok(())
"#
    ));
    assert!(error.contains("nested Option"), "{error}");
    let error = support::check_error(&format!(
        r#"{IMPORTS}
def main() -> Result[(), AllocError | SerialError[JsonError]]:
    inner: Option[i64] = Nothing
    value: Option[Box[Option[i64]]] = Some(Box(inner)?)
    drop(std.json.dump(&value, &std.json.limits())?)
    Ok(())
"#
    ));
    assert!(error.contains("nested Option"), "{error}");
}

#[test]
fn dictionaries_sets_boxes_and_tuple_lengths() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    options = std.json.codec_options()
    bounds = std.json.limits()
    values = std.json.load[dict[str, Box[list[i16]]]](b"{{\"x\":[-12,34]}}", &options)?
    print(std.json.dump(&values, &bounds)?.to_text().unwrap())?
    print(len(std.json.load[set[i64]](b"[3,4]", &options)?))?
    match std.json.load[set[i64]](b"[3,3]", &options):
        case Err(error):
            print(error.cause.fault)?
        case Ok(_):
            print("unexpected")?
    match std.json.load[(i64, bool)](b"[3]", &options):
        case Err(error):
            print(error.cause.fault)?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#
        ),
        "{\"x\":[-12,34]}\n2\nstd.json.JsonFault.Serialization(std.serial.Fault.DuplicateElement)\nstd.json.JsonFault.Serialization(std.serial.Fault.WrongLength(2, 1))\n",
    );
}

#[test]
fn malformed_trailing_numbers_and_tree_text_budgets() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), IoError]:
    options = std.json.codec_options()
    for input in [b"[1", b"[1] false"] .unwrap():
        match std.json.load[list[i64]](&input, &options):
            case Err(error):
                print(error.cause.fault)?
            case Ok(_):
                print("unexpected")?
    match std.json.load[f32](b"1e39", &options):
        case Err(error):
            print(error.cause.fault)?
        case Ok(_):
            print("unexpected")?
    mut limits = std.json.tree_limits()
    limits.max_text_bytes = 2
    match std.json.from_value[str](Json.Text("hello"), &limits):
        case Err(error):
            print(error.cause.fault)?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#
        ),
        "std.json.JsonFault.UnexpectedEnd\nstd.json.JsonFault.TrailingData\nstd.json.JsonFault.Serialization(std.serial.Fault.OutOfRange)\nstd.json.JsonFault.OutputTooLarge\n",
    );
}

#[test]
fn codecs_are_worker_eligible() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def work() -> ():
    options = std.json.codec_options()
    value = std.json.load[list[u8]](b"[1,2]", &options).unwrap()
    tree = std.json.to_value(&value, &std.json.tree_limits()).unwrap()
    decoded = std.json.from_value[list[u8]](tree, &std.json.tree_limits()).unwrap()
    print(decoded == value).unwrap()

def main() -> Result[(), SpawnError[()]]:
    with spawn(work).drop_job()?:
        pass
    Ok(())
"#
        ),
        "True\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn path_allocation_failure_keeps_typed_cause() {
    support::assert_filtered_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), IoError]:
    print("__test_fail_allocations_after_0__")?
    failure = std.serial.error(Fault.OutOfRange).at(Step.Index(7))
    print("__test_restore_allocations__")?
    print(failure.cause, failure.path.is_none(), failure.complete)?
    Ok(())
"#
        ),
        "std.serial.Fault.OutOfRange True False",
    );
}

#[test]
fn generic_class_and_enum_hooks_use_static_dispatch() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
class Cell[T]:
    value: T
    def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), SerialError[E]]:
        std.serial.encode[T, E, S](&self.value, out)
    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[Cell[T], SerialError[E]]:
        Ok(Cell[T](std.serial.decode[T, E, D](source)?))

enum Mode:
    Fast
    Slow
    def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), SerialError[E]]:
        match self:
            case Mode.Fast:
                std.serial.wrap(out.text("fast"))
            case Mode.Slow:
                std.serial.wrap(out.text("slow"))
    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[Mode, SerialError[E]]:
        match std.serial.wrap(source.text())?:
            case "fast":
                Ok(Mode.Fast)
            case "slow":
                Ok(Mode.Slow)
            case _ as other:
                Err(std.serial.error(source.reject(Fault.UnknownVariant(other))))

def main() -> Result[(), IoError | SerialError[JsonError]]:
    options = std.json.codec_options()
    value = std.json.load[Cell[Mode]](b"\"fast\"", &options)?
    print(std.json.dump(&value, &std.json.limits())?.to_text().unwrap())?
    Ok(())
"#
        ),
        "\"fast\"\n",
    );
}

#[test]
fn private_hooks_cross_modules_without_exposing_fields() {
    let directory = tempfile::tempdir().unwrap();
    let model = r#"
import std.serial
pub class Secret:
    value: i64
    def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), std.serial.SerialError[E]]:
        std.serial.wrap(out.integer(self.value))
    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[Secret, std.serial.SerialError[E]]:
        Ok(Secret(std.serial.wrap(source.integer())?))
"#;
    std::fs::write(directory.path().join("model.plenty"), model).unwrap();
    let main = directory.path().join("main.plenty");
    std::fs::write(
        &main,
        format!(
            r#"{IMPORTS}
from model import Secret
def main() -> Result[(), IoError | SerialError[JsonError]]:
    value = std.json.load[Secret](b"42", &std.json.codec_options())?
    print(std.json.dump(&value, &std.json.limits())?.to_text().unwrap())?
    Ok(())
"#
        ),
    )
    .unwrap();
    let executable =
        support::compile_file(directory.path(), &std::fs::read_to_string(&main).unwrap());
    let output = std::process::Command::new(&executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"42\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_release_partial_values_and_keep_causes() {
    support::assert_filtered_stdout(
        &format!(
            r#"{IMPORTS}
from std.bytes import Bytes

def attempt(which: i64, value: &list[str], source: &Bytes, tree: Json) -> Result[(), SerialError[JsonError]]:
    if which == 0:
        drop(std.json.dump(value, &std.json.limits())?)
    elif which == 1:
        drop(std.json.load[list[str]](&*source, &std.json.codec_options())?)
    elif which == 2:
        drop(std.json.to_value(value, &std.json.tree_limits())?)
    else:
        drop(std.json.from_value[list[str]](tree, &std.json.tree_limits())?)
    Ok(())

def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    value = ["a string longer than inline storage", "another string longer than inline storage"]?
    source = std.json.dump(&value, &std.json.limits())?
    mut markers = list[str]()?
    for count in range(128):
        markers.append("".join(&["__test_fail_allocations_after_", str.from(count)?, "__"]?)?)?
    for which in range(4):
        mut refused = False
        mut succeeded = False
        mut other = False
        for count in range(128):
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
"#
        ),
        "True True False\nTrue True False\nTrue True False\nTrue True False",
    );
}

#[test]
fn dump_rejects_incomplete_handwritten_output() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
class Broken:
    mode: i64

    def __encode__[E, S: std.serial.Encoder[E]](self, output: &mut S) -> Result[(), SerialError[E]]:
        if self.mode == 1:
            std.serial.wrap(output.start_array())?
        elif self.mode == 2:
            std.serial.wrap(output.start_object())?
            std.serial.wrap(output.key("unfinished"))?
        Ok(())

def main() -> Result[(), IoError]:
    for mode in range(3):
        match std.json.dump(&Broken(mode), &std.json.limits()):
            case Ok(_):
                print("unexpected success")?
            case Err(error):
                print(error.cause.fault)?
    Ok(())
"#
        ),
        "std.json.JsonFault.InvalidState\nstd.json.JsonFault.InvalidState\nstd.json.JsonFault.InvalidState\n",
    );
}

#[test]
fn adapter_nonfinite_float_and_final_tuple_boundary_regressions() {
    support::assert_stdout(
        r#"import std.toml
import std.json
import std.io
import std.bytes
from std.bytes import Bytes
from std.io import Cursor, StreamError

pub class Failing:
    cursor: Cursor
    pub def read(self: &mut Failing, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        if self.cursor.position() >= 4:
            return Err(StreamError.UnexpectedEof)
        end = unchecked:
            start + 1
        self.cursor.read(buffer, start, end)

def main() -> Result[(), IoError | AllocError]:
    for text in ["a = inf", "a = -inf", "a = nan", "a = 1e39"]?:
        match std.toml.load[dict[str, f32]](&text, &std.toml.codec_options()):
            case Ok(values):
                value = values.get("a").unwrap()
                print(value == f32.INF, value == -f32.INF, value != value)?
            case Err(error):
                print(error.cause.fault, error.path)?
    mut source = std.json.codec_reader(Failing(std.io.cursor(std.bytes.from_text("[\"x\"]")?)))
    match std.json.load_from[(str,), Failing](&mut source, &std.json.codec_options()):
        case Ok(_):
            print("unexpected")?
        case Err(error):
            print(error.cause.fault, error.path)?
    Ok(())
"#,
        "True False False\nFalse True False\nFalse False True\nstd.toml.TomlFault.Serialization(std.serial.Fault.OutOfRange) Option[list[std.serial.Step]].Some([std.serial.Step.Key(\"a\")])\nstd.json.JsonFault.Stream(std.io.StreamError.UnexpectedEof) Option[list[std.serial.Step]].Some([std.serial.Step.Index(1)])\n",
    );
}
#[test]
fn deeply_nested_unknown_tree_values_preserve_siblings_and_depth_limits() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}{SERVER}
def unknown_tree() -> Result[Json, AllocError]:
    mut slot = [Json.Null]?
    for depth in range(1000):
        if depth % 2 == 0:
            mut items = list[Json]()?
            items.append(replace(slot[0], Json.Null))?
            slot[0] = Json.Array(items)
        else:
            mut members = list[Member]()?
            members.append(Member("nested", replace(slot[0], Json.Null)))?
            slot[0] = Json.Object(members)
    mut members = list[Member]()?
    members.append(Member("ignored", replace(slot[0], Json.Null)))?
    members.append(Member("port", Json.Int(80)))?
    Ok(Json.Object(members))

def main() -> Result[(), IoError | AllocError | SerialError[JsonError]]:
    mut bounds = std.json.tree_limits()
    bounds.max_depth = 1024
    server = std.json.from_value[Server](unknown_tree()?, &bounds, std.serial.UnknownFields.Ignore)?
    print(server.port, server.label)?
    bounds.max_depth = 64
    match std.json.from_value[Server](unknown_tree()?, &bounds, std.serial.UnknownFields.Ignore):
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error.cause.fault, error.path.unwrap(), error.complete)?
    Ok(())
"#
        ),
        "80 local\nstd.json.JsonFault.DepthExceeded [std.serial.Step.Field(\"ignored\")] True\n",
    );
}

#[test]
fn toml_text_adapter_accepts_ranges_without_changing_generic_codec_callbacks() {
    support::assert_stdout(
        r#"
import std.toml
import std.serial

def main() -> Result[(), IoError | std.serial.SerialError[std.toml.TomlError]]:
    source = "!name='café 🙂'\n!"
    options = std.toml.codec_options()
    value = std.toml.load[dict[str, str]](&source[1:-1], &options)?
    print(value.get("name").unwrap() == "café 🙂")?
    print(std.toml.dump(&value, &options)?.startswith("name = "))?
    Ok(())
"#,
        "True\nTrue\n",
    );
}
