# Decode configuration into classes

Use `@serial` when configuration has a known shape. It generates ordinary
checked codecs for a class or enum, so JSON and TOML can use the same types.
The generated decoder calls the normal constructor: validation and constructor
defaults still apply.

```plenty
import std.json
import std.toml
from std.json import JsonError
from std.toml import TomlError
from std.serial import Fault, SerialError

@serial
class Server:
    port: u16
    host: str

    def __init__(self, port: u16, host: str = "localhost") -> Result[(), Fault]:
        if port == 0u16:
            return Err(Fault.OutOfRange)
        self.port = port
        self.host = host
        Ok(())

@serial
class Config:
    server: Server
    tags: list[str]
    backup: Option[Server]

def main() -> Result[(), IoError | SerialError[JsonError] | SerialError[TomlError]]:
    json_options = std.json.codec_options()
    config = std.json.load[Config](b"{\"tags\":[\"web\"],\"server\":{\"port\":8080}}", &json_options)?
    print(config.server.host, config.server.port, config.backup.is_none())?

    toml_options = std.toml.codec_options()
    text = std.toml.dump(&config, &toml_options)?
    restored = std.toml.load[Config](&text, &toml_options)?
    print(restored == config)?
    Ok(())
```
```output
localhost 8080 True
True
```

Fields can appear in any input order. An absent field uses its constructor
parameter's default, or `Nothing` for an optional field without a default.
Other missing fields fail. An explicitly supplied null is decoded as a value:
it becomes `Nothing` for `Option[T]` and does not invoke a constructor default.

Unknown and duplicate fields are rejected by default, which catches misspelled
configuration keys. Set `options.unknown_fields = std.serial.UnknownFields.Ignore`
on mutable options when deliberately accepting extra fields. A type can define
its own private `__decode__` hook for a different schema or constructor mapping.

TOML has no null value. The encoder omits a class field whose value is
`Nothing`; the decoder restores an absent optional field. A dictionary entry
with a null value is rejected so a round trip cannot silently lose its key.

Keep the complete error type when propagating a failure. `SerialError[JsonError]`
and `SerialError[TomlError]` preserve their format-specific causes and collect
the available field, variant, key and index path. Path collection can fail to
allocate; `complete=False` marks the truncated path while retaining the cause.
Constructor errors of type `Fault` use that same format error domain. A
constructor returning another error type needs a handwritten decoding hook to
choose its mapping.

## Readable errors

Render the complete typed error to keep its nested field path. Reports have an
explicit output budget and do not include rejected values or source text by
default. Keep the structured error for recovery if rendering fails.

```plenty
import std.json
import std.report
import std.fmt

@serial
class Settings:
    enabled: bool

def main() -> Result[(), IoError | std.fmt.FormatError]:
    match std.json.load[Settings](b"{\"enabled\":42}", &std.json.codec_options()):
        case Ok(_):
            print("valid")?
        case Err(error):
            message = std.report.to_string(&error, 1024, 16)?
            print(message.find("expected boolean, found number").is_some())?
            print(message.endswith("at $[\"enabled\"]"))?
    Ok(())
```
```output
True
True
```

For a local source excerpt, explicitly call `std.report.diagnostic(&error,
&source)`. Its byte budgets also cover escaping and caret output. TOML syntax
errors retain locations; typed TOML schema errors have a field path without
source coordinates because the owning tree retains no source spans. Read
[serialization diagnostics](../../design/stdlib/serial-diagnostics.md) for
Unicode, privacy, allocation and truncation details.

See [derived codec contracts](../../design/stdlib/serial-derive.md),
[JSON limits and reader loading](../../design/stdlib/json-codecs.md), and
[TOML representation limits](../../design/stdlib/toml-codecs.md) for the full
supported types and costs.
