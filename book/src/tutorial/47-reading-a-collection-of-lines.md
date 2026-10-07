# Reading a collection of lines

`readlines()` collects the remaining lines into an independent `list[str]`,
keeping their translated newlines. At EOF it returns an empty list. Failures
release the partial list, but may have consumed input. For large or untrusted
files, bounded `readline(count)` lets you control memory use instead.

```plenty
def demo() -> Result[list[str], IoError]:
    write_text("collection.txt", "first\r\n\nlast")?
    with open("collection.txt")? as file:
        return file.readlines()
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
Result[list[str], IoError].Ok(["first\n", "\n", "last"])
```

`writelines(lines)` writes a `list[str]` without consuming it. It inserts no
newlines: include them in the strings when wanted. The operation returns
`Result[(), IoError]`; an error may leave a partially written file.

```plenty
def demo() -> Result[(), Failure]:
    lines = ["one\n", "two\n"]?
    with open("written.txt", "w")? as file:
        file.writelines(lines)?
    print(len(lines))?
    with open("written.txt")? as file:
        print(file.readlines()?)?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
2
["one\n", "two\n"]
Result[(), Failure].Ok(())
```
