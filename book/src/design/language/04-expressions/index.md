# Expressions and evaluation

Arguments and ordinary binary operands evaluate once, from left to right.
`and` and `or` short-circuit; conditions and Boolean operators require `bool`
rather than truthiness. Conditional expressions evaluate only the selected
branch. [Assignment](assignment.md) evaluates its value before destination
indices.

Operator precedence, from highest to lowest:

| Group | Operators or forms |
| --- | --- |
| Postfix | Calls, member access, indexing, error propagation `?` |
| Unary | `+`, `-`, dereference `*`, borrowing `&` and `&mut` |
| Multiplication | `*`, `/`, `//`, `%` |
| Addition | `+`, `-` |
| Comparison and membership | `==`, `!=`, `<`, `>`, `<=`, `>=`, `in`, `not in` |
| Boolean negation | `not` |
| Boolean conjunction | `and` |
| Boolean disjunction | `or` |
| Conditional | `value_if_true if condition else value_if_false` |

Arithmetic operators associate left to right. Parentheses override precedence.
Chained comparisons are rejected. There is no exponentiation operator: `a ** b`
reads as `a * (*b)`, and when `b` cannot be dereferenced the diagnostic says
that `**` is unsupported and to multiply explicitly. [Numeric rules](../03-types/numbers.md) specify
type matching, casts, overflow, and float behavior. Membership follows the
container's [facilities contract](../../facilities/index.md).

Suites and conditionals have [value and exit rules](branches.md). Enums support
[exhaustive matching](matching.md); [loops](loops.md) provide repeated execution.
Postfix `?` follows [error propagation](../08-errors/index.md), and resource scopes
follow [cleanup](../05-ownership/index.md).
