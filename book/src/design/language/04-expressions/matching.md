# Pattern matching

`match` evaluates its scrutinee once and selects a `case`. The scrutinee is an
enum, an [anonymous sum](../03-types/anonymous-sums.md), a class, a tuple, an integer, a
`str`, or a `bool`, or a reference to one:

```plenty fragment
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
| `name: B \| C` | the members of an anonymous sum that earlier cases leave, bound as that smaller sum; see [residual members](../03-types/anonymous-sums.md#residual-members) |
| `Enum.Variant` or `Enum.Variant(p, ...)` | that variant, with one pattern per payload position |
| `Some(p)`, `Nothing`, `Ok(p)`, `Err(p)` | that standard variant |
| `(p, q)` or `(p,)` | a tuple with one pattern per component |
| `Class(p, ..., field=p, ...)` | a class value, with one pattern per field; see [class patterns](#class-patterns) |
| `1`, `-1`, `0u8` | an integer equal to the literal |
| `"text"` | a `str` equal to the literal |
| `True`, `False` | that `bool` |
| `p \| q` | a value either alternative matches |

A user-defined or library variant is always qualified by its enum's name, a
module path to it, or an alias of the matched type. `Some`, `Nothing`, `Ok`, and
`Err` are never qualified. A pattern takes no type arguments, because the
matched value supplies them: `case Choice.Value(v)` matches a `Choice[i64]` and
a `Choice[str]` alike, and `case Choice[i64].Value(v)` is an error.

At an anonymous-sum position, an enum variant pattern selects that enum
member before testing its variant. This also works in nested payloads, such
as `Err(ParseError.Invalid)` for `Result[T, AllocError | ParseError]`.
Coverage includes every variant of every member. A generic qualifier that
names several members is ambiguous; select one with a typed member pattern
before matching its variants. See [anonymous sums](../03-types/anonymous-sums.md).

A bare name binds only at a payload or field position; the case's own pattern
is a variant, a class or tuple pattern, a literal, `_`, alternatives of those,
`_ as name`, or a typed member pattern. A bare name as the case's own pattern is an error, and a payload name equal to a variant of
that payload's enum is rejected, so a forgotten qualifier cannot silently bind
everything. A variant pattern at a `Box[T]` payload matches the box's contents.
Patterns nest to at most 64 levels.

```plenty fragment
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

## Tuple patterns

Tuple patterns are recursive fixed-arity products. `(p,)` matches a singleton;
`(p)` groups a pattern. Empty `()` is unit, not an empty tuple pattern; use `_`
for a unit component. A tuple pattern can appear at the root or inside enum,
class, tuple, or boxed payloads. Every component must have a pattern.
At an anonymous-sum position, it selects a unique tuple member with that arity.
If several members have the same arity, bind one with a typed member pattern
before matching its components.

```plenty
def combine(left: Option[i64], right: Option[i64]) -> Result[i64, OverflowError[i64]]:
    match (left, right):
        case (Some(a), Some(b)):
            Ok(a + b)
        case (Some(a), Nothing):
            Ok(a)
        case (Nothing, Some(b)):
            Ok(b)
        case (Nothing, Nothing):
            Ok(0)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(combine(Some(2), Some(3))?)?
    Ok(())
```
```output
5
```

Coverage considers combinations of component patterns, so omitting
`(Nothing, Nothing)` above is an error. Alternatives retain the existing
256-combination expansion limit and decision-tree work limit. Matching owns and
moves each bound affine component once and destroys ignored components once.
Matching a reference instead binds component references, preserving disjoint
component loans.
When the matched value is a tuple literal, such as `match (left, right):`,
the match holds each component in its own place and never builds the tuple:
components are evaluated left to right and released in the same order as when
matching a built tuple.

## Class patterns

A class pattern takes a class value apart, written as the class's generated
constructor takes its fields: positional patterns in field-declaration order,
then `field=pattern`. It matches a class position, the contents of a `Box`, or
that class's member of an anonymous sum.

```plenty fragment
class Point:
    x: i64
    y: i64

def axis(p: Point) -> Result[i64, OverflowError[i64]]:
    match p:
        case Point(0, y):
            Ok(y)
        case Point(x=x, y=0):
            Ok(x)
        case Point(x, y):
            Ok(x + y)
```

The rules have no exceptions:

- Every field appears exactly once; ignore one with `_` or `field=_`. A class
  with no fields is written `Name()`.
- Positional patterns precede named ones. An unknown, repeated, or missing
  field, or one positional pattern too many, is an error.
- The class is named as in an expression: by its name, a module path, or an
  import. As for enums, the matched value supplies type arguments, so
  `case Wrap(item)` matches a `Wrap[i64]` and `case Wrap[i64](item)` is an
  error.
- Each field must be [visible](../02-source/visibility.md) where the pattern
  is written, so a class with a private field cannot be taken apart outside
  its module.
- A class pattern tests nothing itself: it is irrefutable when every
  subpattern is, and a nested literal or variant makes it refutable.
- A bare name that spells the class at its position, such as
  `case Err(OverflowError):`, is rejected rather than bound, so the parentheses
  cannot be forgotten silently.
- Owned matching moves bound fields into their bindings and destroys the rest,
  as for an enum payload. A class with `__del__` expects every field
  initialized, so an owned match cannot bind its fields; match a reference such
  as `&value` to borrow them, or bind it whole. Borrowed matching binds field
  references, as for payloads.

A `@newtype` class or any one-field class is matched the same way; its pattern
costs no projection at run time. The error of a
[`checked:` block](../03-types/numbers.md#checked-blocks) is the motivating
case:

```plenty fragment
def total(a: i64, b: i64) -> i64:
    result = checked:
        a + b
    match result:
        case Ok(sum):
            sum
        case Err(OverflowError(wrapped)):
            wrapped
```

```plenty fragment
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

```plenty fragment
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
as `missing Ok(Some(Shape.Dot)), Ok(Nothing)` or `missing Point(_, _)`. A case, or a single alternative,
that earlier patterns already cover is an error. Incorrect payload arity and
wrong enum identities are errors. A pattern expands to at most 256 combinations
of its alternatives.

Literals never cover an integer or `str` position: such a position also needs
`_` or a payload name, and a match on an integer or `str` ends with `case _:`
or `case _ as name:`. `True` and `False` together cover a `bool`, so a `_` after
both is unreachable. A
repeated literal is an unreachable case.

Matching preserves source-order behavior, including overlapping payload patterns.
Native dispatch uses a jump table for at least 256 distinct integer values or
enum tags forming one contiguous range, in any source order. Signed values and
all integer widths are supported; out-of-range values still take the wildcard
or defensive invalid-tag trap. Table storage grows with the case count.
Smaller or sparse sets use comparison chains; a `str` case compares lengths
and then bytes. These chains can be sensitive to which cases occur most often; the
[dispatch measurements](../../../development/profiling-match-results.md#dense-match-dispatch)
explain the conservative table threshold. A match on at least 16 distinct `str`
literals instead binary-searches them once, so its cost grows with the
logarithm of the case count and does not depend on arm order; see the
[string match measurements](../../../development/profiling-match-results.md#string-match-dispatch). Keep using `match` for clear
exhaustive branching; dispatch selection does not change its behavior.

Guards, float patterns, range patterns, and `pattern as name`
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

```plenty fragment
def area(shape: Shape) -> Result[i64, OverflowError[i64]]:
    side: i64 = match shape:
        case Shape.Dot:
            return Ok(0)
        case Shape.Circle(side):
            side
        case Shape.Rect(width, height):
            if width < height:
                width
            else:
                height
    Ok(side * side)
```

The statement ends with the last case; nothing follows the block on its line.
Patterns, exhaustiveness, and ownership of the scrutinee are those of any
other match.

In a binding or assignment, the final expression of the selected case is the
value. A final `if` or `match` supplies the value of its selected suite.

- Continuing cases must agree on one type. An annotation, or the type of the
  assigned local, is the expected type of every case's final expression, as it
  is for an ordinary right-hand side. Without one, the first continuing case is
  inferred alone and its type is the expected type of the cases after it, as
  for the second value of an inline conditional. A later case can therefore be
  `Nothing` or `[]?`; a first continuing case that is one needs the annotation,
  or a typed case before it. Expected types never convert a value, so a
  `[text]?` of `&str` still differs from a `list[str]`.
- A case that ends in `return`, `break`, `continue`, or a propagating `?` gives
  no value. At least one case must continue; a match whose cases all exit is
  rejected with advice to write it as a statement.
- A case whose final statement has no value, such as `pass` or an assignment,
  gives unit `()`. When every continuing case does, the match binds unit, like
  any [unit value](../03-types/index.md); otherwise the cases disagree on the
  value type.
- The value may be a reference, plain or inside an `Option` or `Result`, into
  an owner that outlives the match, such as a binding of a borrowed scrutinee.
  While it is used, every case's owner stays borrowed; see
  [choosing with `if` or `match`](../05-ownership/05-optional-references.md#choosing-with-if-or-match).
  It cannot be a view or a closure with borrowed captures. Produce an owned
  value, or declare `mut name` and assign in each case.
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
call there is a [source-tail call](../06-functions/06-tail-calls.md), and a
statement after the block is unreachable. A case that ends without a value
returns unit, which is an error unless the function returns unit. A returned
reference follows the rules of any other [returned
reference](../05-ownership/04-returned-references.md).

Augmented assignment, assignment to several targets, call arguments, operands,
and every other expression position reject a `match` block; bind its value to a
name first. A [block `if`](branches.md#an-if-as-a-statements-value) supports the
same whole-statement value positions and retains these expression-position
restrictions.

## Compared to Python and Rust

Plenty's `match`/`case` syntax looks like Python's, but its bindings have
arm-local lexical scope. A pattern introduces fresh immutable locals, which may
shadow outer names; it does not assign to those outer bindings.

In Python, a [capture pattern](https://docs.python.org/3/reference/compound_stmts.html#capture-patterns)
binds in the enclosing scope, like an assignment. A successful pattern binds
its names **before** its guard is evaluated. For example, this Python program
prints `1 2 3`, even though the guard is false:

```python
a, b, c = 5, 6, 7
match (1, 2, 3):
    case (a, b, c) if a > 10:
        print("large")
    case _:
        print(a, b, c)
```

Those captures replace the enclosing `a`, `b`, and `c`; Python's successful
pattern bindings also outlive the `match`. This is different from a pattern
that fails partway through: Python deliberately leaves the persistence of
partial bindings [unspecified](https://docs.python.org/3/reference/compound_stmts.html#overview).

In Plenty, the same names in a selected arm shadow rather than replace the
outer bindings:

```plenty
def main() -> Result[(), IoError]:
    a, b, c = 5, 6, 7
    match (1, 2, 3):
        case (a, b, c):
            print("inner", a, b, c)?
    print("outer", a, b, c)?
    Ok(())
```
```output
inner 1 2 3
outer 5 6 7
```

Calling this binding behavior “hygiene” is informal: hygiene normally describes
name resolution in [macros](https://doc.rust-lang.org/reference/macros-by-example.html#hygiene).
Here the rule is ordinary lexical scope and shadowing, not rollback of arbitrary
side effects.

**Plenty has no case guards.** Both Python and
[Rust](https://doc.rust-lang.org/reference/expressions/match-expr.html#match-guards)
support guards that can reject a matching pattern and continue trying cases.
An `if` inside a Plenty arm runs only after that arm has been selected; a false
condition does **not** resume matching. Handle both outcomes inside the arm, or
use an `if`/`elif` chain when selection depends on such conditions.

Plenty is closer to Rust in these respects:

- Pattern bindings are [local to the arm](https://doc.rust-lang.org/reference/expressions/match-expr.html#r-expr.match.pattern-vars),
  rather than Python-style assignments to enclosing names.
- Matches must be [exhaustive](https://doc.rust-lang.org/book/ch06-02-match.html#matches-are-exhaustive).
  Python permits no case to match, in which case execution simply continues.
- Matching respects ownership: bindings can move owned noncopyable values or
  borrow payloads when matching references. See [Ownership](#ownership) and
  [Rust's binding modes](https://doc.rust-lang.org/reference/patterns.html#binding-modes).

There are also important differences from Rust. Rust's `match` is a general
expression; Plenty permits value-producing matches only at function tails and
in the [whole-statement value positions](#a-match-as-a-statements-value).
Both Rust and Python allow a bare capture name at the root of a pattern
(in Rust, a name resolving to a constant or variant is instead a path pattern).
Plenty deliberately requires `_ as name` for an untyped whole-value binding;
a bare name binds only within a payload or field pattern.

## Ownership

Owned matching consumes the scrutinee: bound payloads move into their bindings
and every part no pattern binds is destroyed. `_ as name` moves the whole value
into `name`, with no copy or allocation. Testing a literal copies the
integer, `str`, or `bool` it compares, so a later pattern can still bind or
move the value; for a `str` the copy shares the text and allocates nothing. Matching a shared or exclusive
reference binds shared or exclusive payload references instead; see
[borrowed matching](../05-ownership/index.md). Unit payload bindings produce unit
expressions and can be returned from a unit-returning function.
