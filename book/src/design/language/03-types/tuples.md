# Tuples

`(a, b)` creates a tuple; `(value,)` creates a one-component tuple. Parentheses
without a comma group an expression, and empty `()` is unit. Annotations accept
`tuple[A, B]` or `(A, B)`.

Tuples are immutable structural products. Construction allocates nothing;
component expressions retain their own allocation and error contracts. Tuples
containing owned components move, while copyable tuples copy their components.
Resolved owned closures can be stored; references and generators cannot.

`pair[0]` selects a component. Indices must be nonnegative integer literals within
the tuple's arity. Owned components may be observed or explicitly copied; moving
them out requires whole-tuple unpacking:

```python
number, text = (42, "cm")
```

Flat unpacking transfers each component; `_` destroys a discarded component.
Nested and starred unpacking are unsupported. Unit components can be discarded
or used as unit expressions without enabling standalone unit storage.

Distinct components of a named tuple can be borrowed independently. Reads of
one component can continue while another is exclusively borrowed; whole-tuple
access overlaps all components. See [borrowing](../05-ownership/index.md) for
projection and collection-element loan rules.
