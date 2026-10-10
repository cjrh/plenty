# Binding and assignment

`name = expression` first declares an immutable local with inferred type.
`name: Type = expression` adds an annotation. `mut name = expression` or
`mut name: Type = expression` declares a mutable local.

Later `name = value` assigns to an existing local and requires `mut` and the same
type. Repeating `mut` or an annotation for that name is a duplicate declaration.
Parameters are immutable, and declarations always have an initializer. Copies,
moves, and exclusive access follow [ownership](../05-ownership/index.md).

Field and indexed assignment accept nested writable paths such as
`rows[0][-1] = 9` and `records[0].values[1] = 7`. The root must be mutable or an
exclusive reference. Existing list slots and dictionary values change in place,
without copying or allocating their containing owners. A missing dictionary key
is an error; use fallible `insert` to add one.

Assignment proceeds in this order:

1. Evaluate the right-hand value.
2. Evaluate destination indices once each, from root to leaf.
3. Resolve the destination using the resulting contents, checking bounds or keys.
4. Replace the destination while holding exclusive access.

Index expressions can read or resize the destination. Their values are saved
before any address is resolved, and no user expression runs between address
resolution and the write. Replacing an owned value destroys its previous value
exactly once. If an index propagates an error, a pending right-hand owner is
cleaned up and no write occurs.

`replace(owner.field, replacement)` follows the same evaluation order but returns
the previous field value to the caller instead of destroying it. It requires the
same type, visibility, mutability, and loan checks. It supports nested fields,
boxed owners, and fields reached through collection elements. It allocates
nothing and keeps the field initialized. It accepts a field rather than a whole
binding, element, or temporary receiver. A user declaration named `replace`
continues to resolve normally.

Whole-instance assignment through `*reference = instance` is rejected for class
references, including class-valued field references. Replace the owning binding
or a named field instead.
