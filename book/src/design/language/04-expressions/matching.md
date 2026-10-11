# Pattern matching

`match` evaluates an enum scrutinee once and selects a `case`:

```python
enum Reading:
    Missing
    Value(i64)

def amount(reading: Reading) -> i64:
    match reading:
        case Reading.Missing:
            0
        case Reading.Value(number):
            number
```

A case holds one pattern. A pattern is one of:

| Pattern | Matches |
|---|---|
| `_` | any value, binding nothing |
| `name` | any payload, bound to an immutable local |
| `Enum.Variant` or `Enum.Variant(p, ...)` | that variant, with one pattern per payload position |
| `Some(p)`, `Nothing`, `Ok(p)`, `Err(p)` | that standard variant |
| `p \| q` | a value either alternative matches |

A user-defined or library variant is always qualified by its enum's name, a
module path to it, or an alias of the matched type. `Some`, `Nothing`, `Ok`, and
`Err` are never qualified. A pattern takes no type arguments, because the
matched value supplies them: `case Choice.Value(v)` matches a `Choice[i64]` and
a `Choice[str]` alike, and `case Choice[i64].Value(v)` is an error.

A name binds only at a payload position; the case's own pattern is a variant,
`_`, or alternatives of those. A payload name equal to a variant of that
payload's enum is rejected, so a forgotten qualifier cannot silently bind
everything. A variant pattern at a `Box[T]` payload matches the box's contents.
Patterns nest to at most 64 levels.

```python
enum Shape:
    Dot
    Circle(i64)
    Rect(i64, i64)

def size(shape: Option[Shape]) -> i64:
    match shape:
        case Some(Shape.Circle(side) | Shape.Rect(side, _)):
            side
        case Some(Shape.Dot) | Nothing:
            0
```

Every alternative of `p | q` binds the same names with the same types, at any
depth. Cases are tried in order and the first match runs. Bindings stay within
the arm and may shadow outer names.

Coverage must be exhaustive; the error lists missing values as patterns, such
as `missing Ok(Some(Shape.Dot)), Ok(Nothing)`. A case, or a single alternative,
that earlier patterns already cover is an error. Incorrect payload arity and
wrong enum identities are errors. A pattern expands to at most 256 combinations
of its alternatives.

Guards, tuple patterns, literal patterns, whole-value bindings, and scalar
matching are unsupported. A literal pattern such as `case "text":` or `case 1:`,
or a scrutinee that is not an enum, is rejected with advice to use an
`if`/`elif` chain.

Payload bindings cannot be declared `mut`. To change an owned payload, move it
into a mutable local inside the arm (`mut local = payload`); to change it in
place, match `&mut value`.

Continuing arms must agree on their result type. Arms ending in `return`, `break`,
or `continue` contribute no join value. A function-tail match supplies the final
expression of its selected arm, just like a tail conditional.

Owned matching consumes the scrutinee: bound payloads move into their bindings
and every part no pattern binds is destroyed. Matching a shared or exclusive
reference binds shared or exclusive payload references instead; see
[borrowed matching](../05-ownership/index.md). Unit payload bindings produce unit
expressions and can be returned from a unit-returning function.
