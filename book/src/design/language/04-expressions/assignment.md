# Binding and assignment

`name = expression` first declares an immutable local with inferred type.
`name: Type = expression` adds an annotation. `mut name = expression` or
`mut name: Type = expression` declares a mutable local.

The value of a binding or a single-target assignment is an expression or a
[`match` block](matching.md#a-match-as-a-statements-value), whose selected case
supplies it.

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

Whole-instance assignment through `*reference = instance` is rejected for class
references, including class-valued field references. Replace the owning binding
or a named field instead.

## Augmented assignment

`target += value`, `-=`, `*=`, `/=`, `//=`, and `%=` update an existing numeric
destination. The destination can be a mutable binding, field, collection element,
or exclusive reference, including nested paths. The right side takes the
destination's numeric type. Integer division uses `//=`, float division uses
`/=`, and `%=` currently accepts integers. Strings and collections do not
support augmented assignment; string concatenation remains fallible and explicit.

Unlike ordinary assignment, augmented assignment evaluates its target before
the right side:

1. Evaluate each target index once, from root to leaf, saving its value.
2. Read the destination's current scalar value.
3. Evaluate the right side and apply the arithmetic operation.
4. Write back to the same destination using the saved indices.

A shared loan protects the destination during right-side evaluation. The right
side may observe it (`count += count`), but cannot move, replace, or resize its
owner. Collection loans conservatively cover the whole collection. Index
expressions may mutate the owner before the destination is read. If an index or
the right side propagates an error, the update performs no write. Missing keys
and invalid indices trap as in ordinary indexed assignment. The update itself
allocates nothing, introduces no binding, and cannot initialize an unread
constructor field.

## Several targets

`target, target = value, value` assigns to two or more targets at once:

```plenty
def main() -> Result[(), Failure]:
    mut items = [10, 20, 30]?
    items[0], items[2] = items[2], items[0]
    print(items)?
    mut low = 1
    mut high = 2
    low, high = high, low
    print((low, high))?
    Ok(())
```

```output
[30, 20, 10]
(2, 1)
```

The right side is one tuple. With several targets its parentheses are optional,
so `a, b = b, a` means `a, b = (b, a)`; a single target still requires them. Its
component count must equal the target count.

1. Evaluate the whole right side.
2. Assign each component to its target, from left to right, exactly as a single
   assignment to that target would.

Because the right side is complete before the first write, targets can exchange
values. Each target evaluates its own indices when its turn comes, so an earlier
target can change a later target's index: `i, items[i] = 2, 5` writes `items[2]`.
A target is a name, a field, a collection element, `*reference`, or `_`, which
destroys its component. A name follows the single-assignment rule: it assigns an
existing `mut` local or declares a new immutable one. `mut a, b = value`
declares mutable locals and accepts names only. Naming the same local twice,
nesting targets, starred targets, and a closure capture as a name target are
rejected. In a constructor, a `self.field` target initializes its field, and
later targets may read it.

The right side obeys ordinary ownership, so reading an owned element such as a
nested list still cannot move it out of its collection. Exchange owned list
elements with [`swap`](../../facilities/01-builtins/lists.md). When a target
traps on a missing index or key, earlier targets have already been assigned.
Nothing is allocated.

## Replacing a field

`replace(owner.field, replacement)` follows the same evaluation order but returns
the previous field value to the caller instead of destroying it. It requires the
same type, visibility, mutability, and loan checks. It supports nested fields,
boxed owners, and fields reached through collection elements. It allocates
nothing and keeps the field initialized. It accepts a field rather than a whole
binding, element, or temporary receiver. A user declaration named `replace`
continues to resolve normally.
