# Enumerate and classify paths

Use `std.path` to represent a native path and `std.fs` to enumerate it lazily.
Each generator item is a Result because advancement can fail after opening:

```plenty
import std.fs as fs
import std.path as path

def main() -> Result[(), Failure]:
    write_text("sample.txt", "abc")?
    root = path.from_text(".")?
    for result in fs.entries(fs.read_dir(&root)?):
        entry = result?
        name = entry.file_name()?
        if name.to_text()? == "sample.txt":
            print(entry.file_type()? == fs.FileType.File)?
            print(entry.metadata()?.size())?
    Ok(())
```

```output
True
3
```

`entries` consumes the directory owner. Exhaustion closes it; an early break,
return or propagated error drops it. Keep a named mutable ReadDir when you want
manual `next()` calls or explicit `close()`. Entries own their data and remain
valid after the directory closes. Enumeration includes dot-prefixed names and
has unspecified order.

`entry.file_type()` and `entry.metadata()` inspect a symlink itself.
`fs.metadata(&path)` follows its target; `fs.symlink_metadata(&path)` does not.
A dangling link can therefore be inspected without requiring its target.
Types distinguish regular files, directories, links and other native objects.

Path text conversion is checked. For arbitrary Unix filenames, use
`name.as_bytes()` and [NUL-framed transport](20-binary-paths.md), avoiding text
decoding. These filesystem APIs currently require 64-bit Linux and statx for
metadata. See the [directory reference](../../design/stdlib/fs.md).
