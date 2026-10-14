//! `std.toml`: the document parser, the value tree, and the writer, checked
//! against TOML 1.0 specification examples, hand-written expectations, and
//! generated documents.

/// Helpers shared by every program. Inputs arrive as hex so a case can hold
/// any byte sequence. `check` prints the parsed tree, or the located error,
/// and reports any short-read stream that disagrees with the whole buffer.
const HEADER: &str = r#"import std.bytes
import std.encoding
import std.io
import std.toml
from std.bytes import Bytes
from std.io import StreamError
from std.toml import DateTime, DateTimeKind, Entry, Limits, Toml, TomlError, TomlFault

pub class Trickle:
    pub data: Bytes
    pub at: i64
    pub step: i64

    pub def read(self: &mut Trickle, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        mut count = stop - start
        if count > self.step:
            count = self.step
        if count > self.data.len() - self.at:
            count = self.data.len() - self.at
        match buffer.overwrite(start, &self.data, self.at, self.at + count):
            case Ok(_):
                pass
            case Err(_):
                return Err(StreamError.OutOfRange)
        self.at += count
        Ok(count)

pub class Sink:
    pub data: Bytes
    pub room: i64

    pub def write(self: &mut Sink, buffer: &Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        mut count = stop - start
        if count > 3:
            count = 3
        if count > self.room:
            count = self.room
        if count == 0:
            return Ok(0)
        match self.data.extend_range(buffer, start, start + count):
            case Ok(_):
                pass
            case Err(_):
                return Err(StreamError.OutOfRange)
        self.room -= count
        Ok(count)

    pub def flush(self: &mut Sink) -> Result[(), StreamError]:
        Ok(())

# Fails or misbehaves once, at read `when`, according to `mode`: 0 is an
# interruption, 1 a device error, 2 an impossible count.
pub class Faulty:
    pub data: Bytes
    pub at: i64
    pub reads: i64
    pub when: i64
    pub mode: i64

    pub def read(self: &mut Faulty, buffer: &mut Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        self.reads += 1
        if self.reads == self.when:
            if self.mode == 0:
                return Err(StreamError.Interrupted)
            if self.mode == 1:
                return Err(StreamError.UnexpectedEof)
            return Ok(stop - start + 1)
        mut count = stop - start
        if count > 2:
            count = 2
        if count > self.data.len() - self.at:
            count = self.data.len() - self.at
        match buffer.overwrite(start, &self.data, self.at, self.at + count):
            case Ok(_):
                pass
            case Err(_):
                return Err(StreamError.OutOfRange)
        self.at += count
        Ok(count)

def unhex(digits: &str) -> Result[Bytes, Failure]:
    Ok(std.encoding.hex_from_text(digits)?)

def placed(error: &TomlError) -> Result[str, Failure]:
    Ok(" ".join(&[str.repr(&error.fault)?, str.from(error.offset)?, str.from(error.line)?, str.from(error.column)?]?)?)

def padded(value: i64, width: i64) -> Result[str, Failure]:
    mut text = str.from(value)?
    while text.byte_len() < width:
        text = ("0" + text)?
    Ok(text)

def stamp(moment: &DateTime) -> Result[str, Failure]:
    date = "-".join(&[padded(moment.year, 4)?, padded(moment.month, 2)?, padded(moment.day, 2)?]?)?
    time = ":".join(&[padded(moment.hour, 2)?, padded(moment.minute, 2)?, padded(moment.second, 2)?]?)?
    fields = " ".join(&[date, time, str.from(moment.nanosecond)?, str.from(moment.offset_minutes)?]?)?
    match moment.kind:
        case DateTimeKind.OffsetDateTime:
            Ok("".join(&["odt(", fields, ")"]?)?)
        case DateTimeKind.LocalDateTime:
            Ok("".join(&["ldt(", fields, ")"]?)?)
        case DateTimeKind.LocalDate:
            Ok("".join(&["ld(", fields, ")"]?)?)
        case DateTimeKind.LocalTime:
            Ok("".join(&["lt(", fields, ")"]?)?)

# Whether the writer gives `value` its own section: a table, or a non-empty
# array of tables only.
def sectioned(value: &Toml) -> bool:
    match value:
        case Toml.Table(_):
            True
        case Toml.Array(items):
            if len(items) == 0:
                return False
            for item in items:
                match item:
                    case Toml.Table(_):
                        pass
                    case _:
                        return False
            True
        case _:
            False

# A compact, typed rendering: floats always show a point or exponent, and
# strings are quoted with escapes.
def show(value: &Toml) -> Result[str, Failure]:
    rendered(value, False)

# With `ordered`, each table lists its plain entries before its sections, as
# the writer does, so a written and reread tree renders the same.
def rendered(value: &Toml, ordered: bool) -> Result[str, Failure]:
    match value:
        case Toml.Bool(flag):
            if *flag:
                return Ok("true")
            Ok("false")
        case Toml.Int(number):
            Ok(str.from(*number)?)
        case Toml.Float(number):
            text = str.from(*number)?
            if "." in text or "e" in text or "N" in text or "n" in text:
                return Ok(text)
            Ok((text + ".0")?)
        case Toml.Text(text):
            Ok(str.repr(text)?)
        case Toml.DateTime(moment):
            stamp(moment)
        case Toml.Array(items):
            mut parts = list[str]()?
            for item in items:
                parts.append(rendered(item, ordered)?)?
            Ok("".join(&["[", ",".join(&parts)?, "]"]?)?)
        case Toml.Table(entries):
            mut parts = list[str]()?
            for pass_ in range(2):
                for entry in entries:
                    if not ordered or (pass_ == 1) == sectioned(&entry.value):
                        if pass_ == 0 or ordered:
                            parts.append("".join(&[str.repr(&entry.key)?, "=", rendered(&entry.value, ordered)?]?)?)?
            Ok("".join(&["{", ",".join(&parts)?, "}"]?)?)

def described(result: Result[Toml, TomlError]) -> Result[str, Failure]:
    match result:
        case Ok(value):
            show(&value)
        case Err(error):
            placed(&error)

def trickled(source: &Bytes, bounds: &Limits, step: i64) -> Result[str, Failure]:
    mut reader = Trickle(source.copy()?, 0, step)
    described(std.toml.read_value(&mut reader, bounds))

# Encoding a parsed tree and parsing the text again gives the same tree, and
# encoding that gives the same text.
def stable(value: &Toml, limited: &Limits) -> Result[str, Failure]:
    mut bounds = std.toml.limits()
    bounds.max_depth = limited.max_depth
    first = std.toml.encode_text(value, &bounds)?
    again = std.toml.parse_text(&first, &bounds)?
    if show(&again)? != rendered(value, True)?:
        return Ok(" ".join(&["REPARSE", show(&again)?, "FROM", str.repr(&first)?]?)?)
    second = std.toml.encode_text(&again, &bounds)?
    if second != first:
        return Ok(" ".join(&["UNSTABLE", str.repr(&second)?]?)?)
    Ok("")

def check(digits: &str, bounds: &Limits) -> Result[(), Failure]:
    source = unhex(digits)?
    parsed = std.toml.parse(&source, bounds)
    mut whole = ""
    mut note = ""
    match &parsed:
        case Ok(value):
            whole = show(value)?
            note = stable(value, bounds)?
        case Err(error):
            whole = placed(error)?
    one = trickled(&source, bounds, 1)?
    if one != whole:
        print("STREAM-1", one)?
    seven = trickled(&source, bounds, 7)?
    if seven != whole:
        print("STREAM-7", seven)?
    if note != "":
        print(note)?
    print(whole)?
    Ok(())

def encoded(value: &Toml, bounds: &Limits) -> Result[str, Failure]:
    match std.toml.encode_text(value, bounds):
        case Ok(text):
            Ok(str.repr(&text)?)
        case Err(error):
            placed(&error)
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}\ndef main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

/// Runs a program and shortens the qualified names in rendered values.
fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.toml.TomlFault.", "")
        .replace("std.io.StreamError.", "")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One `check` outcome per input, under limits prepared by `setup`.
fn outcomes(inputs: &[Vec<u8>], setup: &str) -> Vec<String> {
    let cases = inputs
        .iter()
        .map(|input| format!("\"{}\"", hex(input)))
        .collect::<Vec<_>>()
        .join(", ");
    let body = format!(
        "mut bounds = std.toml.limits()\n{setup}\ncases = [{cases}]?\nfor digits in &cases:\n    check(digits, &bounds)?\n"
    );
    let text = output(&program(&body));
    let lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(lines.len(), inputs.len(), "{text}");
    lines
}

fn table(cases: &[(&str, &str)], setup: &str) {
    let cases = cases
        .iter()
        .map(|(input, expected)| (input.as_bytes(), *expected))
        .collect::<Vec<_>>();
    byte_table(&cases, setup);
}

fn byte_table(cases: &[(&[u8], &str)], setup: &str) {
    let inputs = cases
        .iter()
        .map(|(input, _)| input.to_vec())
        .collect::<Vec<_>>();
    let actual = outcomes(&inputs, setup);
    let mut failures = Vec::new();
    for ((input, expected), actual) in cases.iter().zip(&actual) {
        if actual != expected {
            failures.push(format!(
                "{:?}\n  expected {expected}\n  actual   {actual}",
                String::from_utf8_lossy(input)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn keys_strings_and_tables_build_the_documented_tree() {
    table(
        &[
            ("", "{}"),
            ("# only a comment\n\n", "{}"),
            ("key = \"value\"", r#"{"key"="value"}"#),
            ("bare_key = 1\nbare-key = 2\n1234 = 3", r#"{"bare_key"=1,"bare-key"=2,"1234"=3}"#),
            ("\"127.0.0.1\" = \"v\"\n\"ʎǝʞ\" = 1\n'quoted \"value\"' = 2", r#"{"127.0.0.1"="v","ʎǝʞ"=1,"quoted \"value\""=2}"#),
            ("\"\" = \"blank\"", r#"{""="blank"}"#),
            ("name = \"Orange\"\nphysical.color = \"orange\"\nphysical.shape = \"round\"\nsite.\"google.com\" = true",
             r#"{"name"="Orange","physical"={"color"="orange","shape"="round"},"site"={"google.com"=true}}"#),
            ("fruit . name = \"banana\"\nfruit. color = \"yellow\"", r#"{"fruit"={"name"="banana","color"="yellow"}}"#),
            ("3.14159 = \"pi\"", r#"{"3"={"14159"="pi"}}"#),
            ("[table-1]\nkey1 = \"some string\"\n\n[table-2]\nkey1 = \"another\"",
             r#"{"table-1"={"key1"="some string"},"table-2"={"key1"="another"}}"#),
            ("[dog.\"tater.man\"]\ntype.name = \"pug\"", r#"{"dog"={"tater.man"={"type"={"name"="pug"}}}}"#),
            ("[ g .  h  . i ]\n[ j . \"ʞ\" . 'l' ]", r#"{"g"={"h"={"i"={}}},"j"={"ʞ"={"l"={}}}}"#),
            ("[x.y.z.w]\n[x]", r#"{"x"={"y"={"z"={"w"={}}}}}"#),
            ("[fruit]\napple.color = \"red\"\napple.taste.sweet = true\n[fruit.apple.texture]\nsmooth = true",
             r#"{"fruit"={"apple"={"color"="red","taste"={"sweet"=true},"texture"={"smooth"=true}}}}"#),
            ("str = \"I'm a string. \\\"You can quote me\\\". Name\\tJos\\u00E9\\nLocation\\tSF.\"",
             r#"{"str"="I'm a string. \"You can quote me\". Name\tJosé\nLocation\tSF."}"#),
            ("s = \"\\b\\f\\r\\\\\\U0001F600\"", r#"{"s"="\u{8}\u{c}\r\\😀"}"#),
            ("str1 = \"\"\"\nRoses are red\nViolets are blue\"\"\"", r#"{"str1"="Roses are red\nViolets are blue"}"#),
            ("s = \"\"\"\r\nOne\r\nTwo\"\"\"", r#"{"s"="One\r\nTwo"}"#),
            ("str = \"\"\"\nThe quick brown \\\n\n\n  fox jumps over \\\n    the lazy dog.\"\"\"",
             r#"{"str"="The quick brown fox jumps over the lazy dog."}"#),
            ("str = \"\"\"\\\n       The quick brown \\\n       fox.\\\n       \"\"\"", r#"{"str"="The quick brown fox."}"#),
            ("s = \"\"\"a \\  \n  b\"\"\"", r#"{"s"="a b"}"#),
            ("str4 = \"\"\"Here are two quotation marks: \"\". Simple enough.\"\"\"",
             r#"{"str4"="Here are two quotation marks: \"\". Simple enough."}"#),
            ("str7 = \"\"\"\"This,\" she said, \"is just a pointless statement.\"\"\"\"",
             r#"{"str7"="\"This,\" she said, \"is just a pointless statement.\""}"#),
            ("s = \"\"\"\"\"\"\"\"", r#"{"s"="\"\""}"#),
            ("winpath = 'C:\\Users\\nodejs\\templates'\nregex = '<\\i\\c*\\s*>'",
             r#"{"winpath"="C:\\Users\\nodejs\\templates","regex"="<\\i\\c*\\s*>"}"#),
            ("re = '''I [dw]on't need \\d{2} apples'''\nlines = '''\nThe first newline is\ntrimmed in raw strings.\n'''",
             r#"{"re"="I [dw]on't need \\d{2} apples","lines"="The first newline is\ntrimmed in raw strings.\n"}"#),
            ("apos15 = \"Here are fifteen apostrophes: '''''''''''''''\"\nq = '''''That,' she said.''''",
             r#"{"apos15"="Here are fifteen apostrophes: '''''''''''''''","q"="''That,' she said.'"}"#),
            ("tab = \"a\tb\"\nlit = 'a\tb'", r#"{"tab"="a\tb","lit"="a\tb"}"#),
            ("k = \"é\" # comment with é and \t tab", r#"{"k"="é"}"#),
            ("a = 1\r\nb = 2\r\n", r#"{"a"=1,"b"=2}"#),
            ("\t a \t= \t1\t", r#"{"a"=1}"#),
        ],
        "",
    );
}

#[test]
fn numbers_booleans_and_date_times_follow_the_specification() {
    table(
        &[
            ("int1 = +99\nint2 = 42\nint3 = 0\nint4 = -17", r#"{"int1"=99,"int2"=42,"int3"=0,"int4"=-17}"#),
            ("int5 = 1_000\nint6 = 5_349_221\nint7 = 53_49_221\nint8 = 1_2_3_4_5", r#"{"int5"=1000,"int6"=5349221,"int7"=5349221,"int8"=12345}"#),
            ("hex1 = 0xDEADBEEF\nhex2 = 0xdeadbeef\nhex3 = 0xdead_beef", r#"{"hex1"=3735928559,"hex2"=3735928559,"hex3"=3735928559}"#),
            ("oct1 = 0o01234567\noct2 = 0o755\nbin1 = 0b11010110", r#"{"oct1"=342391,"oct2"=493,"bin1"=214}"#),
            ("max = 9223372036854775807\nmin = -9223372036854775808\nhex = 0x7fffffffffffffff", r#"{"max"=9223372036854775807,"min"=-9223372036854775808,"hex"=9223372036854775807}"#),
            ("zero = +0\nnzero = -0\nhzero = 0x0", r#"{"zero"=0,"nzero"=0,"hzero"=0}"#),
            ("flt1 = +1.0\nflt2 = 3.1415\nflt3 = -0.01", r#"{"flt1"=1.0,"flt2"=3.1415,"flt3"=-0.01}"#),
            ("flt4 = 5e+22\nflt5 = 1e06\nflt6 = -2E-2\nflt7 = 6.626e-34", r#"{"flt4"=5e22,"flt5"=1000000.0,"flt6"=-0.02,"flt7"=6.626e-34}"#),
            ("flt8 = 224_617.445_991_228\nf = 0.0\ng = -0.0\nh = 0e0", r#"{"flt8"=224617.445991228,"f"=0.0,"g"=-0.0,"h"=0.0}"#),
            ("sf1 = inf\nsf2 = +inf\nsf3 = -inf\nsf4 = nan\nsf5 = +nan\nsf6 = -nan", r#"{"sf1"=inf,"sf2"=inf,"sf3"=-inf,"sf4"=NaN,"sf5"=NaN,"sf6"=NaN}"#),
            ("bool1 = true\nbool2 = false", r#"{"bool1"=true,"bool2"=false}"#),
            ("odt1 = 1979-05-27T07:32:00Z\nodt2 = 1979-05-27T00:32:00-07:00\nodt3 = 1979-05-27T00:32:00.999999-07:00",
             r#"{"odt1"=odt(1979-05-27 07:32:00 0 0),"odt2"=odt(1979-05-27 00:32:00 0 -420),"odt3"=odt(1979-05-27 00:32:00 999999000 -420)}"#),
            ("odt4 = 1979-05-27 07:32:00Z\nodt5 = 1979-05-27t07:32:00z\nodt6 = 2000-01-01T00:00:00+05:30",
             r#"{"odt4"=odt(1979-05-27 07:32:00 0 0),"odt5"=odt(1979-05-27 07:32:00 0 0),"odt6"=odt(2000-01-01 00:00:00 0 330)}"#),
            ("ldt1 = 1979-05-27T07:32:00\nldt2 = 1979-05-27T00:32:00.999999",
             r#"{"ldt1"=ldt(1979-05-27 07:32:00 0 0),"ldt2"=ldt(1979-05-27 00:32:00 999999000 0)}"#),
            ("ld1 = 1979-05-27\nlt1 = 07:32:00\nlt2 = 00:32:00.999999",
             r#"{"ld1"=ld(1979-05-27 00:00:00 0 0),"lt1"=lt(0000-00-00 07:32:00 0 0),"lt2"=lt(0000-00-00 00:32:00 999999000 0)}"#),
            ("long = 00:00:00.1234567891234\nleap = 2024-02-29\nsecond = 1990-12-31T23:59:60Z",
             r#"{"long"=lt(0000-00-00 00:00:00 123456789 0),"leap"=ld(2024-02-29 00:00:00 0 0),"second"=odt(1990-12-31 23:59:60 0 0)}"#),
            ("d = 1979-05-27 # a date, then a comment", r#"{"d"=ld(1979-05-27 00:00:00 0 0)}"#),
        ],
        "",
    );
}

#[test]
fn arrays_inline_tables_and_arrays_of_tables_nest() {
    table(
        &[
            ("integers = [ 1, 2, 3 ]\ncolors = [ \"red\", \"yellow\", \"green\" ]\nnested_arrays_of_ints = [ [ 1, 2 ], [3, 4, 5] ]",
             r#"{"integers"=[1,2,3],"colors"=["red","yellow","green"],"nested_arrays_of_ints"=[[1,2],[3,4,5]]}"#),
            ("nested_mixed_array = [ [ 1, 2 ], [\"a\", \"b\", \"c\"] ]\nstring_array = [ \"all\", 'strings', \"\"\"are the same\"\"\", '''type''' ]",
             r#"{"nested_mixed_array"=[[1,2],["a","b","c"]],"string_array"=["all","strings","are the same","type"]}"#),
            ("numbers = [ 0.1, 0.2, 0.5, 1, 2, 5 ]\ncontributors = [\n  \"Foo Bar <foo@example.com>\",\n  { name = \"Baz Qux\", email = \"bazqux@example.com\", url = \"https://example.com/bazqux\" }\n]",
             r#"{"numbers"=[0.1,0.2,0.5,1,2,5],"contributors"=["Foo Bar <foo@example.com>",{"name"="Baz Qux","email"="bazqux@example.com","url"="https://example.com/bazqux"}]}"#),
            ("integers2 = [\n  1, 2, 3\n]\nintegers3 = [\n  1,\n  2, # this is ok\n]\nempty = [ ]\nspaced = [ # c\n\n]",
             r#"{"integers2"=[1,2,3],"integers3"=[1,2],"empty"=[],"spaced"=[]}"#),
            ("name = { first = \"Tom\", last = \"Preston-Werner\" }\npoint = { x = 1, y = 2 }\nanimal = { type.name = \"pug\" }",
             r#"{"name"={"first"="Tom","last"="Preston-Werner"},"point"={"x"=1,"y"=2},"animal"={"type"={"name"="pug"}}}"#),
            ("e = {}\nn = { a = { b = [ { c = 1 } ] } }", r#"{"e"={},"n"={"a"={"b"=[{"c"=1}]}}}"#),
            ("[[products]]\nname = \"Hammer\"\nsku = 738594937\n\n[[products]]  # empty table within the array\n\n[[products]]\nname = \"Nail\"\nsku = 284758393\n\ncolor = \"gray\"",
             r#"{"products"=[{"name"="Hammer","sku"=738594937},{},{"name"="Nail","sku"=284758393,"color"="gray"}]}"#),
            ("[[fruits]]\nname = \"apple\"\n\n[fruits.physical]\ncolor = \"red\"\nshape = \"round\"\n\n[[fruits.varieties]]\nname = \"red delicious\"\n\n[[fruits.varieties]]\nname = \"granny smith\"\n\n\n[[fruits]]\nname = \"banana\"\n\n[[fruits.varieties]]\nname = \"plantain\"",
             r#"{"fruits"=[{"name"="apple","physical"={"color"="red","shape"="round"},"varieties"=[{"name"="red delicious"},{"name"="granny smith"}]},{"name"="banana","varieties"=[{"name"="plantain"}]}]}"#),
            ("points = [ { x = 1, y = 2, z = 3 },\n           { x = 7, y = 8, z = 9 },\n           { x = 2, y = 4, z = 8 } ]",
             r#"{"points"=[{"x"=1,"y"=2,"z"=3},{"x"=7,"y"=8,"z"=9},{"x"=2,"y"=4,"z"=8}]}"#),
            ("mixed = [ 1, { a = 1 } ]\nlast = [ [ { a = 1 } ] ]", r#"{"mixed"=[1,{"a"=1}],"last"=[[{"a"=1}]]}"#),
            ("a.b = 1\n[a.c]\nd = 2", r#"{"a"={"b"=1,"c"={"d"=2}}}"#),
        ],
        "",
    );
}

#[test]
fn malformed_documents_report_the_offending_byte() {
    table(
        &[
            ("key = ", "UnexpectedEnd 6 1 7"),
            ("key = # nothing", "UnexpectedByte 6 1 7"),
            ("= \"no key\"", "UnexpectedByte 0 1 1"),
            ("key \"value\"", "UnexpectedByte 4 1 5"),
            ("first = \"Tom\" last = \"Preston-Werner\"", "UnexpectedByte 14 1 15"),
            ("name = \"Tom\"\nname = \"Pradyun\"", "DuplicateKey 13 2 1"),
            ("spelling = \"favorite\"\n\"spelling\" = \"favourite\"", "DuplicateKey 22 2 1"),
            ("fruit.apple = 1\nfruit.apple.smooth = true", "KeyConflict 16 2 1"),
            ("[fruit]\napple = \"red\"\n\n[fruit]\norange = \"orange\"", "DuplicateTable 23 4 1"),
            ("[fruit]\napple = \"red\"\n\n[fruit.apple]\ntexture = \"smooth\"", "KeyConflict 23 4 1"),
            ("[fruit]\napple.color = \"red\"\napple.taste.sweet = true\n\n[fruit.apple]", "DuplicateTable 54 5 1"),
            ("[fruit]\napple.color = \"red\"\n[fruit.apple.taste]\n[fruit.apple]", "DuplicateTable 48 4 1"),
            ("[product]\ntype = { name = \"Nail\" }\ntype.edible = false", "KeyConflict 35 3 1"),
            ("[product]\ntype.name = \"Nail\"\ntype = { edible = false }", "DuplicateKey 29 3 1"),
            ("a = { b = 1 }\n[a]", "DuplicateTable 14 2 1"),
            ("a = { b = 1 }\n[a.c]", "KeyConflict 14 2 1"),
            ("[x]\n[x.y]\n[x]", "DuplicateTable 10 3 1"),
            ("[a.b]\n[a]\nb.c = 1", "KeyConflict 10 3 1"),
            ("fruits = []\n[[fruits]]", "KeyConflict 12 2 1"),
            ("[[fruits]]\nname = \"apple\"\n[[fruits.varieties]]\nname = \"red delicious\"\n[fruits.varieties]\nname = \"granny smith\"", "KeyConflict 70 5 1"),
            ("[fruit.physical]\ncolor = \"red\"\n[[fruit]]\nname = \"apple\"", "KeyConflict 31 3 1"),
            ("[[a]]\n[a]", "KeyConflict 6 2 1"),
            ("a = [ { b = 1 } ]\n[a.c]", "KeyConflict 18 2 1"),
            ("a = [ { b = 1 } ]\n[[a]]", "KeyConflict 18 2 1"),
            ("point = { x = 1, y = 2, }", "UnexpectedByte 24 1 25"),
            ("point = { x = 1,\n y = 2 }", "UnexpectedByte 16 1 17"),
            ("point = { x = 1 y = 2 }", "UnexpectedByte 16 1 17"),
            ("arr = [ 1, 2", "UnexpectedEnd 12 1 13"),
            ("arr = [ 1 2 ]", "UnexpectedByte 10 1 11"),
            ("arr = [ , ]", "UnexpectedByte 8 1 9"),
            ("arr = [ 1,, ]", "UnexpectedByte 10 1 11"),
            ("str = \"unterminated", "UnexpectedEnd 19 1 20"),
            ("str = \"line\nbreak\"", "ControlCharacter 11 1 12"),
            ("str = 'line\nbreak'", "ControlCharacter 11 1 12"),
            ("str = \"\"\"never closed", "UnexpectedEnd 21 1 22"),
            ("str = \"\"\"seven\"\"\"\"\"\"", "UnexpectedByte 19 1 20"),
            ("str = '''seven''''''", "UnexpectedByte 19 1 20"),
            ("str = \"bad \\q escape\"", "InvalidEscape 11 1 12"),
            ("str = \"bad \\u12 escape\"", "InvalidEscape 11 1 12"),
            ("str = \"surrogate \\uD800\"", "InvalidEscape 17 1 18"),
            ("str = \"too big \\U00110000\"", "InvalidEscape 15 1 16"),
            ("str = \"\"\"trailing \\ x\"\"\"", "InvalidEscape 18 1 19"),
            ("bin = \"\\b\x01\"", "ControlCharacter 9 1 10"),
            ("nul = \"a\x00b\"", "ControlCharacter 8 1 9"),
            ("del = 'a\x7fb'", "ControlCharacter 8 1 9"),
            ("lone = \"a\rb\"", "ControlCharacter 9 1 10"),
            ("bare_cr = 1\r", "UnexpectedByte 11 1 12"),
            ("# comment \x01 control", "ControlCharacter 10 1 11"),
            ("a = 1 # comment \x7f del", "ControlCharacter 16 1 17"),
            ("int = 0_", "InvalidNumber 6 1 7"),
            ("int = _1", "UnexpectedByte 6 1 7"),
            ("int = 1__2", "InvalidNumber 6 1 7"),
            ("int = 1_", "InvalidNumber 6 1 7"),
            ("int = 01", "InvalidNumber 6 1 7"),
            ("int = +0x1", "InvalidNumber 6 1 7"),
            ("int = 0X1", "InvalidNumber 6 1 7"),
            ("int = 0xg", "InvalidNumber 6 1 7"),
            ("int = 0x_1", "InvalidNumber 6 1 7"),
            ("int = 9223372036854775808", "NumberOutOfRange 6 1 7"),
            ("int = -9223372036854775809", "NumberOutOfRange 6 1 7"),
            ("int = 0x8000000000000000", "NumberOutOfRange 6 1 7"),
            ("flt = .7", "UnexpectedByte 6 1 7"),
            ("flt = 7.", "InvalidNumber 6 1 7"),
            ("flt = 3.e+20", "InvalidNumber 6 1 7"),
            ("flt = 1e", "InvalidNumber 6 1 7"),
            ("flt = 1e_1", "InvalidNumber 6 1 7"),
            ("flt = 1.5_", "InvalidNumber 6 1 7"),
            ("flt = 03.14", "InvalidNumber 6 1 7"),
            ("flt = 1e400", "NumberOutOfRange 6 1 7"),
            ("flt = Inf", "UnexpectedByte 6 1 7"),
            ("flt = infinity", "InvalidNumber 6 1 7"),
            ("flt = nan2", "InvalidNumber 6 1 7"),
            ("b = True", "UnexpectedByte 4 1 5"),
            ("b = tru", "UnexpectedByte 4 1 5"),
            ("b = falsey", "UnexpectedByte 9 1 10"),
            ("d = 1979-02-29", "InvalidDateTime 4 1 5"),
            ("d = 1979-13-01", "InvalidDateTime 4 1 5"),
            ("d = 1979-05-32", "InvalidDateTime 4 1 5"),
            ("d = 1979-5-27", "InvalidDateTime 4 1 5"),
            ("d = 1979-05-27T25:00:00", "InvalidDateTime 4 1 5"),
            ("d = 1979-05-27T07:60:00", "InvalidDateTime 4 1 5"),
            ("d = 1979-05-27T07:32:61", "InvalidDateTime 4 1 5"),
            ("d = 1979-05-27T07:32", "InvalidDateTime 4 1 5"),
            ("d = 07:32", "InvalidDateTime 4 1 5"),
            ("d = 07:32:00.", "InvalidDateTime 4 1 5"),
            ("d = 1979-05-27T07:32:00+24:00", "InvalidDateTime 4 1 5"),
            ("d = 1979-05-27T07:32:00+05", "InvalidDateTime 4 1 5"),
            ("d = 1979-05-27T07:32:00Zjunk", "UnexpectedByte 24 1 25"),
            ("d = 1979-05-27 07", "UnexpectedByte 15 1 16"),
            ("[]", "UnexpectedByte 1 1 2"),
            ("[a.]", "UnexpectedByte 3 1 4"),
            ("[.a]", "UnexpectedByte 1 1 2"),
            ("[a]b = 1", "UnexpectedByte 3 1 4"),
            ("[a] [b]", "UnexpectedByte 4 1 5"),
            ("[[a]", "UnexpectedEnd 4 1 5"),
            ("[a]]", "UnexpectedByte 3 1 4"),
            ("a.b. = 1", "UnexpectedByte 5 1 6"),
            ("a. = 1", "UnexpectedByte 3 1 4"),
            ("= 1", "UnexpectedByte 0 1 1"),
            ("a = 1 b", "UnexpectedByte 6 1 7"),
            ("\u{feff}a = 1", "UnexpectedByte 0 1 1"),
            ("k = é", "UnexpectedByte 4 1 5"),
            ("\"bad\nkey\" = 1", "ControlCharacter 4 1 5"),
        ],
        "",
    );
    byte_table(
        &[
            (b"a = \"\xc3\"", "InvalidUtf8 5 1 6"),
            (b"a = \"\xff\"", "InvalidUtf8 5 1 6"),
            (b"# \xe2\x82\nk = 1", "InvalidUtf8 2 1 3"),
            (b"k = '\xed\xa0\x80'", "InvalidUtf8 5 1 6"),
            (b"k = 1\n\xc3\xa9\xc3\xa9 = \xc0\xaf", "InvalidUtf8 13 2 6"),
        ],
        "",
    );
}

#[test]
fn nesting_and_input_size_are_bounded() {
    table(
        &[
            ("a = 1", r#"{"a"=1}"#),
            (
                "a.b = 1\nc = [[1]]\nd = { e = [] }",
                r#"{"a"={"b"=1},"c"=[[1]],"d"={"e"=[]}}"#,
            ),
            ("a.b.c = 1", r#"{"a"={"b"={"c"=1}}}"#),
            ("x = [[[1]]]", "DepthExceeded 6 1 7"),
            ("x = { y = { z = 1 } }", r#"{"x"={"y"={"z"=1}}}"#),
            ("x = { y = { z = {} } }", "DepthExceeded 16 1 17"),
            ("[a.b]\n[a.b.c]", "DepthExceeded 6 2 1"),
            ("[[a.b]]\n[[a.b]]", "DepthExceeded 0 1 1"),
            ("[[a]]\nb = 1", r#"{"a"=[{"b"=1}]}"#),
            ("[[a]]\nb.c = 1", "DepthExceeded 6 2 1"),
        ],
        "bounds.max_depth = 2",
    );
    table(
        &[
            ("a = 1", r#"{"a"=1}"#),
            ("a = []", "DepthExceeded 4 1 5"),
            ("[a]", "DepthExceeded 0 1 1"),
        ],
        "bounds.max_depth = 0",
    );
    table(
        &[
            ("a = 12", r#"{"a"=12}"#),
            ("a = 123", "InputTooLarge 6 0 0"),
            ("a = \"long text\"", "InputTooLarge 6 0 0"),
        ],
        "bounds.max_input = 6",
    );
    let deep = format!("a = {}1{}", "[".repeat(1024), "]".repeat(1024));
    let deeper = format!("a = {}1{}", "[".repeat(1025), "]".repeat(1025));
    let outcome = outcomes(
        &[deep.into_bytes(), deeper.into_bytes()],
        "bounds.max_depth = 1024",
    );
    assert!(outcome[0].starts_with(r#"{"a"=[[[["#), "{}", outcome[0]);
    assert_eq!(outcome[1], "DepthExceeded 1028 1 1029");
}

#[test]
fn invalid_limits_are_rejected_before_any_work() {
    native(
        r#"
value = std.toml.parse_text("a = 1", &std.toml.limits())?
for which in range(4):
    mut bounds = std.toml.limits()
    if which == 0:
        bounds.max_depth = -1
    elif which == 1:
        bounds.max_depth = 1025
    elif which == 2:
        bounds.max_input = -1
    else:
        bounds.max_output = -1
    source = std.bytes.from_text("a = 1")?
    mut reader = Trickle(source.copy()?, 0, 2)
    print(described(std.toml.parse(&source, &bounds))?, described(std.toml.parse_text("a = 1", &bounds))?, described(std.toml.read_value(&mut reader, &bounds))?, encoded(&value, &bounds)?)?
"#,
        r#"
InvalidLimits 0 0 0 InvalidLimits 0 0 0 InvalidLimits 0 0 0 InvalidLimits 0 0 0
InvalidLimits 0 0 0 InvalidLimits 0 0 0 InvalidLimits 0 0 0 InvalidLimits 0 0 0
InvalidLimits 0 0 0 InvalidLimits 0 0 0 InvalidLimits 0 0 0 InvalidLimits 0 0 0
InvalidLimits 0 0 0 InvalidLimits 0 0 0 InvalidLimits 0 0 0 InvalidLimits 0 0 0
"#,
    );
}

#[test]
fn the_writer_puts_plain_entries_before_sections() {
    native(
        r#"
bounds = std.toml.limits()
texts = ["", "b = 1\na = 2", "\"\" = 1\n\"a b\" = 2\n\"é\" = 3\n\"q\\\"\" = 4\nA-z_0 = 5", "s = \"tab\\t nl\\n cr\\r bs\\\\ q\\\" bell\\u0007 del\\u007f é\"", "f = [1.0, -2.5, 1e300, 5e-324, inf, -inf, nan, -0.0, 1e16, 0.1]", "[t]\n[u]\nx = 1\n[[v]]\n[[v]]\ny = 2", "a = []\nb = [[]]\nc = [{}, 1]\nd = [{ x = 1 }]\ne = {}", "[a.b.c]\nd = 1\n[a]\ne = 2", "[[a.b]]\nc = 1\n[[a.b.d]]\n[a.e]", "t = 1979-05-27T07:32:00.120Z\nu = 1979-05-27T07:32:00-00:00\nv = 2000-01-01T00:00:00+05:30\nw = 1979-05-27 07:32:00.000000001\nx = 1979-05-27\ny = 07:32:00", "a.b = 1\na.c = { d = [1, { e = 2 }] }"]?
for text in &texts:
    value = std.toml.parse_text(text, &bounds)?
    print(encoded(&value, &bounds)?)?
"#,
        r#"
""
"b = 1\na = 2\n"
"\"\" = 1\n\"a b\" = 2\n\"é\" = 3\n\"q\\\"\" = 4\nA-z_0 = 5\n"
"s = \"tab\\t nl\\n cr\\r bs\\\\ q\\\" bell\\u0007 del\\u007F é\"\n"
"f = [1.0, -2.5, 1e300, 5e-324, inf, -inf, nan, -0.0, 1e16, 0.1]\n"
"[t]\n\n[u]\nx = 1\n\n[[v]]\n\n[[v]]\ny = 2\n"
"a = []\nb = [[]]\nc = [{}, 1]\n\n[[d]]\nx = 1\n\n[e]\n"
"[a]\ne = 2\n\n[a.b.c]\nd = 1\n"
"[[a.b]]\nc = 1\n\n[[a.b.d]]\n\n[a.e]\n"
"t = 1979-05-27T07:32:00.12Z\nu = 1979-05-27T07:32:00Z\nv = 2000-01-01T00:00:00+05:30\nw = 1979-05-27T07:32:00.000000001\nx = 1979-05-27\ny = 07:32:00\n"
"[a]\nb = 1\n\n[a.c]\nd = [1, { e = 2 }]\n"
"#,
    );
}

#[test]
fn readers_and_writers_report_stream_faults() {
    native(
        r#"
bounds = std.toml.limits()
source = std.bytes.from_text("a = [1, 2]\nb = 'x'")?
for mode in range(3):
    mut reader = Faulty(source.copy()?, 0, 0, 3, mode)
    print(described(std.toml.read_value(&mut reader, &bounds))?)?
value = std.toml.parse(&source, &bounds)?
for room in [100, 7, 0]?:
    mut sink = Sink(std.bytes.empty()?, room)
    match std.toml.write_value(&mut sink, &value, &bounds):
        case Ok(_):
            print("ok", sink.data.len())?
        case Err(error):
            print(placed(&error)?, sink.data.len())?
"#,
        r#"
{"a"=[1,2],"b"="x"}
Stream(UnexpectedEof) 4 0 0
Stream(InvalidProgress) 4 0 0
ok 19
Stream(WriteZero) 7 0 0 7
Stream(WriteZero) 0 0 0 0
"#,
    );
}

#[test]
fn the_writer_rejects_trees_it_cannot_express() {
    native(
        r#"
mut bounds = std.toml.limits()
print(encoded(&Toml.Int(1), &bounds)?)?
mut cases = list[DateTime]()?
cases.append(DateTime(DateTimeKind.LocalDate, 2023, 2, 29, 0, 0, 0, 0, 0))?
cases.append(DateTime(DateTimeKind.LocalDate, 2024, 2, 29, 1, 0, 0, 0, 0))?
cases.append(DateTime(DateTimeKind.LocalTime, 0, 0, 0, 23, 59, 60, 999999999, 0))?
cases.append(DateTime(DateTimeKind.LocalTime, 1, 0, 0, 0, 0, 0, 0, 0))?
cases.append(DateTime(DateTimeKind.LocalDateTime, 2024, 1, 1, 0, 0, 0, 0, 60))?
cases.append(DateTime(DateTimeKind.OffsetDateTime, 2024, 1, 1, 0, 0, 0, 1000000000, 0))?
cases.append(DateTime(DateTimeKind.OffsetDateTime, 2024, 1, 1, 0, 0, 0, 0, -1439))?
cases.append(DateTime(DateTimeKind.OffsetDateTime, 2024, 1, 1, 0, 0, 0, 0, 1440))?
for moment in cases:
    mut entries = list[Entry]()?
    entries.append(Entry("k", Toml.DateTime(moment)))?
    print(encoded(&Toml.Table(entries), &bounds)?)?
mut twice = list[Entry]()?
twice.append(Entry("k", Toml.Int(1)))?
twice.append(Entry("j", Toml.Int(2)))?
twice.append(Entry("k", Toml.Bool(True)))?
mut outer = list[Entry]()?
outer.append(Entry("t", Toml.Table(twice)))?
print(encoded(&Toml.Table(outer), &bounds)?)?
source = std.toml.parse_text("a = 'abcdefgh'\n[b]\nc = [[1]]", &bounds)?
for limit in [0, 5, 29, 30]?:
    bounds.max_output = limit
    print(encoded(&source, &bounds)?)?
for depth in [3, 2, 1, 0]?:
    bounds.max_depth = depth
    print(encoded(&source, &bounds)?)?
"#,
        r#"
NotATable 0 0 0
InvalidDateTime 4 0 0
InvalidDateTime 4 0 0
"k = 23:59:60.999999999\n"
InvalidDateTime 4 0 0
InvalidDateTime 4 0 0
InvalidDateTime 4 0 0
"k = 2024-01-01T00:00:00-23:59\n"
InvalidDateTime 4 0 0
DuplicateKey 4 0 0
OutputTooLarge 0 0 0
OutputTooLarge 5 0 0
OutputTooLarge 29 0 0
"a = \"abcdefgh\"\n\n[b]\nc = [[1]]\n"
"a = \"abcdefgh\"\n\n[b]\nc = [[1]]\n"
DepthExceeded 25 0 0
DepthExceeded 24 0 0
DepthExceeded 15 0 0
"#,
    );
}

/// The manifest shape proposed for project builds in issue #45.
const MANIFEST: &str = r#"manifest-version = 1

[package]
name = "player"
version = "0.1.0"
description = "A small audio player"
authors = ["Example Author"]
license = "MIT"
plenty-version = "0.1.0"

[dependencies.audio]
git = "https://github.com/cjrh/media.git"
subdir = "packages/audio"
tag = "audio-v1.2.0"

[dependencies.utilities]
path = "../utilities"
"#;

#[test]
fn a_project_manifest_reads_and_writes_back_unchanged() {
    let body = r#"
bounds = std.toml.limits()
text = std.encoding.hex_from_text("HEX")?.to_text()?
value = std.toml.parse_text(&text, &bounds)?
print(show(&value)?)?
print(std.toml.encode_text(&value, &bounds)? == text)?
match &value:
    case Toml.Table(root):
        match std.toml.find(root, "dependencies"):
            case Some(index):
                match &root[index].value:
                    case Toml.Table(dependencies):
                        for dependency in dependencies:
                            match &dependency.value:
                                case Toml.Table(fields):
                                    for name in ["git", "path", "tag", "rev"]?:
                                        match std.toml.find(fields, &name):
                                            case Some(at):
                                                print(dependency.key, name, show(&fields[at].value)?)?
                                            case Nothing:
                                                pass
                                case _:
                                    pass
                    case _:
                        pass
            case Nothing:
                pass
    case _:
        pass
"#
    .replace("HEX", &hex(MANIFEST.as_bytes()));
    native(
        &body,
        r#"
{"manifest-version"=1,"package"={"name"="player","version"="0.1.0","description"="A small audio player","authors"=["Example Author"],"license"="MIT","plenty-version"="0.1.0"},"dependencies"={"audio"={"git"="https://github.com/cjrh/media.git","subdir"="packages/audio","tag"="audio-v1.2.0"},"utilities"={"path"="../utilities"}}}
True
audio git "https://github.com/cjrh/media.git"
audio tag "audio-v1.2.0"
utilities path "../utilities"
"#,
    );
}

/// Every proper prefix of a document either parses or fails at a located
/// byte, and short reads agree with the whole buffer.
#[test]
fn every_prefix_parses_or_fails_with_a_position() {
    let document = "# head\nt = 1979-05-27T07:32:00.5-08:00\n[a . \"b\"]\nx = [ 1_0, -0.5e3, 'lit', \"\"\"m\\\nl\"\"\" ]\ny = { z = 0x1f, w = inf }\n[[c]]\nd = '''é'''\n";
    let inputs = (0..=document.len())
        .filter(|end| document.is_char_boundary(*end))
        .map(|end| document.as_bytes()[..end].to_vec())
        .collect::<Vec<_>>();
    let results = outcomes(&inputs, "");
    for (input, result) in inputs.iter().zip(&results) {
        let ok = result.starts_with('{');
        let located = result
            .split(' ')
            .nth(2)
            .is_some_and(|line| line.parse::<u32>().is_ok_and(|line| line >= 1));
        assert!(
            ok || located,
            "{:?} -> {result}",
            String::from_utf8_lossy(input)
        );
        assert!(
            !result.starts_with("STREAM")
                && !result.starts_with("REPARSE")
                && !result.starts_with("UNSTABLE"),
            "{result}"
        );
    }
    assert!(results
        .last()
        .unwrap()
        .starts_with(r#"{"t"=odt(1979-05-27 07:32:00 500000000 -480)"#));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_at_every_step_and_leaks_nothing() {
    native(
        r#"
bounds = std.toml.limits()
text = "title = \"a value long enough to need heap storage\"\n[owner.\"key text long enough for the heap\"]\nlist = [1, 2.5, [true], { k = 1979-05-27 }]\n[[items]]\nname = \"\"\"multi\nline\"\"\"\n"
source = std.bytes.from_text(text)?
value = std.toml.parse(&source, &bounds)?
mut markers = list[str]()?
for count in range(600):
    markers.append("".join(&["__test_fail_allocations_after_", str.from(count)?, "__"]?)?)?
for which in range(3):
    mut refused = 0
    mut other = 0
    mut first = -1
    for count in range(600):
        mut fault = 0
        if which == 0:
            print(markers[count])?
            result = std.toml.parse(&source, &bounds)
            print("__test_restore_allocations__")?
            match result:
                case Ok(_):
                    pass
                case Err(error):
                    match &error.fault:
                        case TomlFault.Allocation(_):
                            fault = 1
                        case _:
                            fault = 2
        elif which == 1:
            mut reader = Trickle(source.copy()?, 0, 5)
            print(markers[count])?
            result = std.toml.read_value(&mut reader, &bounds)
            print("__test_restore_allocations__")?
            match result:
                case Ok(_):
                    pass
                case Err(error):
                    match &error.fault:
                        case TomlFault.Allocation(_):
                            fault = 1
                        case _:
                            fault = 2
        else:
            print(markers[count])?
            result = std.toml.encode(&value, &bounds)
            print("__test_restore_allocations__")?
            match result:
                case Ok(_):
                    pass
                case Err(error):
                    match &error.fault:
                        case TomlFault.Allocation(_):
                            fault = 1
                        case _:
                            fault = 2
        if fault == 1:
            refused += 1
        elif fault == 2:
            other += 1
        elif first < 0:
            first = count
    # Every count below the first success failed, and none above it did.
    print(which, refused > 5, refused == first, other)?
"#,
        r#"
0 True True 0
1 True True 0
2 True True 0
"#,
    );
}

/// xorshift64: the fuzz inputs are the same on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// Bytes with meaning somewhere in TOML.
const INTERESTING: &[u8] = b"[]{}=,.\"'\\#\n\r\t _-+:TZez0179abfinx\x00\x7f\xc3\xa9";

/// Mutated documents never crash the parser: each either parses, with a
/// stable written form, or fails at a located byte.
#[test]
fn mutated_documents_parse_or_fail_cleanly() {
    let seeds = [
        MANIFEST,
        "a = [1, 2.0, \"x\", { b = 'c' }]\n[t.u]\nv = 1979-05-27T07:32:00Z\n[[w]]\nx = \"\"\"y\"\"\"\n",
        "k.l.m = 0x10\n\"q\" = -inf\nd = 07:32:00.25\ns = '''\nraw\n'''\n",
    ];
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let mut inputs = Vec::new();
    for _ in 0..240 {
        let mut input = seeds[random.below(seeds.len())].as_bytes().to_vec();
        for _ in 0..1 + random.below(4) {
            let at = random.below(input.len() + 1);
            match random.below(3) {
                0 if at < input.len() => {
                    input.remove(at);
                }
                1 if at < input.len() => {
                    input[at] = INTERESTING[random.below(INTERESTING.len())];
                }
                _ => input.insert(at, INTERESTING[random.below(INTERESTING.len())]),
            }
        }
        inputs.push(input);
    }
    let results = outcomes(&inputs, "");
    let mut accepted = 0;
    for (input, result) in inputs.iter().zip(&results) {
        assert!(
            !result.starts_with("STREAM")
                && !result.starts_with("REPARSE")
                && !result.starts_with("UNSTABLE"),
            "{:?} -> {result}",
            String::from_utf8_lossy(input)
        );
        if result.starts_with('{') {
            accepted += 1;
        } else {
            let fields = result.split(' ').collect::<Vec<_>>();
            let offset = fields[1].parse::<usize>().unwrap();
            assert!(offset <= input.len(), "{result}");
            assert!(fields[2].parse::<u32>().unwrap() >= 1, "{result}");
        }
    }
    assert!(accepted > 20 && accepted < 220, "{accepted} accepted");
}
