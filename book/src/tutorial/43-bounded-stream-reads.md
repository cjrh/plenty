# Bounded stream reads

Read a bounded number of characters with `read(count)`. The count is Unicode
characters, not UTF-8 bytes. Zero consumes nothing; a negative count reads to EOF.

```plenty
def demo() -> Result[(), IoError]:
    write_text("bounded.txt", "é🦀hello")?
    with open("bounded.txt")? as file:
        print(file.read(2)?)?
        print(file.read()?)?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
é🦀
hello
Result[(), IoError].Ok(())
```


`readline(count)` limits a line read in the same way. A long line can arrive in
several pieces; a negative count reads the rest of the line.

```plenty
def demo() -> Result[(), IoError]:
    write_text("lines.txt", "abcd\nnext")?
    with open("lines.txt")? as file:
        print(file.readline(2)?)?
        print(file.readline(2)?)?
        print(file.readline(2))?
    Ok(())
def main() -> Result[(), IoError]:
    print(demo())?
    Ok(())
```
```output
ab
cd
Result[str, IoError].Ok("\n")
Result[(), IoError].Ok(())
```
