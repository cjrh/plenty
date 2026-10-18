# std.toml

`import std.toml` parses [TOML 1.0](https://toml.io/en/v1.0.0) documents into a
value tree and writes a tree back as TOML text, under explicit limits, with one
error type, `TomlError`.

The module is checked Plenty code with no native primitives of its own. It
reads and writes [`std.bytes`](bytes.md) buffers and [`std.io`](io.md) streams,
and is available on the supported x86-64 Linux GNU target.

## Accepted text

The parser accepts TOML 1.0 and rejects everything else, including TOML 1.1
additions.

| Rule | Behavior |
|---|---|
| Encoding | UTF-8, validated strictly before parsing; no byte-order mark |
| Newlines | `\n` or `\r\n`; a `\r` alone is an error everywhere |
| Comments | `#` to end of line; control characters other than tab are `ControlCharacter` |
| Keys | Bare (`A-Za-z0-9_-`), basic-quoted, or literal-quoted parts joined by `.`, with optional spaces or tabs around each `.` |
| Strings | Basic, literal, multi-line basic, and multi-line literal. A newline right after an opening `"""` or `'''` is dropped; `\r\n` inside a multi-line string is kept as written |
| Escapes | `\b \t \n \f \r \" \\ \uXXXX \UXXXXXXXX`, hex digits of either case, Unicode scalar values only. In a multi-line basic string, `\` before a line end removes the line end and the whitespace and newlines that follow |
| Control characters | Bytes 0-8, 10-31, and 127 are rejected in every string and comment, except newlines in multi-line strings |
| Integers | Decimal with optional sign, or `0x`, `0o`, `0b` with lowercase prefix and no sign. `_` only between digits; no leading zeros. Must fit `i64` |
| Floats | Integer part, then a fraction, an exponent, or both; `inf`, `nan` with optional sign. Must be finite unless written as `inf` or `nan` |
| Booleans | `true`, `false` |
| Date-times | RFC 3339 with `T`, `t`, or one space between date and time; seconds required; `Z`, `z`, or `±HH:MM` offset. Fractional digits beyond nanoseconds are truncated. Year `0000`-`9999`; the day must exist in its month; second `60` is accepted at any time |
| Arrays | Mixed element types, newlines and comments between elements, optional trailing comma |
| Inline tables | One line, no trailing comma; complete as written |

Tables follow the definition rules of the specification:

| Situation | Fault |
|---|---|
| A key assigned twice in one table | `DuplicateKey` |
| A `[table]` header repeated, or naming a table already made by dotted keys or an inline table | `DuplicateTable` |
| A header or dotted key that passes through a value, an inline table, or an array that is not an array of tables | `KeyConflict` |
| A dotted key that adds to a table made by a `[header]` other than the current one | `KeyConflict` |
| `[[name]]` where `name` is not an array of tables | `KeyConflict` |

A header may name a table that an earlier header created implicitly, such as
`[a]` after `[a.b]`, once. A header path that passes through an array of
tables continues in its last table.

## Value tree

```text
pub enum Toml:
    Bool(bool)
    Int(i64)
    Float(f64)
    Text(str)
    DateTime(DateTime)
    Array(list[Toml])
    Table(list[Entry])

pub class Entry:
    pub key: str
    pub value: Toml

pub enum DateTimeKind:
    OffsetDateTime
    LocalDateTime
    LocalDate
    LocalTime

pub class DateTime:
    pub kind: DateTimeKind
    pub year: i64
    pub month: i64
    pub day: i64
    pub hour: i64
    pub minute: i64
    pub second: i64
    pub nanosecond: i64
    pub offset_minutes: i64
```

A document is a `Toml.Table`. Tables keep their entries in the order each key
first appeared in the text, including keys added later by a header, so
`[a.b]` before `[a]` gives `a` the entry `b` first. An array of tables is an
`Array` of `Table` values. `DateTime` fields that its kind does not use are 0;
`offset_minutes` is minutes east of UTC, and `Z` is 0.

| Function | Result | Contract |
|---|---|---|
| `limits()` | `Limits` | The defaults below |
| `parse(source: &Bytes, bounds: &Limits)` | `Result[Toml, TomlError]` | Parse one document from a buffer |
| `parse_text(text: &str, bounds)` | `Result[Toml, TomlError]` | The same, from a `str` |
| `read_value[R: Reader](reader: &mut R, bounds)` | `Result[Toml, TomlError]` | Read to end of input, then parse |
| `encode(value: &Toml, bounds)` | `Result[Bytes, TomlError]` | TOML text for a table |
| `encode_text(value: &Toml, bounds)` | `Result[str, TomlError]` | The same, as a `str` |
| `write_value[W: Writer](writer: &mut W, value: &Toml, bounds)` | `Result[(), TomlError]` | Encode the whole value, then write it; no flush |
| `find(entries: &list[Entry], key: &str)` | `Option[i64]` | Index of the first entry named `key` |

`read_value` reads chunks of up to 4096 bytes, retries `Interrupted`, and
treats a zero-length read as end of input. It reads at most one byte past
`max_input` before reporting `InputTooLarge`.

## Writing

The writer produces text that parses back to the same values:

- Each table lists its plain entries as `key = value` lines first, then each
  table entry as a `[path]` section and each non-empty array whose elements are
  all tables as `[[path]]` sections. A section header is omitted for a
  non-empty table whose entries are all sections. Inside one group, entries
  keep their order. The plain-first order is required: a plain key written
  after a header would belong to that header's table.
- Other arrays and tables nested in them are written inline on one line.
- Keys are bare when they are non-empty and use only bare-key bytes; otherwise
  they are basic strings.
- Strings are basic strings. `"`, `\`, and control characters are escaped
  (`\b \t \n \f \r`, otherwise `\u00XX` in uppercase hex); every other scalar
  is written as UTF-8.
- Floats are the shortest text that parses back to the same value, as
  `str.from`, with `.0` added when that text has no point or exponent;
  infinities and NaN are `inf`, `-inf`, and `nan`.
- Date-times use `T`, `Z` for offset 0, and the shortest fraction that keeps
  the nanoseconds.

Comments, blank lines, key quoting, number spellings, and inline-or-section
choices of the source are not kept. The writer rejects a root that is not a
table (`NotATable`), a table with a repeated key (`DuplicateKey`), and a
`DateTime` whose fields are out of range or whose unused fields are not 0
(`InvalidDateTime`).

## Limits

```text
pub class Limits:
    pub max_depth: i64
    pub max_input: i64
    pub max_output: i64
```

`limits()` returns `Limits(64, 67108864, 67108864)`. The fields are public;
adjust a copy. Every function takes `&Limits` and returns `InvalidLimits` unless
`0 <= max_depth <= 1024`, `max_input >= 0`, and `max_output >= 0`.

| Field | Bounds | When exceeded |
|---|---|---|
| `max_depth` | Tables and arrays nested inside the document's root table. `0` allows only plain values at the root. An array of tables counts two levels: the array and each table | `DepthExceeded` |
| `max_input` | Bytes in one document | `InputTooLarge`; the offset is the limit |
| `max_output` | Bytes one encoding produces | `OutputTooLarge` |

The depth limit also bounds the parser's and writer's recursion, so neither a
document nor a tree built by hand can overflow the stack.

## Errors

```text
pub class TomlError:
    pub fault: TomlFault
    pub offset: i64
    pub line: i64
    pub column: i64
```

For parsing, `offset` is the byte offset from 0, and `line` and `column` start
at 1. Only `\n` ends a line. A column is one Unicode scalar, not one byte. A
syntax fault points at the offending byte. A fault about a whole key or header
(`DuplicateKey`, `DuplicateTable`, `KeyConflict`, and a `DepthExceeded` that a
key or header causes) points at the first byte of its key or `[`. A
`DepthExceeded` that an array or inline table causes points at its opening
bracket, and a fault about a number or date-time at the value's first byte.
`UnexpectedEnd` points one past the last byte. An allocation failure before
scanning starts points at offset 0, line 1, column 1; one while assembling the
value after a successful scan points one past the last byte.

For writing and for stream failures, `offset` is the number of bytes produced,
written, or read when the call failed, and `line` and `column` are 0.
`InvalidLimits` and `NotATable` report 0, 0, 0.

| `TomlFault` | Meaning |
|---|---|
| `UnexpectedByte` | A byte that cannot start or continue the document here |
| `UnexpectedEnd` | Input ended inside a key, value, or header |
| `InvalidUtf8` | Malformed, overlong, surrogate, or out-of-range UTF-8, anywhere |
| `ControlCharacter` | A forbidden control character in a string, key, or comment |
| `InvalidEscape` | An unknown escape, a bad hex digit, or an escape that is not a Unicode scalar |
| `InvalidNumber` | A token that breaks the integer or float grammar |
| `NumberOutOfRange` | An integer outside `i64` or a float outside finite `f64` |
| `InvalidDateTime` | A malformed date-time, or one naming a day or time that does not exist |
| `DuplicateKey` | A key defined twice in one table |
| `DuplicateTable` | A table defined twice |
| `KeyConflict` | A key or header that would extend a value or a closed table |
| `DepthExceeded` | Nesting beyond `max_depth` |
| `InputTooLarge`, `OutputTooLarge` | A size limit was reached |
| `NotATable` | `encode` was given a root that is not a table |
| `InvalidLimits` | `Limits` outside the valid ranges |
| `OutOfRange` | An internal range check failed; not expected |
| `Stream(StreamError)` | A reader or writer failed; `InvalidProgress` if it reported an impossible count |
| `Allocation(AllocError)` | Memory was refused |

Partial trees and buffers are released before an error returns.

## Ownership and threads

A `Toml` owns its whole tree; moving it moves everything and dropping it
releases everything. There is no shared or global state, so trees can be moved
into a worker thread. `parse` copies its source and keeps no reference to it.

`Toml` is recursive, so the builtins that require a finite layout walk reject
it: `==`, `copy`, `print`, and `str.repr` do not accept a `Toml`. Compare and
display trees through `encode_text`. `DateTime` is an ordinary class and can be
printed and compared.

## Costs

| Operation | Allocates | Time |
|---|---|---|
| `parse`, `parse_text` | One copy of the input; one node per key, table, and value; one index entry per named key; one scratch buffer as long as the longest key, string, or number; then the tree | Linear in the input, expected |
| `read_value` | The input buffer, grown in chunks of 4096 bytes, then as `parse` without the copy | As `parse`, plus the reads |
| `encode`, `write_value` | The whole output buffer; one key set per table with two or more entries; a temporary copy of each key and string and a temporary `str` per float, each released before the next | Linear in the output, expected |
| `encode_text` | The output buffer, then a `str` copy | Linear |
| `find` | Nothing | Linear in the entry count |

The parser takes runs of plain string, comment, and bare-key bytes with one
[`std.bytes` scan](bytes.md#scanning) each. A table lookup goes through an index
keyed by parent and name, so a table with many keys costs the same per key as
a small one. Measured with a release compiler on x86-64 Linux, parsing and then
writing a 2,688,896-byte document of 20,000 `[[item]]` tables and one table of
20,000 keys takes about 2 seconds, close to `std.json` on input of the same
size and shape.

## Limits of this module

- No comment- or layout-preserving editing: rewriting a file a person
  maintains loses its comments and formatting.
- No event or streaming interface: the whole document and its tree are held in
  memory. TOML's tables can be extended anywhere in a file, so no part of the
  tree is final before the end of input.
- Integers are `i64` and floats `f64`, as TOML 1.0 requires of
  implementations; a larger integer is an error, never rounded.
- No tree equality, path lookup, or typed decoding helpers; a typed reader is
  written by hand over the tree with `find`.
- TOML 1.1 additions (newlines and trailing commas in inline tables, `\e`,
  `\xHH`, optional seconds) are rejected.
