# Tuples

`(a, b)` creates a tuple; `(value,)` creates a one-component tuple. Parentheses
without a comma group an expression, and empty `()` is unit. Annotations accept
`tuple[A, B]` or `(A, B)`. A one-component type requires `(A,)` or `tuple[A]`;
`(A)` groups the type `A`, just as parentheses group an expression.

Tuples are structural products with fixed component types. Construction allocates nothing;
component expressions retain their own allocation and error contracts. Tuples
containing owned components move, while copyable tuples copy their components.
Resolved owned closures can be stored; references and generators cannot.

A `mut` tuple binding permits mutable component borrows regardless of whether
its components are copyable. A copied tuple has independent inline storage;
changing the original does not change the copy. Components can be changed
through `&mut pair[0]` or a mutable payload match. Direct indexed assignment
(`pair[0] = value`) remains unsupported; assign through the reference instead.

`pair[0]` selects a component. Indices must be nonnegative integer literals within
the tuple's arity. Owned components may be observed or explicitly copied; moving
them out requires whole-tuple unpacking:

```python
number, text = (42, "cm")
number, text = 42, "cm"
```

Flat unpacking transfers each component; `_` destroys a discarded component.
The parentheses are optional on the right of an unpacking statement, and targets
may also be fields and collection elements: see
[assignment to several targets](../04-expressions/assignment.md#several-targets).
Nested and starred unpacking are unsupported. Unit components can be discarded
or used as unit expressions without enabling standalone unit storage.

An owned list also supports flat unpacking with an exact runtime length check;
see [collection rules](../../facilities/01-builtins/collections.md).
Tuples of integers, `bool`, strings, and nested tuples of those types are
copyable dictionary keys and set members. Hashing and equality use their
contents, with no formatting or string-key encoding.

Tuples whose components are integers, floats, strings, or recursively orderable
tuples support `<`, `<=`, `>`, and `>=`. Both operands must have exactly the same
type. They compare lexicographically, using the first differing component, with
the same allocation-free total order as `list.sort()`. This also works through
references and in generic functions; comparison observes both operands.
Booleans, unit, classes, ordinary enums, and collections are not orderable fields.

Floating-point tuple fields use the sorting order: negative zero precedes positive
zero, and all NaNs compare equal in order and follow every number. Structural `==`
and `!=` still use IEEE equality. Consequently, `(-0.0,) == (0.0,)` and
`(-0.0,) < (0.0,)` are both true; a NaN tuple is `<=` itself but not `==` itself.
Scalar float operators keep their [IEEE behavior](numbers.md).

Distinct components of a named tuple can be borrowed independently. Reads of
one component can continue while another is exclusively borrowed; whole-tuple
access overlaps all components. See [borrowing](../05-ownership/index.md) for
projection and collection-element loan rules.
