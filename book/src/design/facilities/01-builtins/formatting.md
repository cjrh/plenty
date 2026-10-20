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
Option and printable aggregates containing Results are allowed. Neither operation
invokes user-defined formatting methods. Their Results must be handled or explicitly
dropped, following the [error rules](../../language/08-errors/index.md).

[f-strings](../../language/04-expressions/format-strings.md) build a `str`
from fields, with Python format specs checked at compile time; fields without
a spec use the same spellings as `print`.

Automatic formatting rejects stored recursive types even when an individual value
is shallow. Print selected scalar fields or write an explicit traversal.
A top-level closure, generator or scoped task, and sums that hold one (for example
`SpawnError[OnceClosure[[], i64]]`), cannot be formatted by `str.repr`. The
diagnostic names the unformattable kind and, when it is nested, the containing
type: match the value and format its parts, or call a closure and format its
result. `print` renders a closure as an opaque marker.
Numeric spelling follows [scalar formatting](numbers.md); stream behavior follows
[standard output](../02-io/streams-args.md).
