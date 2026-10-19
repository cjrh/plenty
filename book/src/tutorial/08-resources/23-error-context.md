# Say which operation failed

`IoError.System(2)` says a file was missing, but not which file or which step
wanted it. `std.error` wraps an error in a `Context` holding an operation label
and, optionally, a path. The original error stays inside as `cause`, with its
type.

```plenty
from std.error import Context, Subject, context, path_context, native_code
from std.path import Path, from_text

def read_limit(path: &Path) -> Result[list[u8], Context[IoError]]:
    path_context(read_bytes(path.as_bytes()), "read limit", path)

def parse_limit(text: &str) -> Result[i64, Context[ParseError]]:
    context(i64.parse(text), "parse limit")

def flush_output() -> Result[(), Context[IoError]]:
    context(flush_stdout(), "flush output")

def limit(path: &Path) -> Result[i64, Failure]:
    match read_limit(path):
        case Ok(bytes):
            Ok(parse_limit(&decode_utf8(bytes)?)?)
        case Err(error):
            print(error.describe()?)?
            print(native_code(error.cause))?
            Ok(10)

def run() -> Result[(), Failure]:
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
read limit "limit.txt": IoError.System(2)
Option[i32].Some(2)
10
parse limit: ParseError.Invalid
True
```

`context(result, label)` and `path_context(result, label, &path)` pass an `Ok`
through and wrap an `Err`, including operations whose success is `()`, as
`flush_output` shows. `main` uses the direct form,
`Context(label, Subject.Absent, cause)`. Nothing is wrapped implicitly: `?`
still needs the function's exact error type, so `read_limit` returns
`Context[IoError]` and says so.

`describe()` builds one line: the label, the quoted path if there is one, and
the cause. It allocates, so it returns a Result. `error.cause` is the untouched
typed error; match it, compare it, or ask `native_code` for the OS number.

`report()` is for the edge of the program. It flushes standard output and
writes `error: ` plus the description to standard error. It returns nothing and
does not choose an exit status, so `main` returns `i32` and decides. If memory
runs out while describing, `report` still writes the label with a fixed
message, and a failed path copy in `path_context` keeps the label and cause.

Here `run` returns `Failure`, which keeps no details, so a failing `run` would
report only `error: run: Failure.Unspecified`. Return a `Context` or your own
enum from the functions whose failures you want explained. See
[the module reference](../../design/stdlib/error.md) for nesting contexts and
the rules standard modules follow.

For a human-readable cause, give its type the public `std.report` formatting
method. Contexts then format their operation labels and delegate to that cause:

```plenty
from std.error import Context, Subject
from std.report import Formatter, FormatError, to_string

pub enum SettingsError:
    Invalid

    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
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
