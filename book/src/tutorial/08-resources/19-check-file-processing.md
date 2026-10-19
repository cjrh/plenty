# Check the application processing module

Test the processing module without command-line arguments. Save its companion
as `report_demo.plenty`:

```plenty-file report_demo.plenty
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

Run this application in a scratch directory; it replaces `numbers.txt` and
`report.txt`:

```plenty
from report_demo import summarize
from std.fmt import FormatError

def main() -> Result[(), AllocError | IoError | ParseError | FormatError]:
    write_text("numbers.txt", "10\r\n\r\n20\r\n")?
    report = summarize("numbers.txt")?
    write_text("report.txt", report)?
    write_stdout(read_text("report.txt")?)?
    Ok(())
```
```output
count=2 total=30
```
