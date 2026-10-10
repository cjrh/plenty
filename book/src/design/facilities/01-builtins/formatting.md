# Formatting values

`str.repr(value) -> Result[str, AllocError]` observes a printable value, including
references, and produces its representation. Strings are quoted and escaped;
aggregates display their structure. Direct Result wrappers can be inspected this
way. Formatting uses checked buffer growth and preserves the input on failure.

`print(value) -> Result[(), IoError]` observes its value, writes a trailing
newline, and uses raw top-level strings. Printing an existing string needs no
temporary formatting allocation; other values use checked growth. Formatting
failure emits no bytes; output errors can leave a written prefix.

Print rejects a direct or borrowed Result: handle it first, or intentionally
format the wrapper with `str.repr(result)` and handle that formatting Result.
Option and printable aggregates containing Results are allowed. Neither operation
invokes user-defined formatting methods. Their Results must be handled or explicitly
dropped, following the [error rules](../../language/08-errors/index.md).

Automatic formatting rejects stored recursive types even when an individual value
is shallow. Print selected scalar fields or write an explicit traversal.
Numeric spelling follows [scalar formatting](numbers.md); stream behavior follows
[standard output](../02-io/streams-args.md).
