# Reverse traversal

`reversed(iterable)` is a source for loops and comprehensions over a list,
string, `bytes`, `std.bytes.Bytes`, or range. It evaluates the source once and
visits its elements from last to first, without changing the source order or allocating a reversed
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
borrowed inputs, or as a [pipeline](06-pipelines.md) source:
`iter(reversed(&values))`. Pipelines have no `reversed` method. Its own input must be a concrete list, string, byte literal,
byte buffer, or range; it cannot reverse `enumerate`, `zip`, a generator,
or a hash collection.
It does not create a stored iterator or dispatch to a user iterator protocol.

There are no free `any` and `all` functions. Test boolean items with the
[pipeline terminals](06-pipelines.md#terminals) `iter(flags).any()` and
`iter(flags).all()`, or pass a predicate: `iter(values).any(is_negative)`.
`reversed` may supply their input: `iter(reversed(&flags)).all()`. A call to
`any(...)` or `all(...)` that names no user function is an error naming these
replacements.

Functions and callable bindings named `reversed`, `any`, or `all` retain
their normal call behavior. Imported generic functions and generator bodies
can use `reversed`.
