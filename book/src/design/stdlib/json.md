# std.json

`import std.json` decodes and encodes JSON (RFC 8259) under explicit limits,
with one error type, `JsonError`. It has two levels:

- **Events**: a resumable `Decoder` and a validating `Encoder` that never hold
  the document. Typed codecs are ordinary functions over them.
- **Value tree**: `Json`, built from and written through the same events.

The module is checked Plenty code with no native primitives of its own. It
reads and writes [`std.bytes`](bytes.md) buffers and [`std.io`](io.md) streams,
and is available on the supported x86-64 Linux GNU target.

## Accepted text

The decoder accepts exactly RFC 8259 and one document per input.

| Rule | Behavior |
|---|---|
| Encoding | UTF-8, validated strictly; no byte-order mark |
| Whitespace | Space, tab, `\n`, `\r` between tokens only |
| Top level | Any value, then only whitespace; anything else is `TrailingData` |
| Extensions | None: no comments, trailing commas, single quotes, `NaN`, `Infinity`, hex, or leading `+` |
| Numbers | `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?` |
| Strings | Bytes below 32 must be escaped; escapes are `\" \\ \/ \b \f \n \r \t \uXXXX` with digits of either case |
| Surrogates | A `\uD800`-`\uDBFF` escape must be followed by a `\uDC00`-`\uDFFF` escape; the pair is one scalar. Anything else is `InvalidSurrogate` |
| Duplicate keys | Compared after unescaping. Rejected by the tree unless `Limits` says `Keep`; never checked by the event decoder |

The encoder writes compact text: no whitespace, members in call order. In
strings it escapes `"`, `\`, and bytes below 32 (`\b \f \n \r \t`, otherwise
`\u00xx` in lowercase); every other scalar, including `/` and non-ASCII, is
written as UTF-8.

## Limits

```text
pub class Limits:
    pub max_depth: i64
    pub max_input: i64
    pub max_output: i64
    pub duplicate_keys: DuplicateKeys   # Reject or Keep
```

`limits()` returns `Limits(64, 67108864, 67108864, DuplicateKeys.Reject)`. The
fields are public; adjust a copy. Every constructor and whole-document function
takes `&Limits` and returns `InvalidLimits` unless
`0 <= max_depth <= 1024`, `max_input >= 0`, and `max_output >= 0`.

| Field | Bounds | When exceeded |
|---|---|---|
| `max_depth` | Arrays and objects open at once. `0` allows scalars only | `DepthExceeded` at the opening bracket |
| `max_input` | Total bytes given to one decoder | `InputTooLarge`; the offset is the limit |
| `max_output` | Total bytes one encoder produces, including bytes already taken | `OutputTooLarge` |
| `duplicate_keys` | Whether the tree accepts a repeated key in one object | `DuplicateKey` at the repeated key |

The depth limit also bounds the encoder's recursion over a `Json` tree, so a
tree built by hand cannot overflow the stack.

## Errors

```text
pub class JsonError:
    pub fault: JsonFault
    pub offset: i64
    pub line: i64
    pub column: i64
```

For decoding, `offset` is the byte offset from 0, and `line` and `column` start
at 1. Only `\n` ends a line. A column is one Unicode scalar, not one byte. A
syntax fault points at the offending byte; a fault found after a token was read
(`DuplicateKey`, `NumberOutOfRange`) points at the token's first byte.
`UnexpectedEnd` points one past the last byte.

For encoding, `offset` is the number of bytes produced when the call failed and
`line` and `column` are 0. `InvalidLimits` and a failed constructor report
0, 0, 0.

| `JsonFault` | Meaning |
|---|---|
| `UnexpectedByte` | A byte that cannot start or continue the document here |
| `UnexpectedEnd` | Input ended inside a document, or was empty |
| `TrailingData` | A non-whitespace byte after the document |
| `InvalidLiteral` | A misspelled `null`, `true`, or `false` |
| `InvalidNumber` | A byte that breaks the number grammar; for `Encoder.number`, text that is not a JSON number |
| `NumberOutOfRange` | An integer outside `i64` or a decimal outside finite `f64`, in the tree |
| `InvalidEscape` | An unknown escape or a non-hex digit in `\uXXXX` |
| `InvalidSurrogate` | An unpaired surrogate escape |
| `ControlCharacter` | An unescaped byte below 32 in a string |
| `InvalidUtf8` | Malformed, overlong, surrogate, or out-of-range UTF-8 in a string |
| `DuplicateKey` | A repeated key under `DuplicateKeys.Reject` |
| `DepthExceeded` | Nesting beyond `max_depth` |
| `InputTooLarge`, `OutputTooLarge` | A size limit was reached |
| `NonFiniteNumber` | A NaN or infinite `f64` given to the encoder |
| `InvalidState` | A call that the decoder's or encoder's state does not allow |
| `InvalidLimits` | `Limits` outside the valid ranges |
| `OutOfRange` | A `feed` range outside its source |
| `Stream(StreamError)` | A reader or writer failed; `InvalidProgress` if it reported an impossible count |
| `Allocation(AllocError)` | Memory was refused |

After any error other than a refused `feed` range, a decoder or encoder is
poisoned: every later call reports `InvalidState`. Partial trees and buffers are
released before the error returns.

## Events

```text
pub enum Event:
    Pending
    End
    Null
    Bool(bool)
    Number(str)
    Text(str)
    Key(str)
    StartArray
    EndArray
    StartObject
    EndObject
```

`Number` holds the validated source text, so a codec chooses the type and loses
nothing: `i64.parse`, `u64.parse`, `f64.parse`, or the text itself. `Text` and
`Key` hold the unescaped string.

### Decoder

`decoder(bounds: &Limits) -> Result[Decoder, JsonError]`.

| Method | Receiver | Result | Contract |
|---|---|---|---|
| `feed(source: &Bytes, start, stop)` | exclusive | `Result[(), JsonError]` | Copy a range into the unexamined input |
| `finish()` | exclusive | `()` | Declare that no more input will arrive |
| `next()` | exclusive | `Result[Event, JsonError]` | The next event |
| `offset()` | shared | `i64` | Bytes examined |
| `depth()` | shared | `i64` | Arrays and objects open |
| `reject(fault: JsonFault)` | shared | `JsonError` | An error placed at the first byte of the most recent event's token |

`next` returns `Pending` when every fed byte has been examined and the document
is incomplete; feed more or call `finish`. After `finish` it returns `End` once
the document and trailing whitespace are consumed, and `End` again on every
later call. A number at the end of the fed input stays pending until the next
byte or `finish` shows where it ends. Input may be split anywhere, including
inside an escape or a UTF-8 sequence.

`read_event[R: Reader](source: &mut Decoder, reader: &mut R) -> Result[Event, JsonError]`
drives a decoder from a stream. It reads up to 4096 bytes whenever `next` is
`Pending`, retries `Interrupted`, treats a zero-length read as end of input, and
never returns `Pending`. Do not mix it with `feed` on one decoder.

A typed codec reads the events it expects and uses `reject` for values it will
not accept:

```text
def coordinate(decoder: &mut std.json.Decoder) -> Result[i64, JsonError]:
    match decoder.next()?:
        case Event.Number(text):
            match i64.parse(text):
                case Ok(value):
                    Ok(value)
                case Err(_):
                    Err(decoder.reject(JsonFault.NumberOutOfRange))
        case _:
            Err(decoder.reject(JsonFault.UnexpectedByte))
```

### Encoder

`encoder(bounds: &Limits) -> Result[Encoder, JsonError]`. Every writing method
takes an exclusive receiver and returns `Result[(), JsonError]`.

| Method | Writes |
|---|---|
| `null()`, `boolean(value: bool)` | A literal |
| `integer(value: i64)`, `unsigned(value: u64)` | Decimal digits |
| `decimal(value: f64)` | The shortest text that parses back to the same value, as `str.from`; non-finite is `NonFiniteNumber` |
| `number(text: &str)` | The text unchanged, after checking the number grammar |
| `text(value: &str)` | A string |
| `key(name: &str)` | A member name; only valid where an object expects one |
| `start_array()`, `end_array()`, `start_object()`, `end_object()` | Brackets; commas and colons are inserted automatically |
| `event(event: &Event)` | The call matching a decoder event; `Pending` and `End` write nothing |
| `value(value: &Json)` | A whole tree as one value |

| Method | Result | Contract |
|---|---|---|
| `pending()` | `i64` | Bytes produced and not yet taken |
| `written()` | `i64` | Bytes produced in total |
| `is_complete()` | `bool` | Exactly one top-level value has been written and closed |
| `take()` | `Result[Bytes, JsonError]` | Move the pending bytes out |

The encoder accepts only call sequences that form one document: a key outside
an object, a value where a key belongs, a mismatched close, or a second
top-level value is `InvalidState`. It does not check member names for
duplicates.

`write_pending[W: Writer](source: &mut Encoder, writer: &mut W) -> Result[(), JsonError]`
writes every pending byte with `std.io.write_all` and keeps the buffer's
capacity. It does not flush. If the writer fails, the bytes it accepted are
removed and the rest stay pending.

## Value tree

```text
pub enum Json:
    Null
    Bool(bool)
    Int(i64)
    Float(f64)
    Text(str)
    Array(list[Json])
    Object(list[Member])

pub class Member:
    pub key: str
    pub value: Json
```

| Function | Result | Contract |
|---|---|---|
| `parse(source: &Bytes, bounds)` | `Result[Json, JsonError]` | Decode one document from a buffer |
| `parse_text(text: &str, bounds)` | `Result[Json, JsonError]` | The same, from a `str` |
| `read_value[R: Reader](reader: &mut R, bounds)` | `Result[Json, JsonError]` | Decode one document, reading to end of input |
| `encode(value: &Json, bounds)` | `Result[Bytes, JsonError]` | Compact encoding |
| `encode_text(value: &Json, bounds)` | `Result[str, JsonError]` | The same, as a `str` |
| `write_value[W: Writer](writer: &mut W, value: &Json, bounds)` | `Result[(), JsonError]` | Encode the whole value, then write it; no flush |
| `find(members: &list[Member], key: &str)` | `Option[i64]` | Index of the first member named `key` |

A number without a fraction or exponent becomes `Int` and must fit `i64`;
otherwise the document is rejected with `NumberOutOfRange`, never rounded. Any
other number becomes `Float`, the nearest `f64`, and must be finite: `1e309` is
`NumberOutOfRange`, while `1e-400` is `0.0`. `-0` is `Int(0)`; `-0.0` keeps its
sign. `Float` values are written with a fraction or exponent (`100.0`, `1e21`),
so a tree keeps its `Int`/`Float` distinction through a round trip.

Objects keep members in source order, including duplicates under `Keep`.
`find` is a linear scan; build a `dict` when an object is searched often.

## Ownership and threads

A `Json` owns its whole tree; moving it moves everything and dropping it
releases everything. `Json`, `Decoder`, and `Encoder` are single-owner values
with no shared or global state. They can be moved into a worker thread, and the
module's functions pass the ordinary worker-effect checks. One decoder or
encoder cannot be used from two threads at once, since every operation needs an
exclusive loan.

Sources are shared loans: `feed` and `parse` copy what they need and keep no
reference to the caller's buffer. Events own their strings.

`Json` is recursive, so the builtins that require a finite layout walk reject
it: `==`, `copy`, `print`, and `str.repr` do not accept a `Json`. Compare and
display trees through `encode_text`.

## Costs

| Operation | Allocates | Time |
|---|---|---|
| `decoder` | Three empty buffers | Constant |
| `feed` | Grows the window by the range; storage is reused once the window is drained | Linear in the range |
| `next` | One `str` per `Number`, `Text`, or `Key` event; one byte of stack per open container; the scratch buffer grows to the longest token | Linear in the bytes examined |
| `read_event` | A 4096-byte window, once | As `next`, plus the reads |
| `parse`, `read_value` | The events, one list per container, one set of keys per object under `Reject`, and for `parse` one copy of the input | Linear in the input, expected |
| Encoder calls | Grow one output buffer; one byte of stack per open container; number text for `integer`, `unsigned`, `decimal` | Linear in the bytes written |
| `encode`, `write_value` | The whole output buffer | Linear in the output |
| `encode_text` | The output buffer, then a `str` copy | Linear |
| `find` | Nothing | Linear in the member count |

The decoder and the tree builder use no recursion. `Encoder.value` recurses once
per nesting level, at most `max_depth` (1024) deep.

An event loop that drains `next` to `Pending` before each `feed`, or uses
`read_event`, holds only the current chunk, the current token, and the depth
stack, whatever the document size. A tree costs more than its text: each value
is an enum cell and each string and container its own allocation.

The decoder takes a run of unescaped ASCII string content, and the encoder a
run of bytes that need no escape, with one [`std.bytes` scan](bytes.md#scanning)
each. Structural bytes, whitespace, numbers, literals, escapes, and non-ASCII
string bytes still go through a checked single-element `Bytes` call. Measured
with a release compiler on x86-64 Linux, pinned to one core, on an
8,388,872-byte document of 34,393 objects (687,862 events):

| Operation | Before the scans | Now |
|---|---|---|
| `Decoder.next` over the whole buffer | 2.55 s | 1.58 s |
| `parse` | 3.59 s | 2.65 s |
| `parse`, then `encode` of the tree | 4.56 s | 3.28 s |

That is roughly 2 to 5 megabytes per second. The remaining time is the calls
made per token and per event, not byte access.

## Limits

- Compact output only: no pretty-printing or key sorting.
- One document per input: no JSON Lines or concatenated documents. Split the
  input and use one decoder per document.
- The tree holds integers as `i64` and other numbers as `f64`. Larger integers
  and exact decimals are available only as `Event.Number` text.
- `write_value` builds the whole encoding before writing. Use an `Encoder` with
  `write_pending` for bounded output memory.
- No derived codecs: a typed codec is written by hand over events.
- No tree equality or path lookup helpers.
