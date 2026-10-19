# Read and write CSV records

`std.csv` reads a table one record at a time, so a large file never has to fit
in memory. A reader sits on any [byte stream](26-byte-streams.md) and fills the
same `Record` on every call:

```plenty
import std.bytes
import std.csv
import std.io

type CsvInputError = ParseError | std.csv.CsvError | std.bytes.BytesError

def main() -> Result[(), AllocError | IoError | CsvInputError]:
    text = "item,price,note\r\npen,3,\"blue, fine\"\r\nbook,12,\"says \"\"read me\"\"\"\r\ncard,2,\"two\nlines\"\r\n"
    mut input = std.csv.reader(std.io.cursor(std.bytes.from_text(text)?), &std.csv.rfc4180(), 16)?
    mut record = std.csv.record()?
    input.read(&mut record)?
    print(str.repr(record.texts()?)?)?
    mut total = 0
    while input.read(&mut record)?:
        total += i64.parse(&record.text(1)?)?
        print(record.number(), record.line(), record.text(0)?, str.repr(record.text(2)?)?)?
    print(total, input.records())?
    Ok(())
```
```output
["item", "price", "note"]
2 2 pen "blue, fine"
3 3 book "says \"read me\""
4 4 card "two\nlines"
17 4
```

`read` returns `True` while there is another record and `False` at the end of
the input. The reader's buffer is the 16 bytes passed to `reader`; records
longer than that are assembled across several fills. Quoted fields may hold
the delimiter, line breaks, and doubled quotes, and arrive without their
quoting. The first record is not special, so the program reads the header
itself.

Fields are bytes. `text(index)` checks one field as UTF-8 and returns a copy
you own; `texts()` copies them all. The record itself is overwritten by the
next `read`, which is why `read` takes it as `&mut`.

`rfc4180()` is the usual dialect: comma, double quote, and every record as
wide as the first. A record of another width is reported, but it has been read
and the reader carries on. Malformed input stops the reader for good:

```plenty
import std.bytes
import std.csv
import std.io
from std.csv import CsvError

def main() -> Result[(), AllocError | IoError | CsvError | std.bytes.BytesError]:
    text = "a,b\nc\nd,e\nf,g\"h\ni,j\n"
    mut input = std.csv.reader(std.io.cursor(std.bytes.from_text(text)?), &std.csv.rfc4180(), 16)?
    mut record = std.csv.record()?
    mut more = True
    while more:
        outcome = input.read(&mut record)
        match outcome:
            case Ok(found):
                if found:
                    print(str.repr(record.texts()?)?)?
                else:
                    more = False
            case Err(cause):
                print(str.repr(&cause)?)?
                match cause:
                    case CsvError.Width(_, _, _):
                        print("skipped", str.repr(record.texts()?)?)?
                    case _:
                        more = False
    print(str.repr(input.read(&mut record))?)?
    Ok(())
```
```output
["a", "b"]
std.csv.CsvError.Width(2, 2, 1)
skipped ["c"]
["d", "e"]
std.csv.CsvError.Invalid(std.csv.Fault.BareQuote, std.csv.Position(record=4, field=1, line=4, offset=13))
Result[bool, std.csv.CsvError].Err(std.csv.CsvError.Stopped)
```

`Width(2, 2, 1)` says record 2 should have two fields and has one. The
`Position` names the record, the field index, the line, and the byte offset of
the stray quote. Record `i,j` is never reached: after a broken quote there is
no trustworthy place to resume.

A writer builds one record from fields and delivers it at `end_record`.
`texts` does both for a list. Change the dialect's public fields before
creating the writer:

```plenty
import std.bytes
import std.csv
import std.io
from std.csv import Newline

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.csv.CsvError]:
    mut dialect = std.csv.rfc4180()
    dialect.newline = Newline.Lf
    dialect.max_field = 12
    mut output = std.csv.writer(std.io.stdout(), &dialect)?
    output.texts(&["item", "note"]?)?
    output.texts(&["pen", "blue, fine"]?)?
    output.text("book")?
    output.text("says \"hi\"")?
    output.end_record()?
    print(str.repr(output.texts(&["card"]?))?)?
    print(str.repr(output.text("much too long for a field"))?)?
    raw = std.bytes.from_text("card:plain")?
    output.field(&raw, 0, 4)?
    output.field(&raw, 5, 10)?
    output.end_record()?
    output.flush()?
    print(output.records())?
    Ok(())
```
```output
item,note
pen,"blue, fine"
book,"says ""hi"""
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Width(4, 2, 1))
Result[(), std.csv.CsvError].Err(std.csv.CsvError.Rejected(std.csv.Fault.FieldTooLong, 4, 0))
card,plain
4
```

Only fields that need quotes get them. The one-field record is refused by the
width policy and the oversized text by `max_field`; neither writes anything.
`field` adds a byte range without building a string first. Call `flush` when
you are done: a writer does not flush or close its stream when it goes out of
scope.

Limits apply to reading too. A field over `max_field` bytes or a record over
`max_record` is an error at the byte that does not fit, so a file with a
missing closing quote cannot consume all memory.

See the [module reference](../../design/stdlib/csv.md) for the exact grammar,
every error, the borrowed `data()` view, and what allocates.
