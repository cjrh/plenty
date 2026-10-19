# Parse typed arguments

Use `std.cli` when a tool needs options, positionals, generated help, and useful
usage errors. The schema explicitly chooses each argument's type and whether it
is required or repeatable. Path values retain native bytes.

This small input-selection tool accepts a row limit and any number of paths.
The fixture models `finder --limit -2 first.csv -- --last.csv`. Real programs
use `raw = args_bytes()?` and `parse(&schema, &raw, 1)` to skip the executable name.

```plenty
from std.cli import Spec, Kind, Value, parse

def main() -> Result[(), Failure]:
    schema = [
        Spec("--limit", Kind.Integer, False, False, False, "maximum rows"),
        Spec("FILE", Kind.Path, True, True, True, "input files")
    ]?
    raw = [encode_utf8("--limit")?, encode_utf8("-2")?,
           encode_utf8("first.csv")?, encode_utf8("--")?,
           encode_utf8("--last.csv")?]?
    parsed = parse(&schema, &raw, 0)?
    for entry in &parsed.entries:
        match &entry.value:
            case Value.Integer(limit):
                print("limit:", limit)?
            case Value.Path(path):
                print("file:", path.display()?)?
            case _:
                pass
    Ok(())
```
```output
limit: -2
file: first.csv
file: --last.csv
```

Long options use separate values: `--limit 12`. A value following an option is
always consumed, even if it starts with `-`. The standalone `--` ends option
recognition; subsequent tokens are positional. Missing required arguments,
unknown options, duplicate nonrepeatable options, and invalid integers are typed
errors. Optional specs produce no entry when absent, leaving defaults to you.

Handle help before running the tool. It is a successful result, not an error.
The following complete program uses a synthetic request so the lesson can run
without shell arguments. Replace `raw` and the start index as above in a binary.

```plenty
from std.cli import Spec, Kind, parse, help, describe_for, exit_status
from std.error import Context, Subject

def run() -> Result[i32, AllocError | IoError]:
    schema = [Spec("FILE", Kind.Path, True, True, False, "input file")]?
    raw = [encode_utf8("--help")?]?
    status = match parse(&schema, &raw, 0):
        case Ok(parsed):
            if parsed.help_requested:
                match help(&schema, "finder"):
                    case Ok(text):
                        write_stdout(text)?
                    case Err(error):
                        Context("help", Subject.Absent, error).report()
                        return Ok(1i32)
            0i32
        case Err(error):
            status = exit_status(&error)
            context = Context("arguments", Subject.Absent, error)
            context.report_with(describe_for(&context.cause, &schema))
            status
    Ok(status)

def main() -> i32:
    match run():
        case Ok(status):
            status
        case Err(error):
            Context("argument setup", Subject.Absent, error).report()
            1i32
```
```output
Usage: finder [--help]
  FILE <path> (required)  input file
  --help  Show this help
```

`--help` bypasses missing-required checks, but an earlier bad argument still
fails. After `--`, or as the next token for a text/path option, `--help` is data.
The parser validates the schema before processing either form.

The `run` helper propagates setup and output failures. `main` reports
those failures and returns a nonzero exit status, just as it does for argument
and help errors.
[`std.cli`](../../design/stdlib/cli.md) lists all errors, costs, and grammar limits.
