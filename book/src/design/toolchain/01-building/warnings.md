# Warnings

A warning reports accepted code that may warrant a change. Warnings never reject a program and never change an exit status.

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
code. A command-line check that fails reports only its errors. Editor checks
can retain warnings established independently of an error elsewhere.

There is no option to disable a warning or silence one site. A warning is advice:
review its suggestion before changing the program, especially when an import
intentionally keeps a module in checking.

`std` code reports no warnings. The `redundant-copy` lint also excludes
the bodies of generic functions and generic class or enum methods: a generic
body is checked per instance, and code that is redundant for one type argument
can be needed for another.

For `redundant-copy`, only top-level function bodies are checked today. Class
and enum method bodies, including `copy(self)?`, and anonymous function bodies report no
warnings, even when they are not generic (issue #846).

## Lints

| Name | Reports |
|---|---|
| `unused-import` | An explicit import binding has no resolved use in its module |
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

### `unused-import`

Each explicit import binding is checked in its defining module, independently
of whether an executable calls that module's declarations. Uses in signatures,
type aliases, class and enum declarations, methods, closures, patterns and generic
bodies count. Aliases are tracked by the binding actually resolved; a local
variable with the same spelling does not use the import. Compiler-inserted
imports and standard-library sources report no import warnings.

Only the selected `if const` branches participate. An import used only on another
target can therefore warn on this target; put both the import and its uses under
the corresponding target condition when appropriate. Package checking checks each
library and executable target's import graph; tests use their own checked graph.
A warning about one target does not mean that another target's import is unused.

The warning points to the imported name or alias and uses the stable identifier
`unused-import`. It neither removes an import nor stops loading its module.
Imports also determine validation coverage: retain an unused import when it
intentionally includes a module in checking. Removing that import can make the
module unreachable from all package targets; `plenty check` then reports the
unimported source separately. No automatic removal is offered.
