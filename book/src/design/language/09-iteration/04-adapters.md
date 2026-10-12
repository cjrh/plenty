# Enumeration and paired iteration

`enumerate(iterable[, start: i64])` and `zip(left, right)` are intrinsic
sources for `for` loops and comprehensions. They compose the existing loop
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
used as an outer adapter's tuple-valued input. `dictionary.items()` retains
its separate two-binding contract and is not an adapter input.

The calls do not construct first-class iterators, support arbitrary iterator
protocols, or implement keyword arguments or zip strictness. Outside a loop
source they report this restriction explicitly. [Reverse traversal](05-reverse-and-reduce.md)
can supply either input. A user function or callable
binding named `enumerate` or `zip` retains its normal call behavior.
