# Creating a file without overwriting

Use mode `"x"` when an existing path should be an error. This check happens
atomically during creation, so there is no separate existence-check race.

```plenty
def demo() -> Result[(), IoError]:
    with open("new.txt", "x")? as file:
        file.write("first version")?
    print(read_text("new.txt")?)?
    Ok(())
def main() -> Result[(), IoError]:
    demo()?
    Ok(())
```
```output
first version
```

Add `+` to an open mode to enable both reading and writing. `r+` preserves an
existing file, `w+` truncates or creates, `x+` creates exclusively, and `a+`
starts at EOF and always appends writes. Writes overwrite bytes at the current
position; they do not insert characters. Use care with multibyte text.

```plenty
def demo() -> Result[(), IoError]:
    write_text("update.txt", "hello")?
    with open("update.txt", "r+")? as file:
        file.read(1)?
        file.write("a")?
    print(read_text("update.txt")?)?
    Ok(())
def main() -> Result[(), IoError]:
    demo()?
    Ok(())
```
```output
hallo
```
