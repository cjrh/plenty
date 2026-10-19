# Read and write JSON

`std.json` decodes and encodes JSON under explicit limits. It offers two levels:
a value tree for documents that fit in memory, and events for documents that
should not be held whole.

## Parse a document into a tree

```plenty
import std.json
from std.json import Json

def main() -> Result[(), AllocError | IoError | std.json.JsonError]:
    bounds = std.json.limits()
    value = std.json.parse_text("{\"name\": \"Ada\", \"tags\": [\"x\", \"y\"], \"age\": 36, \"ratio\": 0.5}", &bounds)?
    match &value:
        case Json.Object(members):
            for member in members:
                match &member.value:
                    case Json.Text(text):
                        print(member.key, "is the text", text)?
                    case Json.Int(number):
                        print(member.key, "is the integer", *number)?
                    case Json.Float(number):
                        print(member.key, "is the float", *number)?
                    case Json.Array(items):
                        print(member.key, "has", len(items), "items")?
                    case _:
                        print(member.key, "is something else")?
            print(std.json.find(members, "age"), std.json.find(members, "email"))?
        case _:
            print("not an object")?
    print(std.json.encode_text(&value, &bounds)?)?
    Ok(())
```
```output
name is the text Ada
tags has 2 items
age is the integer 36
ratio is the float 0.5
Option[i64].Some(2) Option[i64].Nothing
{"name":"Ada","tags":["x","y"],"age":36,"ratio":0.5}
```

`parse_text` returns a `Json` value, an enum with one variant per JSON type.
`match` is how you find out which one you have. A number written without a
fraction or exponent is `Json.Int`; any other number is `Json.Float`.

An object is a `list[Member]` in source order, where each member has a `key` and
a `value`. `find` returns the index of the first member with a key. Output is
compact: no spaces or line breaks, and members stay in the order of the list.

Every call takes `Limits`. `limits()` gives the defaults: 64 levels of nesting,
64 MiB of input, 64 MiB of output, and duplicate keys rejected.

## Errors say where

A `JsonError` carries a `fault`, the byte `offset`, and a `line` and `column`
that both start at 1.

```plenty
import std.json
from std.json import Limits

def attempt(text: &str, bounds: &Limits) -> Result[(), AllocError | IoError | std.json.JsonError]:
    match std.json.parse_text(text, bounds):
        case Ok(value):
            print("ok", std.json.encode_text(&value, bounds)?)?
        case Err(error):
            print(error.fault, "at byte", error.offset, "line", error.line, "column", error.column)?
    Ok(())

def main() -> Result[(), AllocError | IoError | std.json.JsonError]:
    mut bounds = std.json.limits()
    attempt("[1, 2,\n  oops]", &bounds)?
    attempt("[1, 2", &bounds)?
    attempt("{\"a\": 1, \"a\": 2}", &bounds)?
    attempt("[9223372036854775808]", &bounds)?
    attempt("[1] [2]", &bounds)?
    bounds.max_depth = 2
    attempt("[[1]]", &bounds)?
    attempt("[[[1]]]", &bounds)?
    Ok(())
```
```output
std.json.JsonFault.UnexpectedByte at byte 9 line 2 column 3
std.json.JsonFault.UnexpectedEnd at byte 5 line 1 column 6
std.json.JsonFault.DuplicateKey at byte 9 line 1 column 10
std.json.JsonFault.NumberOutOfRange at byte 1 line 1 column 2
std.json.JsonFault.TrailingData at byte 4 line 1 column 5
ok [[1]]
std.json.JsonFault.DepthExceeded at byte 2 line 1 column 3
```

The decoder accepts standard JSON and nothing else: no comments, no trailing
commas, one document per input. An integer that does not fit `i64` is an error
in the tree, never a rounded float. `max_depth` counts open arrays and objects,
so the limit of 2 accepts `[[1]]` and stops at the third `[`.

## Stream events instead of building a tree

A `Decoder` reports one `Event` at a time, and an `Encoder` accepts one call per
event. Neither holds the document, so memory stays bounded however long the
input is.

```plenty
import std.bytes
import std.io
import std.json
from std.json import Event

def main() -> Result[(), IoError | ParseError | std.json.JsonError | std.bytes.BytesError]:
    bounds = std.json.limits()
    mut reader = std.io.cursor(std.bytes.from_text("[{\"id\": 7, \"ok\": true}, {\"id\": 18446744073709551615}]")?)
    mut decoder = std.json.decoder(&bounds)?
    mut ids = 0
    mut expecting_id = False
    while True:
        event = std.json.read_event(&mut decoder, &mut reader)?
        match &event:
            case Event.End:
                break
            case Event.Key(name):
                expecting_id = name == "id"
            case Event.Number(text):
                if expecting_id:
                    ids += 1
                    print("id", u64.parse(text)?)?
            case _:
                pass
    print(ids, "ids in", decoder.offset(), "bytes")?

    mut encoder = std.json.encoder(&bounds)?
    encoder.start_object()?
    encoder.key("ids")?
    encoder.integer(ids)?
    encoder.key("note")?
    encoder.text("line one\nline two")?
    encoder.end_object()?
    mut out = std.io.cursor(std.bytes.empty()?)
    std.json.write_pending(&mut encoder, &mut out)?
    print(out.data.to_text()?)?
    match encoder.null():
        case Ok(_):
            print("written")?
        case Err(error):
            print(error.fault)?
    Ok(())
```
```output
id 7
id 18446744073709551615
2 ids in 53 bytes
{"ids":2,"note":"line one\nline two"}
std.json.JsonFault.InvalidState
```

`read_event` pulls bytes from any `std.io` reader as the decoder needs them and
returns `Event.End` after the document. `Event.Number` holds the number's text
exactly as written, so the program chooses the type: here `u64`, which holds a
value the tree's `i64` cannot.

The encoder checks call order. A key outside an object, a value where a key
belongs, or a second top-level value is `InvalidState`, so an encoder cannot
produce malformed JSON. `write_pending` hands the bytes produced so far to a
writer; call it between events to keep the buffer small.

The [`std.json` reference](../../design/stdlib/json.md) lists every fault,
limit, and cost.
