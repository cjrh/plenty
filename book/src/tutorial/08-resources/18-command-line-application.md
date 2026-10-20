# Build a small command-line application

Put one integer on each line of an input file. This program ignores blank lines,
adds the integers, and writes a short report to an output file. Its two arguments
are the input and output paths. Running it without arguments prints usage.

Test for EOF before stripping whitespace: a blank line should be ignored, not
mistaken for the end of the file.

Save this module as `report.plenty`:

```plenty-file report.plenty
from std.fmt import FormatError

pub def summarize(path: &str) -> Result[str, AllocError | IoError | ParseError | FormatError]:
    mut count = 0
    mut total = 0
    with open(path)? as file:
        while True:
            line = file.readline()?
            if line == "":
                break
            cleaned = line.strip()?
            if cleaned == "":
                continue
            total = total + i64.parse(cleaned)?
            count = count + 1
    Ok(f"count={count} total={total}\n"?)
```

Save this application beside it as `main.plenty`:

```plenty
from report import summarize
from std.fmt import FormatError

enum UsageError:
    Arguments

type AppError = AllocError | IoError | ParseError | FormatError | UsageError

def run() -> Result[(), AppError]:
    arguments = args()?
    if len(arguments) == 1:
        print("usage: number-report INPUT OUTPUT")?
        return Ok(())
    if len(arguments) != 3:
        write_stderr("usage: number-report INPUT OUTPUT\n")?
        return Err(UsageError.Arguments)
    report = summarize(arguments[1])?
    write_text(arguments[2], report)?
    print("saved report")?
    Ok(())

def main() -> Result[(), AppError]:
    run()
```
```output
usage: number-report INPUT OUTPUT
```

Compile and pass arguments to the binary:

```sh
plenty --compile main.plenty -o number-report
./number-report numbers.txt report.txt
```

For an input containing `10`, a blank line, and `20`, the output file contains
`count=2 total=30` followed by a newline. `write_text` replaces the output file;
choose a path whose contents may be replaced.

`AppError` preserves usage, parsing, formatting, I/O, and allocation errors. Returning the
error from `main` reports its cause on standard error and exits unsuccessfully.
`run` prints `"saved report"` only after writing succeeds. It propagates failures
to `main` so a failed operation cannot accidentally become a successful exit.
Values and their total must fit `i64`; arithmetic overflow is a runtime trap.

Checking the argument count directly keeps this two-path interface simple.
For named options and typed argument values, use the
[typed command-line parser](typed-arguments.md). The explicit `readline` loop
is necessary because files do not yet support direct `for line in file`
iteration.

See the
[text I/O reference](../../design/facilities/02-io/index.md)
for file contracts and limits.
