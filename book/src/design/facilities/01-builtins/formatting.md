# Formatting values

`str.repr(value) -> Result[str, AllocError]` observes a printable value, including
references, and produces its representation. Strings are quoted and escaped;
aggregates display their structure. Direct Result wrappers can be inspected this
way. Formatting uses checked buffer growth and preserves the input on failure.

`print(values...) -> Result[(), IoError]` observes its arguments, writes a trailing
newline, and uses raw top-level strings. Printing an existing string needs no
temporary formatting allocation; other values use checked growth. Formatting
failure emits no bytes; output errors can leave a written prefix.

Zero arguments print a newline. Multiple arguments evaluate once from left to
right before output, with one space between values and raw top-level strings.
Their source loans remain live through argument evaluation and printing;
later arguments cannot mutate an earlier observed owner. Printing keeps every
owner intact and does not build an intermediate tuple or list. Multiple values
use a checked formatting buffer, even when all arguments are strings.

Print rejects a direct or borrowed Result in any argument: handle it first, or intentionally
format the wrapper with `str.repr(result)` and handle that formatting Result.
Option and printable aggregates containing Results are allowed. Their Results
must be handled or explicitly dropped, following the
[error rules](../../language/08-errors/index.md).

[Quantities](../../language/03-types/quantities.md#formatting) display their
coherent number and unit at the top level. Debug output, including in
aggregates, uses `Quantity[T, D](value)`. These builtin spellings keep the
ordinary `print` and `str.repr` error types.

## User formatting methods

`print`, `str.repr` and f-strings share one formatting path. A class or enum
controls its spelling with two methods that write to a `std.fmt.Formatter`:

- `display` writes the text `print(x)` and `f"{x}"` produce at the top level;
- `debug` writes the text `str.repr(x)` and `f"{x!r}"` produce, and the
  spelling of `x` as an element of any formatted aggregate.

A type without `display` uses its `debug`; a type without `debug` uses its
structure. Strings are raw at the top level and quoted inside aggregates, so
`print(["a"])` writes `["a"]`.

```plenty
from std.fmt import Formatter

class P:
    n: i64

    def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"<{self.n}>")

    def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.format(f"P#{self.n}")

def main() -> Result[(), AllocError | IoError | FormatError]:
    p = P(3)
    print(p)?
    print(str.repr(p)?)?
    print([P(3)]?)?
    print(f"{p} {p!r}"?)?
    Ok(())
```

```output
<3>
P#3
[P#3]
<3> P#3
```

When a value's type, or any type it holds, has one of these methods, the
method can fail, so the operation's error widens with `std.fmt.FormatError`:
`print` returns `Result[(), IoError | FormatError]`, `str.repr` returns
`Result[str, AllocError | FormatError]`, and an f-string returns
`Result[str, AllocError | FormatError]`. Other values keep the narrower types:
`Result[(), IoError]`, `Result[str, AllocError]` and `Result[str, AllocError]`.
A method's error is returned unchanged and nothing is printed. The method
runs with no byte budget beyond the operation's own and the nesting depth of
an f-string field (32); a private type's method runs with its own module's
access, so an aggregate holding another module's private type still formats.

`display` and `debug` are reserved for this form. Formatting a type whose
method of either name has another signature is a compile error that names
the type, the method and the required signature; rename the method if it
serves another purpose. `std.log.Logger` (a `debug` that logs) and
`std.cli.Progress` (a `display` that draws) are such types, so they cannot be
printed.

`main`'s error report and runtime trap messages do not call these methods;
they use structure (issue #634).

[f-strings](../../language/04-expressions/format-strings.md) build a `str`
from fields, with Python format specs checked at compile time. A field without
a spec uses the same spelling as `print`.

A [`@redacted`](../../language/03-types/classes.md#redacted-types) class or
enum without the method in use formats as its type name followed by `(@redacted)` in `str.repr`, `print`,
f-string fields with any conversion or spec, and `main`'s error report, at any
nesting depth. The marker uses the same checked buffer as ordinary formatting.

Automatic formatting rejects stored recursive types even when an individual value
is shallow. Print selected scalar fields or write an explicit traversal.
A top-level closure, generator or scoped task, and sums that hold one (for example
`SpawnError[OnceClosure[[], i64]]`), cannot be formatted by `str.repr`. The
diagnostic names the unformattable kind and, when it is nested, the containing
type: match the value and format its parts, or call a closure and format its
result. `print` renders a closure as an opaque marker.
Numeric spelling follows [scalar formatting](numbers.md); stream behavior follows
[standard output](../02-io/streams-args.md).
