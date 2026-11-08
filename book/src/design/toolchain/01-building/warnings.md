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

`std` code reports no warnings. The `redundant-copy` lint checks function
bodies, class and enum method bodies, and anonymous function and closure bodies.
It excludes code the compiler generates, generic function and generic class or
enum method bodies, and anonymous functions and closures inside those generic
bodies: a generic body is checked per instance, and code that is redundant for
one type argument can be needed for another.

## Lints

| Name | Reports |
|---|---|
| `unused-import` | An explicit import binding has no resolved use in its module |
| `redundant-copy` | `copy(x)?` or `copy(&x)?` when the type of `x` already copies |
| `text-view-parameter` | A `str` or ordinary `&str` parameter only supplies recognized text reads |

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
  `*p.x` dereferences the field instead. A shared `self` receiver is such a
  reference, so `copy(self)?` in a method of a `@copy` class suggests
  `*self`;
- a closure capture, such as `k` in `def[k]` or `def[&k]`; write `k`, since
  the closure body reads the captured value by name.

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

### `text-view-parameter`

A text reader can usually accept a whole string and a substring through
`&strview`. This lint suggests that type for a `str` or ordinary `&str`
parameter when all its uses are recognized text reads. For example:

```text
parameter `text` only supplies text reads; consider `&strview` instead of `str` to also accept substring views [text-view-parameter]
```

The warning points to the parameter's type. A caller still passes a whole
string normally; a borrowed range now works too:

```plenty
def byte_count(text: &strview) -> i64:
    text.byte_len()

def main() -> Result[(), IoError]:
    text = "héllo"
    print(byte_count(text), byte_count(&text[1:3]))?
    Ok(())
```

```output
6 3
```

This is advice about accepted inputs, not a promise of faster execution.
A view's `byte_len()` is constant time, but scalar `len(view)` scans the text;
owned strings cache their scalar count. Review repeated length queries before
changing the signature. See [substring views](../../language/04-expressions/slices.md#substring-views).

The analysis is deliberately conservative. It recognizes direct parameter uses
in scalar iteration, indexing and copying slices, builtin text methods,
`len`, `ord`, printing and formatting, text operations with literal operands,
and forwarding to a resolved `&strview` input (including named arguments).
An unused parameter is not reported. Any unrecognized use suppresses the
suggestion, including aliasing, returning or storing the owning handle, taking
an ordinary reference, explicit dereferencing, or passing it to an owning,
ordinary-reference or generic input. The lint does not infer that another
`str` reader should change in the same edit.

Methods, generic functions and their instances, anonymous/generated bodies,
foreign functions, C exports, functions with defaults, generators and functions
returning loans are excluded. A function used as a value anywhere in the
checked user graph keeps its callable signature, including uses in generic
bodies that have not been instantiated. Parameter shadowing and captures also
suppress advice. Standard-library sources report no warnings.

The lint does not automatically rewrite signatures. Ordinary `&str` remains
appropriate for generic references, returned references, retained string handles
and fixed callback or protocol contracts. Public API changes still need review;
absence of a warning does not mean that a text view would be unsuitable.
