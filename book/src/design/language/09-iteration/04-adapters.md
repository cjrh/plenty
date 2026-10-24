# Enumeration and paired iteration

`enumerate(iterable[, start: i64])` and `zip(left, right)` are intrinsic
sources for `for` loops and comprehensions, and can start local
[pipelines](06-pipelines.md). Pipelines also have `.enumerate(start=0)` and
`.zip(right)` methods. They compose the existing loop
plans and allocate no iterator object, snapshot, or item buffer. Inputs
evaluate once in source order; both zip inputs evaluate even if one is empty.
Owned items use inline tuples, allowing one tuple binding or two unpacked
bindings. Nested adapters work with owned items.

Enumeration yields `(i64, element)`, starting at zero by default. Its counter
increments with checked arithmetic only after another element is available.
A last index of `i64.MAX` is valid; a further item traps at the adapter's
source location. `continue` advances the source and keeps the count aligned.

Zip yields `(left element, right element)` and stops when either input ends.
It tests the left condition before the right, including generator advancement.
When the right generator ends first, the left can have one unmatched pending
item; cleanup drops it. Generator bodies therefore may have one extra left
side effect. `break`, return, and propagation drop all remaining owned inputs.

Each input follows the [ordinary iteration ownership rules](01-iterables.md).
Borrowed elements retain their reference types and source loans. Such adapters
require two unpacked bindings and bind the references directly, without
constructing a tuple containing references. Aliasing exclusive inputs and
invalidating mutation remain errors. An inner borrowed adapter cannot be
used as an outer adapter's tuple-valued input. `dictionary.items()` yields
two references; it can start a pipeline but cannot become a nested tuple
containing references.

The calls do not construct first-class iterators, support arbitrary iterator
protocols, or implement zip strictness. Pipeline enumeration also accepts the
`start` keyword. [Reverse traversal](05-reverse-and-reduce.md)
can supply either input. A user function or callable
binding named `enumerate` or `zip` retains its normal call behavior.

The free forms and methods yield the same items: `enumerate(&names, 1)` and
`iter(&names).enumerate(1)` are interchangeable as a loop source or pipeline
start. Use the method form to enumerate or zip partway
through a chain, after a `filter` or `skip`, where it counts or pairs only the
items that reach it. `reversed` is not a pipeline adapter: reverse the source
instead, as in `iter(reversed(&values))` or `zip(reversed(&values), &values)`.
