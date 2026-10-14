# std.csv

`import std.csv` reads and writes delimited records over
[`std.io`](io.md) byte streams. A `Reader` fills one reusable `Record` per
call from a fixed buffer. A `Writer` encodes one record at a time. Neither
loads a whole file, and every size is bounded by the `Dialect`.

The module is checked Plenty over [`std.bytes`](bytes.md) and `std.io`, with no
native calls of its own. It is available on the supported x86-64 Linux GNU
target.

## Format

The grammar is RFC 4180 with a configurable delimiter and quote byte. There
is one accepted form; nothing is trimmed, skipped, or repaired.

- A record ends at LF, CRLF, or CR. The last record may end at end of input.
- Fields are separated by the delimiter. `a,` has two fields, the second empty.
- An empty line is one record with one empty field. Empty input has no records.
- A field that starts with the quote byte is quoted and ends at the next
  unpaired quote. Inside it, the delimiter, CR, and LF are data, and two quotes
  are one quote byte.
- A closing quote must be followed by the delimiter, a record end, or end of
  input.
- A quote byte anywhere else in an unquoted field is an error.
- Spaces are data, including those next to a delimiter or quote.

There are no headers, comments, or escape characters other than the doubled
quote. The first record is an ordinary record.

## Dialect

`Dialect` is a class with public fields. `std.csv.rfc4180()` returns the
values in the third column; change fields before creating a reader or writer.

| Field | Type | `rfc4180()` | Meaning |
|---|---|---|---|
| `delimiter` | `u8` | `44u8` (`,`) | Field separator |
| `quote` | `u8` | `34u8` (`"`) | Quote byte |
| `newline` | `Newline` | `CrLf` | Record end a `Writer` emits: `Lf` or `CrLf` |
| `quoting` | `Quoting` | `Needed` | `Needed` quotes a field that contains the delimiter, the quote, CR, or LF; `Always` quotes every field |
| `width` | `Width` | `First` | Row-width policy, below |
| `skip_bom` | `bool` | `False` | A `Reader` discards a leading UTF-8 byte-order mark |
| `max_field` | `i64` | `65536` | Most bytes in one unescaped field |
| `max_record` | `i64` | `1048576` | Most bytes in the unescaped fields of one record |
| `max_fields` | `i64` | `4096` | Most fields in one record |

`delimiter` and `quote` must be different ASCII bytes other than CR and LF,
each limit at least one, and `Width.Exactly(count)` at least one. Otherwise
`reader` and `writer` return `InvalidDialect`. A reader accepts all three
record ends whatever `newline` says, and ignores `quoting`.

| `Width` | Policy |
|---|---|
| `Any` | Each record keeps its own field count |
| `First` | Every record has the field count of the first |
| `Exactly(count)` | Every record has `count` fields |

Fields are bytes. The module never decodes them on its own: `Record.text`
validates one field as UTF-8 when asked, and a writer emits the bytes it is
given. Other encodings are read with `Record.bytes` and converted by the
program.

## Errors

| `CsvError` | Meaning |
|---|---|
| `Invalid(Fault, Position)` | A reader found malformed or oversized input |
| `Rejected(Fault, record, field)` | A writer refused a field that exceeds a limit |
| `Width(record, expected, found)` | A record's field count breaks the width policy |
| `EmptyRecord(record)` | A writer was asked to end a record with no fields |
| `InvalidUtf8(record, field)` | `Record.text` found bytes that are not UTF-8 |
| `InvalidDialect` | The dialect breaks the rules above |
| `OutOfRange` | A field index, byte range, or buffer capacity is invalid |
| `Stopped` | The reader already failed |
| `Stream(StreamError)` | The inner stream failed |
| `Allocation(AllocError)` | A record or buffer could not grow |

| `Fault` | Reader | Writer |
|---|---|---|
| `BareQuote` | A quote inside an unquoted field | |
| `AfterQuote` | Data between a closing quote and the next delimiter or record end | |
| `UnterminatedQuote` | Input ended inside a quoted field | |
| `FieldTooLong` | A field exceeds `max_field` | The same |
| `RecordTooLong` | A record exceeds `max_record` | The same |
| `TooManyFields` | A record exceeds `max_fields` | The same |

`Position` locates the byte a reader stopped at:

| Field | Counts from | Meaning |
|---|---|---|
| `record` | 1 | The record being read; matches `Record.number()` |
| `field` | 0 | The field being read; matches the index `Record.text` takes |
| `line` | 1 | The physical line, counting record ends and line breaks inside quotes; CRLF counts once |
| `offset` | 0 | The byte offset in the stream; for `UnterminatedQuote`, the input length |

A limit fault points at the first byte that does not fit. Record numbers and
field indexes in the other variants use the same bases.

## Reading

| Function | Result |
|---|---|
| `reader(inner: R, dialect: &Dialect, capacity: i64)` | `Result[Reader[R], CsvError]` |
| `record()` | `Result[Record, CsvError]` |

`reader` takes ownership of `inner`, copies the dialect, and allocates one
buffer of exactly `capacity` bytes, which never grows. A capacity below one is
`OutOfRange`. `inner` needs the `Reader` method of `std.io`. The reader does
its own buffering, so `inner` needs no `BufReader`.

| `Reader[R]` | Result | Contract |
|---|---|---|
| `read(record: &mut Record)` | `Result[bool, CsvError]` | Replace `record` with the next record; `False` is end of input |
| `records()` | `i64` | Records returned so far |
| `line()` | `i64` | The current line, from one |
| `offset()` | `i64` | Bytes consumed from the stream |

`read` retries `Interrupted` and continues across short reads; a record may
span any number of buffer fills. After `False`, the record is empty and every
later call returns `False` without reading the stream again.

A `Width` error is reported after the whole record has been read: `record`
holds it, and the next `read` continues with the following record. Every
other error empties `record` and stops the reader; later calls
return `Stopped`. A stopped reader cannot resynchronize, because a malformed
quote leaves no reliable record boundary.

`inner` is a public field, for calls such as `input.inner.close()`. Reading it
directly skips bytes the reader already holds.

## Records

A `Record` stores the unescaped fields end to end in one buffer and one list
of field ends. A reader overwrites it, keeping its capacity.

| `Record` | Result | Contract |
|---|---|---|
| `len()` | `i64` | Number of fields |
| `number()` | `i64` | The record's number from one; zero before the first read |
| `line()` | `i64` | The line on which the record started |
| `text(index)` | `Result[str, CsvError]` | An owned copy of one field, validated as UTF-8 |
| `texts()` | `Result[list[str], CsvError]` | An owned list of every field as text |
| `bytes(index)` | `Result[Bytes, CsvError]` | An owned copy of one field |
| `append_to(index, out: &mut Bytes)` | `Result[(), CsvError]` | Append one field to the caller's buffer |
| `data()` | `&Bytes` | Every field's bytes, borrowed |
| `start(index)`, `stop(index)` | `Result[i64, CsvError]` | Field `index` is `data()[start:stop]` |
| `clear()` | `()` | Remove the fields; `number()` and `line()` are kept |

An index outside `0 <= index < len()` is `OutOfRange`.

`data()` is the borrowed view. It and the offsets describe the current
contents only. `Reader.read` needs `&mut Record`, so the compiler rejects a
program that still holds the view across the next read. `text`, `texts`,
`bytes`, and `append_to` return or fill storage the caller owns, which
survives any number of reads.

## Writing

| Function | Result |
|---|---|
| `writer(inner: W, dialect: &Dialect)` | `Result[Writer[W], CsvError]` |

`writer` takes ownership of `inner`, which needs the `Writer` methods of
`std.io`, and copies the dialect.

| `Writer[W]` | Result | Contract |
|---|---|---|
| `field(source: &Bytes, start, stop)` | `Result[(), CsvError]` | Add `source[start:stop]` as the next field |
| `text(value: &str)` | `Result[(), CsvError]` | Add the UTF-8 bytes of `value` as the next field |
| `end_record()` | `Result[(), CsvError]` | Finish the record in progress and deliver it |
| `record(record: &Record)` | `Result[(), CsvError]` | Add every field of `record`, then end the record |
| `texts(fields: &list[str])` | `Result[(), CsvError]` | Add every text, then end the record |
| `discard()` | `()` | Drop the record in progress |
| `flush()` | `Result[(), CsvError]` | Deliver held bytes, then flush the inner stream |
| `records()` | `i64` | Records finished so far |
| `pending()` | `i64` | Bytes of finished records the stream has not accepted |

A record in progress lives in the writer's buffer and reaches the stream only
at `end_record`, as one run of writes that retries `Interrupted` and short
counts. A record with one empty field is written as two quote bytes, so that
it is not an empty line. `end_record` with no fields is `EmptyRecord`.

Failures leave a defined state:

- `Rejected`, `OutOfRange`, and `Allocation` from `field` or `text` add
  nothing; the record in progress keeps its earlier fields.
- `Width` from `end_record` discards the record in progress. Nothing of it
  was written. With `Width.First`, the first finished record sets the count.
- `Stream` from `end_record` means the record is finished and counted, and
  its undelivered bytes are held. The next `field`, `end_record`, or `flush`
  delivers them first and fails again if the stream still refuses.
- `record` and `texts` discard the record in progress on any error. A
  `Stream` error still leaves the finished record held.

Nothing is written when a `Writer` is dropped. End the last record and call
`flush`. The writer does not close `inner`; it is the public field `inner`.

## Ownership and threads

A reader or writer owns its stream and has one user at a time: every
operation that moves data takes `&mut self`. A `Record` is an ordinary owned
value, independent of the reader that filled it, and can be moved to another
thread. The module holds no global state and adds no synchronization.

## Costs

Reading examines each input byte once and copies field bytes into the record
in runs. Writing examines each field byte once to decide quoting, and once
more when quoting to double quote bytes. Both are linear in the bytes
processed. Field access by index is constant time.

The reader and writer find the next delimiter, quote, or line break with the
[`std.bytes` scans](bytes.md#scanning), so the work in Plenty code is per
field and per quote, not per byte. Measured with a release compiler on x86-64
Linux, pinned to one core, reading 64 MB from standard input into one reused
`Record`:

| Input | Before the scans | Now |
|---|---|---|
| 3,234,608 fields of about 19 bytes, some quoted | 4.73 s (14 MB/s) | 2.00 s (32 MB/s) |
| 376,760 fields of about 180 bytes, some quoted | 3.37 s (19 MB/s) | 0.30 s (213 MB/s) |

Short fields are bounded by the cost of the calls made per field, about
600 ns, not by scanning.

| Allocates | Does not allocate |
|---|---|
| `reader`: one buffer; `record`, `writer`: empty storage | `Reader.read` once the record's capacity covers the record |
| `Reader.read`, when a record outgrows the `Record` | `Record.append_to` within the capacity of `out` |
| `Record.text` and `bytes`: one copy; `texts`: one list and a copy per field | `Record.data`, `start`, `stop`, `len`, `clear` |
| `Writer.text`: one temporary copy of the text | `Writer.field`, `record`, `end_record`, `flush` within the writer's capacity |
| `Writer.field`, when a record outgrows the writer's buffer | `Writer.discard`, `records`, `pending` |

A `Record` holds at most `max_record` bytes of content and `max_fields`
offsets. A reader consumes at most `2 * max_record + 3 * max_fields + 2`
bytes for one record before returning it or a limit fault. A writer holds at
most one finished record that the stream refused and one record in progress,
each at most that many bytes. Storage grows geometrically, so capacity can be
up to twice the content.

## Limits

- Only x86-64 Linux GNU is supported.
- There is no lenient mode: stray quotes, and data after a closing quote, are
  errors, and a reader does not continue after them.
- There are no header mapping, typed columns, comment lines, or whitespace
  trimming. Convert fields with the type's `parse` method.
- Only the UTF-8 byte-order mark is recognized, and only with `skip_bom`. A
  writer never emits one. Other encodings need conversion by the program.
- `delimiter` and `quote` are single ASCII bytes.
- A stream error that could be retried, such as `EAGAIN` on a nonblocking
  descriptor, stops the reader like any other.
- End of input is final: a reader does not look for data appended later.
- `Writer.text` copies the text once to reach its bytes. Use `field` with a
  reused `Bytes` to avoid the copy.
