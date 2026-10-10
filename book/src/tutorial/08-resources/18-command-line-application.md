# Build a small command-line application

Put one integer on each line of an input file. This program ignores blank lines,
adds the integers, and writes a short report to an output file. Its two arguments
are the input and output paths. Running it without arguments prints usage.

Test for EOF before stripping whitespace: a blank line should be ignored, not
mistaken for the end of the file.

Save this module as `report.plenty`:

```plenty-file report.plenty
pub def summarize(path: &str) -> Result[str, Failure]:
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
    heading = "count=".concat(str.from(count)?)?
    result = heading.concat(" total=")?.concat(str.from(total)?)?.concat("\n")?
    Ok(result)
```

Save this application beside it as `main.plenty`:

```plenty
from report import summarize

def run() -> Result[(), Failure]:
    arguments = args()?
    if len(arguments) == 1:
        print("usage: number-report INPUT OUTPUT")?
        return Ok(())
    if len(arguments) != 3:
        write_stderr("usage: number-report INPUT OUTPUT\n")?
        return Err(Failure.Unspecified)
    report = summarize(arguments[1])?
    write_text(arguments[2], report)?
    print("saved report")?
    Ok(())

def main() -> Result[(), Failure]:
    match run():
        case Ok(_):
            Ok(())
        case Err(_):
            write_stderr("report failed; check paths and integer input\n")?
            Err(Failure.Unspecified)
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

`Failure` discards error details, so this diagnostic cannot distinguish a bad
integer from an I/O or allocation error. Keep concrete error types when callers
need that distinction. Values and their total must fit `i64`; arithmetic overflow
is a runtime trap.

See the
[text I/O reference](../../design/05-current-language-contract/01-practical-text-i-o.md)
for file contracts and limits.
