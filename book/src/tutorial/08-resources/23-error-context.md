# Say which operation failed

`IoError.System(2)` says a file was missing, but not which file or which step
wanted it. `std.error` wraps an error in a `Context` holding an operation label
and, optionally, a path. The original error stays inside as `cause`, with its
type.

```plenty
from std.error import Context, Subject, context, path_context, native_code, native_message
from std.path import Path, from_text

def read_limit(path: &Path) -> Result[list[u8], Context[IoError]]:
    path_context(read_bytes(path.as_bytes()), "read limit", path)

def parse_limit(text: &str) -> Result[i64, Context[ParseError]]:
    context(i64.parse(text), "parse limit")

def flush_output() -> Result[(), Context[IoError]]:
    context(flush_stdout(), "flush output")

def limit(path: &Path) -> Result[i64, AllocError | IoError | Context[ParseError]]:
    match read_limit(path):
        case Ok(bytes):
            Ok(parse_limit(&decode_utf8(bytes)?)?)
        case Err(error):
            match native_code(error.cause):
                case Some(code):
                    message = native_message(code).unwrap_or("unknown error")
                    cause = f"{message} (os error {code})"?
                    print(error.describe_with(&cause)?)?
                case Nothing:
                    print(error.describe()?)?
            Ok(10)

def run() -> Result[(), AllocError | IoError | Context[IoError] | Context[ParseError]]:
    path = from_text("limit.txt")?
    print(limit(&path)?)?
    match parse_limit("ten"):
        case Ok(value):
            print(value)?
        case Err(error):
            print(error.describe()?)?
            print(error.cause == ParseError.Invalid)?
    flush_output()?
    Ok(())

def main() -> i32:
    match run():
        case Ok(_):
            0i32
        case Err(cause):
            Context("run", Subject.Absent, cause).report()
            1i32
```
```output
read limit "limit.txt": No such file or directory (os error 2)
10
parse limit: ParseError.Invalid
True
```

`context(result, label)` and `path_context(result, label, &path)` pass an `Ok`
through and wrap an `Err`, including operations whose success is `()`, as
`flush_output` shows. `main` uses the direct form,
`Context(label, Subject.Absent, cause)`. Nothing is wrapped implicitly: `?`
needs a compatible declared error, so `read_limit` returns `Context[IoError]`
and callers can include that context in an error sum.

`describe()` builds one line: the label, the quoted path if there is one, and
the cause. It allocates, so it returns a Result. `error.cause` is the untouched
typed error; match it, compare it, or ask `native_code` for the OS number.
`native_message` turns that number into fixed English text without
allocating; it returns `Nothing` for a number it does not know, so the
program picks the fallback. `describe_with(&text)` builds the same line with
that text in place of the cause, which is how `limit` reports a missing file.
See [describing a native code](../../design/stdlib/error.md#describing-a-native-code).

`report()` is for the edge of the program. It flushes standard output and
writes `error: ` plus the description to standard error. It returns nothing and
does not choose an exit status, so `main` returns `i32` and decides. If memory
runs out while describing, `report` still writes the label with a fixed
message, and a failed path copy in `path_context` keeps the label and cause.

Here `run` preserves its allocation, I/O, and contextual errors in a sum.
The outer context adds the operation label without discarding the cause.
Return a `Context` or your own enum when a failure needs more explanation. See
[the module reference](../../design/stdlib/error.md) for nesting contexts and
the rules standard modules follow.

For a human-readable cause, give its type the public `std.report` formatting
method. Contexts then format their operation labels and delegate to that cause:

```plenty
from std.error import Context, Subject
from std.fmt import Formatter, FormatError
from std.report import to_string

pub enum SettingsError:
    Invalid

    pub def format[W](self, output: &mut Formatter[W]) -> Result[(), FormatError]:
        output.text("invalid setting")

def main() -> Result[(), FormatError | IoError]:
    inner = Context("parse", Subject.Absent, SettingsError.Invalid)
    outer = Context("load settings", Subject.Absent, inner)
    print(to_string(&outer, 120, 8)?)?
    print(to_string(&outer, 120, 8)?)?
    Ok(())
```

```output
load settings: parse: invalid setting
load settings: parse: invalid setting
```

Formatting borrows both contexts and includes the cause once. Returning an
`Err` with this context type from `main` selects the same wording automatically,
with the entrypoint's prefix and newline. A context whose cause has no formatting
method still uses structural entrypoint reporting. The existing `describe`,
`describe_with`, `report` and `report_with` methods keep their behavior.

Without a path, context formatting adds no allocation. A present path still
uses fallible `Path.display`; failure leaves the typed cause intact and may
leave part of the report already written. See
[bounded reports](report-formatting.md) for limits and failure handling.

## One error type with labels at every layer

`Context[E]` changes the error type each time it wraps: wrapping a
`Context[ParseError]` again gives `Context[Context[ParseError]]`, so `?` cannot
forward it in a function that returns `Context[ParseError]`. When an
application wants a single error type for all its functions, add one class for
"a cause with labels" to its error sum and a small `context` helper that either
wraps a plain cause or adds a label to an existing one:

```plenty
type Base = AllocError | IoError | ParseError

# Caps how many labels one error keeps, so a retry loop cannot grow it.
# Labels are normally literals; a formatted label adds its own length.
const FRAMES: i64 = 8

# A typed cause with the innermost operation and later labels, innermost
# first. Wrapping allocates nothing; the label list is made on first use. A
# label past FRAMES, or one without memory to store it, is only counted.
class Trace:
    pub cause: Base
    pub operation: str
    pub outer: Option[list[str]]
    pub omitted: i64

    def add(self: &mut Trace, operation: str) -> ():
        if self.kept() < FRAMES:
            match &mut self.outer:
                case Some(labels):
                    if labels.append(operation).is_ok():
                        return
                case Nothing:
                    match [operation]:
                        case Ok(labels):
                            self.outer = Some(labels)
                            return
                        case Err(_):
                            pass
        if self.omitted < i64.MAX:
            unchecked:
                self.omitted += 1

    def kept(self) -> i64:
        match &self.outer:
            case Some(labels):
                len(labels)
            case Nothing:
                0

    # Outermost first, as nested `Context` descriptions read.
    def describe(self) -> Result[str, AllocError]:
        mut text = self.operation.concat(": ")?.concat(&str.repr(self.cause)?)?
        match &self.outer:
            case Some(labels):
                for label in labels:
                    text = label.concat(": ")?.concat(&text)?
            case Nothing:
                pass
        if self.omitted > 0:
            text = f"[{self.omitted} omitted] {text}"?
        Ok(text)

type Error = Base | Trace

const DEFAULT_LIMIT: i64 = 10

def context[T](result: Result[T, Error], operation: str) -> Result[T, Error]:
    match result:
        case Ok(value):
            Ok(value)
        case Err(error):
            match error:
                case trace: Trace:
                    mut traced = trace
                    traced.add(operation)
                    Err(traced)
                case rest: Base:
                    Err(Trace(rest, operation, Nothing, 0))

def read_limit(text: &str) -> Result[i64, Error]:
    context(i64.parse(text), "parse limit")

def load(text: &str) -> Result[i64, Error]:
    context(read_limit(text), "load settings")

def main() -> Result[(), Error]:
    for text in ["12", "ten"]?:
        match load(text):
            case Ok(limit):
                print(limit)?
            case Err(error):
                match error:
                    case trace: Trace:
                        print(trace.describe()?)?
                        match &trace.cause:
                            case ParseError.Invalid:
                                print("using the default limit", DEFAULT_LIMIT)?
                            case _:
                                return Err(trace)
                    case rest: Base:
                        return Err(rest)
    Ok(())
```

```output
12
load settings: parse limit: ParseError.Invalid
using the default limit 10
```

`Error` is `Base | Trace`, so plain `?` still forwards a `ParseError` or
`IoError`, and `context` keeps the type at every layer. `context[T]` takes a
type parameter so one helper serves every success type, `i64` here and `()`
for a write; [generic functions](../09-generics/generic-functions.md) explains
the syntax. In the helper,
`case rest: Base:` binds whichever plain member remains, with the smaller type
`Base`: see [recovering from one error](../03-absence/anonymous-sums.md#recover-from-one-error-and-forward-the-rest).

The cause keeps its type: `main` matches `ParseError.Invalid` to recover and
forwards anything else unchanged. Do not turn the cause into a message string
to fit one error type; that loses what the caller could match, as `Failure`
does.

Wrapping a cause allocates nothing. Storing a later label may allocate; if it
cannot, or `FRAMES` labels are already kept, the label is counted in `omitted`
and the cause stays. `FRAMES` caps the number of labels, so a retry loop cannot
grow the error; it does not cap a label's length, which is why labels are
usually string literals. The
[module reference](../../design/stdlib/error.md#one-application-error-with-context)
shows the same pattern keeping the progress of a partial write.
