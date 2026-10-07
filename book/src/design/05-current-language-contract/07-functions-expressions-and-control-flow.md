# Functions, expressions, and control flow

Named functions are declared at module scope or as class methods, with complete
signatures. Capture-free multiline anonymous function expressions can appear in
function bodies; see [function values](../25-function-values.md). There are no
nested named functions, captured environments, default/keyword arguments,
overloads, or redefinitions.
Signatures are collected before any body is checked, allowing forward calls
and mutual recursion. All declarations and statements are checked before any
native code is emitted or executed.

A suite's last expression is its value. Continuing branches of a value-producing
`if`/`elif`/`else` must agree. A non-final expression is evaluated and discarded;
a non-final conditional discards its branches' results. An `if` without an
`else` can only have unit result. The inline conditional uses Python order:
`value_if_true if condition else value_if_false`.

Conditions and Boolean operators accept only `bool`; there is no truthiness.
`and` and `or` short-circuit. Arguments and ordinary binary operands evaluate
left to right. Each expression is evaluated once.

`return value` exits the enclosing function from any suite; bare `return`
returns unit. Every explicit return is checked against the declared return
type, even inside a non-final or statically unchosen branch. A returning branch
does not participate in the type join of paths that continue. A function with
any continuing path must still produce its declared result on that path; an
`if` without `else` cannot prove that all paths return. This analysis is
structural, without constant-condition folding. Statements after an explicit
return or an exhaustive conditional whose branches all return are rejected
as unreachable at their source position.

```python
def clamp_low(value: i64, minimum: i64) -> i64:
    if value < minimum:
        return minimum
    value
```

`for` and `while` loops, including `break` and `continue`, are implemented.
Tail calls in final expressions, final branches, and explicit return
expressions (including early guard clauses) become tail-call operations.
Cranelift emits `return_call` or `return_call_indirect` with the Tail calling
convention. Functions with resource-bearing parameters or locals retain ordinary
calls so observable cleanup happens after the callee returns.
