# Expressions and evaluation

Arguments and ordinary binary operands evaluate once, from left to right.
`and` and `or` short-circuit; conditions and Boolean operators require `bool`
rather than truthiness. Conditional expressions evaluate only the selected
branch. [Assignment](assignment.md) evaluates its value before destination
indices.

Operator precedence, from highest to lowest:

| Group | Operators or forms |
| --- | --- |
| Postfix | Calls, member access, indexing, [slicing](slices.md), error propagation `?` |
| Unary | `+`, `-`, bitwise not `~`, dereference `*`, borrowing `&` and `&mut` |
| Multiplication | `*`, `/`, `//`, `%` |
| Addition | `+`, `-` |
| Shift | `<<`, `>>` |
| Bitwise and | `&` |
| Bitwise exclusive or | `^` |
| Bitwise or | `\|` |
| Comparison and membership | `==`, `!=`, `<`, `>`, `<=`, `>=`, `in`, `not in` |
| Boolean negation | `not` |
| Boolean conjunction | `and` |
| Boolean disjunction | `or` |
| Conditional | `value_if_true if condition else value_if_false` |

Arithmetic operators associate left to right. Parentheses override precedence.
Chained comparisons are rejected. There is no exponentiation operator: two
adjacent stars, as in `a ** b`, are rejected with a diagnostic that names
[`base.pow(exponent)`](../../facilities/01-builtins/numbers.md#powers), whatever
the operand types. Multiplying by a dereference needs a space: `a * *b`. [Numeric rules](../03-types/numbers.md) specify
type matching, casts, overflow, and float behavior. Membership follows the
container's [facilities contract](../../facilities/index.md).

Ordering operators accept matching integers, floats, strings, and
[orderable tuples](../03-types/tuples.md). Tuple operators compare
lexicographically with each component's own operator, so float components keep
IEEE behavior: signed zeros are equal and a NaN component makes every ordered
relation false. `sort()` has a separate total order.

[Format strings](format-strings.md) build text from fields with static specs.
Suites and conditionals have [value and exit rules](branches.md). Enums support
[exhaustive matching](matching.md); [loops](loops.md) provide repeated execution.
Postfix `?` follows [error propagation](../08-errors/index.md), and resource scopes
follow [cleanup](../05-ownership/index.md).

## Nesting

Blocks and expressions nest to at most 64 levels in total. A function body is
level 1. A suite is one level below the statement that owns it, and so are that
statement's expressions. An operand, argument, index, or element is one level
below the expression that holds it, and parentheses add a level around what
they enclose.

Operators group to the left, so `a + b + c` is `(a + b) + c`: every binary
operator, call, member access, index, or `?` in a chain puts everything before
it one level deeper. A statement in a function body can chain 62 of them. Each
manager of a [`with`](../08-errors/03-resource-scopes.md) statement is one
level around the managers after it and the body, and each `for` or `if` clause
of a comprehension is one level around the clauses after it and the element.
The `elif` arms of an `if` are [not nested](branches.md).

Deeper source is rejected at the token that reaches level 65, with
`block and expression nesting exceeds the implementation limit of 64`. Bind an
intermediate value to a name, group a long chain with parentheses, or move
inner blocks into a function. [Types](../03-types/recursive-data.md) and
[patterns](matching.md) have their own limits of 64.
