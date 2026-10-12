# Comprehensions and collection construction

List, set, and dictionary comprehensions return
`Result[collection, AllocError]`. Multiple `for` and `if` clauses nest from
left to right, each one a level of
[nesting](../04-expressions/index.md#nesting). Each iterable evaluates when its enclosing iteration reaches
it; filters run before the result expression. Dictionary keys evaluate before
their values. Iteration uses the same consuming or borrowed contracts as
[loops](01-iterables.md).

```plenty
def main() -> Result[(), Failure]:
    values = [n * n for n in range(5) if n > 1]?
    print(values)?
    Ok(())
```

Comprehension bindings have their own scope; the first iterable sees the
enclosing scope. Flat tuple targets are supported. Conditional expressions in
iterables or filters must be parenthesized. Parenthesized
[generator expressions](03-generators.md#generator-expressions) produce lazy
owned frames instead of collection Results. Async comprehensions are unsupported.

Output ownership and growth are fallible. On growth failure, the partial output,
current yielded owners, and active hidden iterators clean up. No iterator
advances and no later filter or element expression runs. Nested expressions
keep their own Result types and failure behavior. A nested `?` propagates from
the enclosing function rather than becoming an error inside the surrounding
comprehension Result. Already performed side effects are not rolled back.

Collection displays `[elements]`, `{elements}`, and `{key: value}` follow the
same recoverable output-allocation policy. Their owner allocates before entries
evaluate left to right; allocation failure prevents later entry evaluation.
Expected Result types supply element types for empty displays. See
[collection construction](../../facilities/01-builtins/collections.md) for literal
types and duplicate-entry behavior.

`list[T].from(source)` and `set[T].from(source)` consume an owned collection,
range, or generator and return Result with `AllocError`. Failure destroys
partial output, the current element, and remaining source. Generator effects
before failure persist. Sets require hashable elements and keep the first
occurrence of each value. Borrowed sources and strings are unsupported by
these constructors.

A directly written range in a comprehension can receive numeric element
context from the output annotation through direct arithmetic, for example
`squares: list[u8] = [n * n for n in range(8)]?`. This inference does not run
backward through arbitrary calls, filters, stored ranges, or explicit range
type arguments. Use `range[u8](...)` at those boundaries. Fixed-width
arithmetic follows its normal overflow rules.
