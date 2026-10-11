# Signatures, methods, and calls

Named functions are declared at module scope or as class methods. Every
parameter and result has an explicit type, except the class receiver shorthand
`self`. All signatures are available before bodies are checked, permitting
forward calls and mutual recursion. Nested named functions, default or keyword
arguments, overloads, and redefinitions are unsupported.

```plenty
def clamp_low(value: i64, minimum: i64) -> i64:
    if value < minimum:
        return minimum
    value

def main() -> Result[(), Failure]:
    print(clamp_low(2, 5))?
    Ok(())
```

The final expression supplies the function's result. `return value` exits from
any suite; bare `return` supplies unit. Every return must match the declared
type, and every continuing path must produce that result. The
[branch and return rules](../04-expressions/branches.md) define how continuing
paths join and unreachable statements are rejected.

A string literal that begins a body is the function's documentation, not its
final expression. A `str` function whose body is only that string is rejected
with this explanation; write `return "text"` to return the string.

Calls evaluate the callee once, then arguments once from left to right.
Parameter types supply expected types, including numeric literal context.
Owned arguments copy or move; reference arguments follow
[borrowing](../05-ownership/02-borrowing.md). A later argument's `?` cleans up
already evaluated arguments without entering the callee. Arguments are
positional: `name=value` in a call is rejected as a keyword argument.

A method's first parameter is `self`. Bare `self` means `&Class` in ordinary
methods and `&mut Class` in `__init__` and `__del__`. Mutating ordinary methods
declare `self: &mut Class`; their receiver must be a mutable owner or exclusive
reference. Methods resolve statically and borrow their receiver. Temporary
owned receivers are supported when no reference escapes. Attempting mutation
through an ordinary method's bare `self` reports the explicit
`self: &mut Class` annotation needed for an exclusive receiver. For a generic
class, use its declared type parameters in that annotation, such as
`self: &mut Cell[T]`; the diagnostic points to these parameters rather than a
particular specialization. See
[classes](../03-types/classes.md) for initialization and field rules and
[returned references](../05-ownership/04-returned-references.md) for receiver
lifetime restrictions.
