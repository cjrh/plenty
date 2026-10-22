# Pattern matching

`match` evaluates its scrutinee once and selects a `case`. The scrutinee is an
enum, an [anonymous sum](../03-types/anonymous-sums.md), an integer, a `str`,
or a `bool`, or a reference to one:

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
| `_ as name` | the whole matched value, bound to an immutable local |
| `name: MemberType` | a whole member of an anonymous sum; see [member patterns](../03-types/anonymous-sums.md) |
| `Enum.Variant` or `Enum.Variant(p, ...)` | that variant, with one pattern per payload position |
| `Some(p)`, `Nothing`, `Ok(p)`, `Err(p)` | that standard variant |
| `1`, `-1`, `0u8` | an integer equal to the literal |
| `"text"` | a `str` equal to the literal |
| `True`, `False` | that `bool` |
| `p \| q` | a value either alternative matches |

A user-defined or library variant is always qualified by its enum's name, a
module path to it, or an alias of the matched type. `Some`, `Nothing`, `Ok`, and
`Err` are never qualified. A pattern takes no type arguments, because the
matched value supplies them: `case Choice.Value(v)` matches a `Choice[i64]` and
a `Choice[str]` alike, and `case Choice[i64].Value(v)` is an error.

A bare name binds only at a payload position; the case's own pattern is a
variant, a literal, `_`, alternatives of those, `_ as name`, or a typed member
pattern. A bare name as the case's own pattern is an error, and a payload name equal to a variant of
that payload's enum is rejected, so a forgotten qualifier cannot silently bind
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

A literal pattern matches an integer, `str`, or `bool` position: the scrutinee
itself, a payload, or the contents of a `Box`. An unsuffixed integer literal
takes the position's type and must fit it; a suffixed one must name that type
exactly. Float literals are not patterns.

```python
def opcode(name: str) -> i64:
    match name:
        case "cpy":
            1
        case "inc" | "dec":
            2
        case _:
            0

def describe(reading: Option[i64]) -> str:
    match reading:
        case Some(0):
            "zero"
        case Some(_):
            "nonzero"
        case Nothing:
            "missing"
```

`case _ as name:` matches every value the earlier cases leave and binds the
whole matched value, for any supported scrutinee type. It does not make floats,
collections, or other unsupported scrutinees matchable.
It is accepted only as a case's entire pattern: not as a `|` alternative,
and not at a payload position, where
the name alone binds. Because it matches everything, a case after it is
unreachable, and it is itself unreachable when earlier cases are exhaustive.
`name` cannot be a variant of the matched enum, `_`, or `mut`.

```python
def forward(shape: Shape) -> i64:
    match shape:
        case Shape.Dot:
            0
        case _ as other:
            size(Some(other))
```

Every alternative of `p | q` binds the same names with the same types, at any
depth. Cases are tried in order and the first match runs. Bindings stay within
the arm and may shadow outer names.

Coverage must be exhaustive; the error lists missing values as patterns, such
as `missing Ok(Some(Shape.Dot)), Ok(Nothing)`. A case, or a single alternative,
that earlier patterns already cover is an error. Incorrect payload arity and
wrong enum identities are errors. A pattern expands to at most 256 combinations
of its alternatives.

Literals never cover an integer or `str` position: such a position also needs
`_` or a payload name, and a match on an integer or `str` ends with `case _:`
or `case _ as name:`. `True` and `False` together cover a `bool`, so a `_` after
both is unreachable. A
repeated literal is an unreachable case.

Literal cases are tested in source order, one comparison each; a `str` case
compares lengths and then bytes. No jump table or hash is built, so a match
with many literal cases costs as much as the equivalent `if`/`elif` chain.
Enum tags use the same comparison-chain dispatch. Large dense matches can
therefore be sensitive to which cases occur most often; the
[dispatch measurements](../../../development/profiling.md#dense-match-dispatch)
describe the current cost and an experimental optimization. Keep using `match`
for clear exhaustive branching; the experiment does not change its behavior.

Guards, tuple patterns, float patterns, range patterns, and `pattern as name`
for any pattern other than the case's own `_` are unsupported. A scrutinee of any other type, such as a float or a
collection, is rejected with advice to use an `if`/`elif` chain.

Payload and whole-value bindings cannot be declared `mut`. To change an owned
payload, move it into a mutable local inside the arm (`mut local = payload`); to change it in
place, match `&mut value`.

Continuing arms must agree on their result type. Arms ending in `return`, `break`,
or `continue` contribute no join value. A function-tail match supplies the final
expression of its selected arm, just like a tail conditional. A match elsewhere
in a suite is a statement, and its arms' values are discarded, unless it is
[a statement's value](#a-match-as-a-statements-value).

## A match as a statement's value

A `match` block can be the whole value of these statements, and of no other
position:

| Statement | Effect |
|---|---|
| `name = match value:`, with `mut` or an annotation as for any binding | declares the local, or assigns an existing `mut` local |
| `place = match value:`, where `place` is a field, a collection element, or `*reference` | assigns the place |
| `return match value:` | returns from the function |

```python
def area(shape: Shape) -> i64:
    side: i64 = match shape:
        case Shape.Dot:
            return 0
        case Shape.Circle(side):
            side
        case Shape.Rect(width, height):
            if width < height:
                width
            else:
                height
    side * side
```

The statement ends with the last case; nothing follows the block on its line.
Patterns, exhaustiveness, and ownership of the scrutinee are those of any
other match.

In a binding or assignment, the final expression of the selected case is the
value. A final `if` or `match` supplies the value of its selected suite.

- Continuing cases must agree on one type. An annotation, or the type of the
  assigned local, is the expected type of every case's final expression, as it
  is for an ordinary right-hand side; without one, each case is inferred alone,
  so `Nothing` or `[]?` needs the annotation.
- A case that ends in `return`, `break`, `continue`, or a propagating `?` gives
  no value. At least one case must continue; a match whose cases all exit is
  rejected with advice to write it as a statement.
- A case whose final statement has no value, such as `pass` or an assignment,
  makes the match unit, which cannot be bound.
- The value cannot be a reference or a closure with borrowed captures, because
  the case's bindings end before the statement uses it. Produce an owned value,
  or declare `mut name` and assign in each case.
- Earlier expressions in a case are discarded as in any suite, so an unhandled
  `Result` there is still an error.

Evaluation order is: the scrutinee; the selected case's suite; cleanup of that
case's bindings and locals, in reverse declaration order, and of scrutinee
parts no pattern bound; then the binding or assignment. An assignment
evaluates [destination indices](assignment.md) and destroys the replaced value
after all of that. An exit from a case performs the ordinary cleanup for that
exit and assigns nothing.

`return match value:` returns the final expression of the selected case, exactly
as if that case ended with `return expression`. Every case therefore exits, a
call there is a [source-tail call](../06-functions/05-tail-calls.md), and a
statement after the block is unreachable. A case that ends without a value
returns unit, which is an error unless the function returns unit. A returned
reference follows the rules of any other [returned
reference](../05-ownership/04-returned-references.md).

Augmented assignment, assignment to several targets, call arguments, operands,
and every other expression position reject a `match` block; bind its value to a
name first. `if` blocks are never values in these positions: use the inline
conditional, or a `match` on the `bool`.

## Ownership

Owned matching consumes the scrutinee: bound payloads move into their bindings
and every part no pattern binds is destroyed. `_ as name` moves the whole value
into `name`, with no copy or allocation. Testing a literal copies the
integer, `str`, or `bool` it compares, so a later pattern can still bind or
move the value; for a `str` the copy shares the text and allocates nothing. Matching a shared or exclusive
reference binds shared or exclusive payload references instead; see
[borrowed matching](../05-ownership/index.md). Unit payload bindings produce unit
expressions and can be returned from a unit-returning function.
