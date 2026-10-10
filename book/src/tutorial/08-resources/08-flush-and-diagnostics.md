# Flushing output and reporting diagnostics

Use `write_stderr(text)` for diagnostics. It returns the same character-count
result as `write_stdout`. `flush_stdout()` and `flush_stderr()` return
`Result[(), IoError]`. Flush when the caller needs to observe buffered-write
errors or when displaying a prompt; a flush does not promise disk durability.

```plenty
def report() -> Result[(), IoError]:
    write_stdout("ready")?
    flush_stdout()?
    Ok(())

def main() -> Result[(), IoError]:
    report()?
    print("")?
    Ok(())
```
```output
ready
```
