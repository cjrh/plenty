# Read and write TOML

`std.toml` parses TOML 1.0 documents, such as configuration files and project
manifests, into a value tree, and writes a tree back as TOML text. Every call
takes explicit limits.

## Parse a configuration file

```plenty
import std.toml
from std.toml import Entry, Toml

def show(entries: &list[Entry], indent: &str) -> Result[(), Failure]:
    for entry in entries:
        name = (indent + entry.key)?
        match &entry.value:
            case Toml.Table(inner):
                print((name + ":")?)?
                show(inner, &(indent + "  ")?)?
            case Toml.Text(text):
                print(name, "is the text", text)?
            case Toml.Int(number):
                print(name, "is the integer", *number)?
            case Toml.Array(items):
                print(name, "has", len(items), "items")?
            case Toml.DateTime(moment):
                print(name, "is a date in", moment.year)?
            case _:
                print(name, "is something else")?
    Ok(())

def main() -> Result[(), Failure]:
    bounds = std.toml.limits()
    text = "# Service settings\ntitle = \"cache\"\n\n[server]\nhost = \"0.0.0.0\"\nport = 8080\n\n[server.tls]\nenabled = true\n\n[[backend]]\nurl = \"http://a.internal\"\n\n[[backend]]\nurl = \"http://b.internal\"\n\n[release]\ndate = 2026-10-01\n"
    value = std.toml.parse_text(text, &bounds)?
    match &value:
        case Toml.Table(root):
            show(root, "")?
            print(std.toml.find(root, "server"), std.toml.find(root, "client"))?
        case _:
            print("a document is always a table")?
    Ok(())
```
```output
title is the text cache
server:
  host is the text 0.0.0.0
  port is the integer 8080
  tls:
    enabled is something else
backend has 2 items
release:
  date is a date in 2026
Option[i64].Some(1) Option[i64].Nothing
```

A document is always a `Toml.Table`: a `list[Entry]` in source order, where
each entry has a `key` and a `value`. `[server]` and `[server.tls]` become
nested tables, and each `[[backend]]` adds one table to the array `backend`.
`find` returns the index of the first entry with a key.

`Toml` has one variant per TOML type: `Bool`, `Int`, `Float`, `Text`,
`DateTime`, `Array`, and `Table`. Integers are `i64`. A `DateTime` holds the
date and time fields, its offset from UTC in minutes, and a `kind` that says
which parts the text gave: an offset date-time, a local date-time, a local
date, or a local time.

## Errors say where

A `TomlError` carries a `fault`, the byte `offset`, and a `line` and `column`
that both start at 1.

```plenty
import std.toml
from std.toml import Limits

def attempt(text: &str, bounds: &Limits) -> Result[(), Failure]:
    match std.toml.parse_text(text, bounds):
        case Ok(_):
            print("ok")?
        case Err(error):
            print(error.fault, "at line", error.line, "column", error.column)?
    Ok(())

def main() -> Result[(), Failure]:
    bounds = std.toml.limits()
    attempt("name = \"Tom\"\nname = \"Pradyun\"", &bounds)?
    attempt("[fruit]\na = 1\n[fruit]\nb = 2", &bounds)?
    attempt("point = { x = 1 }\npoint.y = 2", &bounds)?
    attempt("port = 08080", &bounds)?
    attempt("day = 2026-02-29", &bounds)?
    attempt("key = \"unterminated", &bounds)?
    Ok(())
```
```output
std.toml.TomlFault.DuplicateKey at line 2 column 1
std.toml.TomlFault.DuplicateTable at line 3 column 1
std.toml.TomlFault.KeyConflict at line 2 column 1
std.toml.TomlFault.InvalidNumber at line 1 column 8
std.toml.TomlFault.InvalidDateTime at line 1 column 7
std.toml.TomlFault.UnexpectedEnd at line 1 column 20
```

TOML forbids defining a key or table twice, and an inline table such as
`{ x = 1 }` is complete as written, so `point.y` cannot add to it. Integers may
not have leading zeros, and dates must exist. An error about a key or a table
points at the start of the line that repeats it.

## Write a tree as TOML

```plenty
import std.toml
from std.toml import Entry, Toml

def main() -> Result[(), Failure]:
    bounds = std.toml.limits()
    mut package = list[Entry]()?
    package.append(Entry("name", Toml.Text("player")))?
    package.append(Entry("version", Toml.Text("0.1.0")))?
    mut authors = list[Toml]()?
    authors.append(Toml.Text("Ada"))?
    package.append(Entry("authors", Toml.Array(authors)))?
    mut root = list[Entry]()?
    root.append(Entry("package", Toml.Table(package)))?
    root.append(Entry("manifest-version", Toml.Int(1)))?
    text = std.toml.encode_text(&Toml.Table(root), &bounds)?
    print(text.rstrip()?)?
    Ok(())
```
```output
manifest-version = 1

[package]
name = "player"
version = "0.1.0"
authors = ["Ada"]
```

The writer puts each table's plain keys first and its tables after them as
`[section]` headers, because a key written after a header would belong to that
header's table. Parsing the output gives back the same values. The writer does
not keep comments or the original layout, so it suits generated files rather
than editing a file a person maintains.

The [`std.toml` reference](../../design/stdlib/toml.md) lists every fault,
limit, and cost.
