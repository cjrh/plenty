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
prove that a function returns or that an initializer sets every field.

Control transfers destroy values whose ownership ends on that path. An explicit
return exits the enclosing function, while `?` follows the enclosing callable's
[error propagation](../08-errors/index.md) rule.
