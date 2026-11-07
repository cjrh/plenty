# Warnings

A warning reports code the checker accepts but that has a simpler correct
form. Warnings never reject a program and never change an exit status.

Checking reports them: `plenty --check`, `plenty --check-module`,
`plenty check`, and the [language server](language-server.md). Compiling and
running do not, because a [program cache hit](../02-architecture/cache.md)
skips the frontend that finds them. Run a check to see a program's warnings.

The command line prints each warning on stderr after the check passes, sorted
by file and position:

```text
main.plenty:3:14: warning: `copy(original)?` is redundant: `str` copies on assignment and when passed or returned; write `original` [redundant-copy]
```

The bracketed name is the lint's stable identifier. The language server
publishes the same text with warning severity and the name as the diagnostic
code. A check that fails reports only its errors.

There is no option to disable a warning or silence one site. Every warning has
a replacement that keeps the program's behavior; apply it.

`std` code and the bodies of generic functions and generic class or enum
methods report no warnings: a generic body is checked per instance, and code
that is redundant for one type argument can be needed for another.

Only top-level function bodies are checked today. Class and enum method
bodies, including `copy(self)?`, and anonymous function bodies report no
warnings, even when they are not generic (issue #846).

## Lints

| Name | Reports |
|---|---|
| `redundant-copy` | `copy(x)?` or `copy(&x)?` when the type of `x` already copies |

### `redundant-copy`

A value whose type [copies](../../language/05-ownership/01-bindings-moves-and-copies.md)
is copied by plain use: assigning, passing, or returning it. `copy(x)?` then
produces the same value as `x`, with a `Result` and an `AllocError` path that
cannot add anything. The lint uses the checker's own copy rule for the
instantiated type, so `str`, numbers, `bool`, `bytes`, copyable tuples and
sums, and `@copy` classes such as `Point[i64]` are reported, and a list or
ordinary class is not.

It reports `copy(x)?` only when a plain expression instead keeps the type,
evaluates the same expression once, and moves nothing. `x` must be one of:

- a local that is not a reference or view;
- a field path from such a local, such as `record.name`;
- a list, dictionary, or tuple element, such as `words[i]`;
- a string, number, boolean, or bytes literal;
- a shared borrow of one of the first three, such as `copy(&s)?` or
  `copy(&record.name)?`; write the place itself, `s` or `record.name`;
- a local shared reference `r: &T`, such as a `&str` parameter or a loop
  variable over `&words`; write `*r`, which reads the copyable value. Before
  `.` or `[`, as in `copy(p)?.x`, the message suggests `(*p)`, because
  `*p.x` dereferences the field instead.

For a name or field path the message spells the replacement. These are left
alone:

- `copy(x)` used as a `Result`, for example matched or stored, rather than
  propagated with `?`;
- an exclusive reference, such as `copy(m)?` for `m: &mut str`, a
  dereference such as `copy(*r)?`, and a borrowed literal;
- a call or other computed argument, such as `copy(f())?`;
- `copy(x)?` of a type that moves, which makes an independent value,
  including `copy(&items)?` for a list.

```plenty
def main() -> Result[(), AllocError | IoError]:
    original = "hello"
    copied = original
    print(original, copied)?
    Ok(())
```

```output
hello hello
```

Removing a `copy(...)?` can leave a function's `AllocError` unused. The
function still checks; narrow its declared error type if nothing else needs it.
