# Loops and control transfers

`while condition:` repeats its suite while the Boolean condition is true.
`for name in source:` evaluates its source and visits the supported sequence.
Flat tuple unpacking is also accepted in the loop binding. Source kinds,
iteration loans, generators, and comprehensions are described under
[iteration](../09-iteration/index.md).

`break` exits the nearest loop. `continue` advances to its next iteration.
Both must appear within a loop. Statements following either transfer are
unreachable errors. Loops are unit-valued; loop `else` suites are unsupported.

Declarations inside a loop body do not establish values outside it. Assignments
to existing mutable locals persist. A loop may run zero times and cannot alone
prove that an initializer sets every field.

## Loops that never finish

A `while` loop diverges when both hold:

- its condition is the literal `True`, optionally parenthesized; and
- its body has no `break` that leaves it. A `break` inside an `if`, `match`,
  `with`, or `checked:` or `unchecked:` suite in the body leaves this loop. A
  `break` inside a nested `while` or `for` leaves only the nested loop.

No other condition counts, including a `const` that is `True` or an expression
such as `not False`. A diverging loop is a control-flow exit like `return`: it
can end a function or suite that must produce a value, and a statement after it
is an unreachable error. Its body leaves only by `return`, `?`, or a trap. In a
generator, `yield` suspends the loop as usual, and a suspended generator can be
dropped without resuming it.
Type checking, ownership analysis, and code generation apply the same rule, so
no path, drop, or result is assumed after the loop.

```plenty
def first_square_over(limit: i64) -> Result[i64, OverflowError[i64]]:
    mut n = 1
    while True:
        if n * n > limit:
            return Ok(n)
        n = n + 1

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(first_square_over(50)?)?
    Ok(())
```

```output
8
```

Control transfers destroy values whose ownership ends on that path. An explicit
return exits the enclosing function, while `?` follows the enclosing callable's
[error propagation](../08-errors/index.md) rule.
