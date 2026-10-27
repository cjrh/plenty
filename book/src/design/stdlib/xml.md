# std.xml

`std.xml` reads and writes bounded XML 1.0 events over [`std.io`](io.md)
byte streams. It is implemented in Plenty so buffer, string, attribute-list,
and nesting-stack allocation failures remain explicit. It keeps a current
token and a stack of open names, without building a document tree.

## Events and format

`Attribute(name: str, value: str)` has public fields. Events own their strings
and attribute lists; they remain valid after another reader call.

| `Event` | Meaning |
|---|---|
| `Declaration(Option[bool])` | XML 1.0 declaration; optional standalone yes/no |
| `Start(str, list[Attribute])` | Open an element with ordered attributes |
| `Empty(str, list[Attribute])` | Empty element, without a separate end event |
| `End(str)` | Close the matching element |
| `Text(str)` | Decoded character data, including whitespace |
| `CData(str)` | CDATA content without delimiters |
| `Comment(str)` | Comment content without delimiters |
| `ProcessingInstruction(str, str)` | Target and data; separator whitespace is omitted |
| `Eof` | Complete, well-formed document |

Input must be UTF-8, optionally with a leading UTF-8 BOM. A declaration is
optional; when present it must be first, specify version `1.0`, and use UTF-8
if it names an encoding. The standalone flag is retained. There must be
exactly one root element; comments, processing instructions, and XML whitespace
may surround it. Names use XML 1.0 Fifth Edition character ranges. Duplicate
attribute names and mismatched closing names are errors.

Literal CRLF and CR normalize to LF. Literal whitespace in attributes becomes
spaces. The five predefined entities (`amp`, `lt`, `gt`, `apos`, `quot`) and
decimal/hexadecimal character references are decoded in text and attributes;
references to tab, LF, or CR in attributes retain those characters. Comments,
CDATA, and processing instructions do not expand references. XML-forbidden
characters, malformed UTF-8, and malformed references are rejected.

DTD declarations, including internal subsets and external entities, are
rejected. The module never loads an external resource. Names with colons and
`xmlns` attributes are preserved literally: it does not resolve namespaces or
check namespace bindings. It does not validate against a schema or DTD, build
a DOM, query XPath, canonicalize XML, or support encodings other than UTF-8.
Events preserve content rather than original spelling, quote choices, entity
spellings, or declaration formatting.

## Limits

`limits()` returns a `Limits` with public fields. Constructors copy the limits;
changing the original afterwards has no effect.

| Field | Default | Meaning |
|---|---:|---|
| `max_depth` | 64 | Open nesting including an empty element; permitted range 0–1024 |
| `max_input` | 67108864 | Total input bytes consumed, including BOM |
| `max_output` | 67108864 | Total encoded bytes written |
| `max_token` | 1048576 | Raw bytes per reader event, or encoded bytes per writer event, including markup |
| `max_attributes` | 4096 | Attributes per element |

Byte and attribute totals must be nonnegative and `max_token` at least one.
A zero depth permits no root and therefore no complete document. The input
limit applies to readers and the output limit to writers. The XML declaration
has its own fixed grammar of at most three fields. The BOM is checked against
the token limit separately from the first event.

A reader uses a fixed 4096-byte read buffer and may read ahead that far. Size
limits constrain consumption, rather than the inner stream's position. Text
between markup tokens is one event and must fit `max_token`; it is not split
into arbitrarily sized chunks. Memory is bounded by the buffer, current event,
and at most `max_depth` retained names, plus events retained by the caller.
These limits bound input-controlled work and storage, not elapsed time or a
stream that repeatedly reports interruption.

## Reading and writing

`reader[R](inner: R, bounds: &Limits) -> Result[Reader[R], XmlError]` owns a
stream with a public `read` method matching `std.io.Reader`.
`Reader.next(&mut self) -> Result[Event, XmlError]` retries interrupted reads,
checks reported progress, and returns `Eof` repeatedly after successful EOF.
`Reader.offset()` counts consumed input bytes, before line normalization.

`writer[W](inner: W, bounds: &Limits) -> Result[Writer[W], XmlError]` owns a
stream with public `write` and `flush` methods matching `std.io.Writer`.
`Writer.write(&mut self, event: &Event) -> Result[(), XmlError]` validates and
encodes one event before writing it. It checks nesting, root count, attribute
uniqueness, names, characters, and event ordering. Text and attribute values
are escaped; attribute tab/LF/CR and text CR inside elements are written as
character references. Comments containing `--` or ending in `-`, CDATA
containing `]]>`, and processing instructions containing `?>` are rejected.
Raw CR in comments, CDATA, processing instructions, or document whitespace
still follows XML line normalization when read back.

`Writer.finish(&mut self) -> Result[(), XmlError]` verifies that the root was
completed and flushes the stream. It is idempotent after success. Writing
`Eof` calls `finish`; further writes are rejected. `Writer.offset()` counts
bytes accepted by the inner writer. Both wrappers expose their owned `inner`
stream; avoid reading or writing it directly while using the wrapper, since
that bypasses the wrapper's position and document state.

A writer can stream a reader's events straight through, but this is not an
atomic file replacement: earlier events and a partially written event remain
visible on failure. Use a separately managed temporary destination when that
matters. See [the XML lesson](../../tutorial/08-resources/28-xml-events.md).

## Errors

`XmlError` has public fields `fault: Fault` and `offset: i64`. The offset is
zero-based consumed input or accepted output bytes at detection. Validation
may detect a malformed token after consuming it; this is not necessarily the
byte containing the original mistake.

`Fault` distinguishes syntax, unexpected EOF, invalid names/characters/UTF-8,
invalid references, unknown entities, duplicate attributes, mismatched ends,
forbidden doctypes, unsupported declarations, each resource bound, invalid
limits, invalid writer state, and `Stopped`. `Allocation(AllocError)` and
`Stream(StreamError)` retain their causes, including stream allocation errors.
All allocation in the module is fallible. A malformed or oversized reader
event, failed writer event, or failed finish permanently stops that wrapper;
subsequent operations return `Stopped`. Successfully finished writers instead
reject additional writes with `InvalidState`.

## Acknowledgements

- [W3C XML 1.0 Fifth Edition](https://www.w3.org/TR/xml/) supplies the character,
  name, document, reference, and normalization rules implemented independently.
- [quick-xml](https://docs.rs/quick-xml/) informs the streaming event API;
  the module does not wrap or ship its code.
