# Reverse traversal and boolean reductions

`reversed(iterable)` is a source for loops and comprehensions over a list,
string, or range. It evaluates the source once and visits its elements from
last to first, without changing the source order or allocating a reversed
copy. Text is traversed by Unicode scalar value, with linear total byte
traversal; combining marks remain separate scalars. Ranges retain their
element type and original step, including descending and unsigned ranges.

The [ordinary iteration ownership rules](01-iterables.md) apply. An owned
list transfers its elements and drops its unvisited elements on early exit.
`reversed(&values)` yields shared element references; `reversed(&mut values)`
yields exclusive references for a list. A borrowed temporary source stays
alive until the loop scope exits. `continue` advances backward once, and
`break`, return, and propagation clean up the remaining source and current
item. Flat tuple unpacking and comprehensions work as in forward iteration.

Use `reversed` as an input to [enumerate or zip](04-adapters.md), including
borrowed inputs. Its own input must be a concrete list, string, or range;
it cannot reverse `enumerate`, `zip`, a generator, or a hash collection.
It does not create a stored iterator or dispatch to a user iterator protocol.

`any(iterable)` and `all(iterable)` return `bool`. They accept owned or
borrowed boolean lists, sets, dictionary keys, and owned boolean generators.
`reversed` may supply their boolean input. There is no implicit truth-value
conversion: use comparisons to produce booleans from other types.

| Reduction | Stops at | Empty input |
| --- | --- | --- |
| `any` | First `True` | `False` |
| `all` | First `False` | `True` |

The source expression evaluates once. Generator bodies run only until the
answer is determined; remaining frame state is then dropped. A consumed list
or generator is unavailable afterward. Borrow the list to preserve it.
These reductions allocate no result container or iterator object. An input
expression can still allocate, and an eager list comprehension builds its
whole list before reduction. Generator expressions remain unsupported; use
a named generator for incremental predicate evaluation.

Functions and callable bindings named `reversed`, `any`, or `all` retain
their normal call behavior. Imported generic functions and generator bodies
can use the intrinsics.
