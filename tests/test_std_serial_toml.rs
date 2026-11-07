//! Typed TOML codecs share std.serial hooks without routing through JSON.
mod support;

const IMPORTS: &str = r#"
import std.toml
import std.serial
from std.toml import Toml, TomlError, TomlFault, Entry
from std.serial import SerialError, Fault, Step

def failure[T](result: Result[T, SerialError[TomlError]]) -> TomlFault:
    error: Option[TomlFault] = match result:
        case Err(error):
            Some(error.cause.fault)
        case Ok(_):
            Nothing
    error.unwrap()
"#;

#[test]
fn dictionary_tuple_and_optional_member_roundtrip() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), IoError | AllocError | SerialError[TomlError]]:
    options = std.toml.codec_options()
    original = {{"ports": [80u16, 443u16]?, "backup": [8080u16]?}}?
    text = std.toml.dump(&original, &options)?
    restored = std.toml.load[dict[str, list[u16]]](&text, &options)?
    print(restored == original)?
    mut fields = std.serial.wrap(std.toml.tree_encoder(&options.tree_limits))?
    std.serial.wrap(fields.start_object())?
    std.serial.wrap(fields.field("present"))?
    std.serial.wrap(fields.integer(42))?
    std.serial.wrap(fields.field("absent"))?
    std.serial.wrap(fields.null())?
    std.serial.wrap(fields.end_object())?
    print(std.toml.encode_text(&std.serial.wrap(fields.finish())?, &options.limits).unwrap())?
    tree = std.toml.to_value(&(True, "name", 3.5), &options.tree_limits)?
    print(std.toml.from_value[(bool, str, f64)](tree, &options.tree_limits)?)?
    Ok(())
"#
        ),
        "True\npresent = 42\n\n(True, \"name\", 3.5)\n",
    );
}

#[test]
fn numeric_and_null_representation_errors_have_paths() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), IoError | AllocError]:
    options = std.toml.codec_options()
    match std.toml.dump(&{{"too_big": 18446744073709551615u64}}?, &options):
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
        case Ok(_):
            print("unexpected")?
    match std.toml.dump[dict[str, Option[i64]]](&{{"missing": Nothing}}?, &options):
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
        case Ok(_):
            print("unexpected")?
    match std.toml.dump(&{{"items": [Some(1), Nothing]?}}?, &options):
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
        case Ok(_):
            print("unexpected")?
    match std.toml.to_value[Option[i64]](&Nothing, &options.tree_limits):
        case Err(error):
            print(error.cause.fault, error.path.is_none())?
        case Ok(_):
            print("unexpected")?
    match std.toml.load[dict[str, list[u8]]]("items = [1, 256]", &options):
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#
        ),
        "std.toml.TomlFault.Serialization(std.serial.Fault.Unrepresentable) [std.serial.Step.Key(\"too_big\")]\nstd.toml.TomlFault.Serialization(std.serial.Fault.Unrepresentable) [std.serial.Step.Key(\"missing\")]\nstd.toml.TomlFault.Serialization(std.serial.Fault.Unrepresentable) [std.serial.Step.Index(1), std.serial.Step.Key(\"items\")]\nstd.toml.TomlFault.Serialization(std.serial.Fault.Unrepresentable) True\nstd.toml.TomlFault.Serialization(std.serial.Fault.OutOfRange) [std.serial.Step.Index(1), std.serial.Step.Key(\"items\")]\n",
    );
}

const MODEL: &str = r#"
class Config:
    port: u16
    label: str

    def __encode__[E, S: std.serial.Encoder[E]](self, output: &mut S) -> Result[(), SerialError[E]]:
        std.serial.wrap(output.start_object())?
        std.serial.wrap(output.key("port"))?
        std.serial.at(std.serial.encode[u16, E, S](&self.port, output), Step.Field("port"))?
        std.serial.wrap(output.key("label"))?
        std.serial.at(std.serial.encode[str, E, S](&self.label, output), Step.Field("label"))?
        std.serial.wrap(output.end_object())

    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[Config, SerialError[E]]:
        std.serial.wrap(source.start_object())?
        mut port: Option[u16] = Nothing
        mut label: Option[str] = Nothing
        while True:
            match std.serial.wrap(source.next_key())?:
                case Some(name):
                    match name:
                        case "port":
                            port = Some(std.serial.at(std.serial.decode[u16, E, D](source), Step.Field("port"))?)
                        case "label":
                            label = Some(std.serial.at(std.serial.decode[str, E, D](source), Step.Field("label"))?)
                        case _:
                            std.serial.at(std.serial.wrap(source.unknown_key(&name)), Step.Field(name))?
                case Nothing:
                    break
        match port:
            case Some(value):
                Ok(Config(value, label.unwrap_or("local")))
            case Nothing:
                Err(std.serial.error(source.reject(Fault.MissingField("port"))))
"#;

#[test]
fn handwritten_private_hooks_unknown_policy_and_missing_fields() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}{MODEL}
def main() -> Result[(), IoError | SerialError[TomlError]]:
    mut options = std.toml.codec_options()
    config = std.toml.load[Config]("port = 8080", &options)?
    print(config.port, config.label)?
    print(std.toml.load[Config](&std.toml.dump(&config, &options)?, &options)? == config)?
    match std.toml.load[Config]("port = 80\nextra = {{ nested = [1, 2, 3] }}", &options):
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
        case Ok(_):
            print("unexpected")?
    options.unknown_fields = std.serial.UnknownFields.Ignore
    print(std.toml.load[Config]("port = 80\nextra = {{ nested = [1, 2, 3], date = 1979-05-27 }}", &options)?.port)?
    match std.toml.load[Config]("label = 'only'", &options):
        case Err(error):
            print(error.cause.fault)?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#
        ),
        "8080 local\nTrue\nstd.toml.TomlFault.Serialization(std.serial.Fault.UnknownField(\"extra\")) [std.serial.Step.Field(\"extra\")]\n80\nstd.toml.TomlFault.Serialization(std.serial.Fault.MissingField(\"port\"))\n",
    );
}

#[test]
fn tree_limits_parser_writer_limits_and_complete_output() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
class Broken:
    mode: i64
    def __encode__[E, S: std.serial.Encoder[E]](self, output: &mut S) -> Result[(), SerialError[E]]:
        if self.mode == 1:
            std.serial.wrap(output.start_object())?
        elif self.mode == 2:
            std.serial.wrap(output.start_object())?
            std.serial.wrap(output.key("unfinished"))?
        Ok(())

    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[Broken, SerialError[E]]:
        Ok(Broken(0))

def main() -> Result[(), IoError | AllocError]:
    mut options = std.toml.codec_options()
    for mode in range(3):
        print(failure(std.toml.dump(&Broken(mode), &options)))?
    print(failure(std.toml.load[Broken]("x = 1", &options)))?
    value = {{"items": [1, 2]?}}?
    options.tree_limits.max_depth = 1
    print(failure(std.toml.dump(&value, &options)))?
    options.tree_limits.max_depth = 64
    options.tree_limits.max_nodes = 2
    print(failure(std.toml.load[dict[str, list[i64]]]("items = [1, 2]", &options)))?
    options.tree_limits.max_nodes = 100
    options.tree_limits.max_text_bytes = 2
    print(failure(std.toml.dump(&value, &options)))?
    options.tree_limits.max_text_bytes = 100
    options.limits.max_input = 2
    print(failure(std.toml.load[dict[str, i64]]("item = 1", &options)))?
    options.limits.max_output = 2
    print(failure(std.toml.dump(&value, &options)))?
    Ok(())
"#
        ),
        "std.toml.TomlFault.InvalidState\nstd.toml.TomlFault.InvalidState\nstd.toml.TomlFault.InvalidState\nstd.toml.TomlFault.InvalidState\nstd.toml.TomlFault.DepthExceeded\nstd.toml.TomlFault.OutputTooLarge\nstd.toml.TomlFault.OutputTooLarge\nstd.toml.TomlFault.InputTooLarge\nstd.toml.TomlFault.OutputTooLarge\n",
    );
}

#[test]
fn nonfinite_floats_and_root_table_contract() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), IoError | SerialError[TomlError]]:
    options = std.toml.codec_options()
    values = std.toml.load[dict[str, f64]]("positive = inf\nnegative = -inf\nmissing = nan", &options)?
    text = std.toml.dump(&values, &options)?
    restored = std.toml.load[dict[str, f64]](&text, &options)?
    print(restored.get("positive").unwrap() == values.get("positive").unwrap())?
    print(restored.get("negative").unwrap() == values.get("negative").unwrap())?
    value = restored.get("missing").unwrap()
    print(value != value)?
    print(failure(std.toml.dump(&42, &options)))?
    Ok(())
"#
        ),
        "True\nTrue\nTrue\nstd.toml.TomlFault.NotATable\n",
    );
}

#[test]
fn omitted_optional_fields_still_reject_duplicate_names() {
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), IoError | SerialError[TomlError]]:
    mut output = std.serial.wrap(std.toml.tree_encoder(&std.toml.tree_limits()))?
    std.serial.wrap(output.start_object())?
    std.serial.wrap(output.field("absent"))?
    std.serial.wrap(output.null())?
    match output.key("absent"):
        case Err(error):
            print(error.fault)?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#
        ),
        "std.toml.TomlFault.Serialization(std.serial.Fault.DuplicateField(\"absent\"))\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_keep_cause_and_release_partial_trees() {
    support::assert_filtered_stdout(
        &format!(
            r#"{IMPORTS}
def attempt(which: i64, value: &dict[str, list[str]], text: &str, tree: Toml) -> Result[(), SerialError[TomlError]]:
    options = std.toml.codec_options()
    if which == 0:
        drop(std.toml.dump(value, &options)?)
    elif which == 1:
        drop(std.toml.load[dict[str, list[str]]](text, &options)?)
    elif which == 2:
        drop(std.toml.to_value(value, &options.tree_limits)?)
    elif which == 3:
        drop(std.toml.from_value[dict[str, list[str]]](tree, &options.tree_limits)?)
    else:
        mut output = std.serial.wrap(std.toml.tree_encoder(&options.tree_limits))?
        std.serial.wrap(output.start_object())?
        std.serial.wrap(output.field("an omitted optional field"))?
        std.serial.wrap(output.null())?
        std.serial.wrap(output.end_object())?
        drop(std.serial.wrap(output.finish())?)
    Ok(())

def main() -> Result[(), IoError | AllocError | SerialError[TomlError]]:
    value = {{"contents": ["a string longer than inline storage", "another string longer than inline storage"]?}}?
    options = std.toml.codec_options()
    text = std.toml.dump(&value, &options)?
    mut markers = list[str]()?
    for count in range(128):
        markers.append("".join(&["__test_fail_allocations_after_", str.from(count)?, "__"]?)?)?
    for which in range(5):
        mut refused = False
        mut succeeded = False
        mut other = False
        for count in range(128):
            tree = std.toml.to_value(&value, &options.tree_limits)?
            print(markers[count])?
            result = attempt(which, &value, &text, tree)
            print("__test_restore_allocations__")?
            match result:
                case Ok(_):
                    succeeded = True
                case Err(error):
                    match error.cause.fault:
                        case TomlFault.Allocation(_):
                            refused = True
                        case _:
                            other = True
        print(refused, succeeded, other)?
    Ok(())
"#
        ),
        "True True False\nTrue True False\nTrue True False\nTrue True False\nTrue True False",
    );
}

#[test]
fn ignored_mixed_containers_preserve_siblings_and_enforce_tree_depth() {
    let mut toml_value = "[]".to_string();
    let mut json_value = "[]".to_string();
    for _ in 0..20 {
        toml_value = format!("{{ nested = [{toml_value}, {{}}], empty = [] }}");
        json_value = format!(r#"{{"nested":[{json_value},{{}}],"empty":[]}}"#);
    }
    let toml = format!("extra = {toml_value}\nport = 8080\nlabel = 'following'\nlast = []");
    let json = format!(r#"{{"extra":{json_value},"port":8080,"label":"following","last":[]}}"#);
    support::assert_stdout(
        &format!(
            r#"{IMPORTS}{MODEL}
import std.json
from std.json import JsonError

def main() -> Result[(), IoError | SerialError[TomlError] | SerialError[JsonError]]:
    mut options = std.toml.codec_options()
    options.unknown_fields = std.serial.UnknownFields.Ignore
    value = std.toml.load[Config]({toml:?}, &options)?
    print(value.port, value.label)?
    json_bounds = std.json.limits()
    tree = std.serial.wrap(std.json.parse_text({json:?}, &json_bounds))?
    mut tree_bounds = std.json.tree_limits()
    from_json = std.json.from_value[Config](tree, &tree_bounds, std.serial.UnknownFields.Ignore)?
    print(from_json.port, from_json.label)?
    options.tree_limits.max_depth = 4
    match std.toml.load[Config]({toml:?}, &options):
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
        case Ok(_):
            print("unexpected")?
    deep_tree = std.serial.wrap(std.json.parse_text({json:?}, &json_bounds))?
    tree_bounds.max_depth = 4
    match std.json.from_value[Config](deep_tree, &tree_bounds, std.serial.UnknownFields.Ignore):
        case Err(error):
            print(error.cause.fault, error.path.unwrap())?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#
        ),
        "8080 following\n8080 following\nstd.toml.TomlFault.DepthExceeded [std.serial.Step.Field(\"extra\")]\nstd.json.JsonFault.DepthExceeded [std.serial.Step.Field(\"extra\")]\n",
    );
}
