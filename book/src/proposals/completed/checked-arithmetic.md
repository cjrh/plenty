# Checked arithmetic blocks

> **Status: Implemented** (issue #557). The
> [numbers reference](../../design/language/03-types/numbers.md#checked-blocks)
> defines current behavior; this page records the decisions and rationale.

## Problem

Integer overflow and division by zero trap. That is the right default when
they mean a bug, but code that takes sizes, counts or prices from input needs
to report them instead. Before this change the only recoverable options were
`try_from` after widening, or `std.math` helpers, which do not compose across
an expression.

## Rejected: a `Result` from every operator

Local PR #678 made every integer `+ - * // %` return `Result` and changed
existing code to `.unwrap()` or `?` each one. It was abandoned unmerged:

- ordinary math became noisy, and most call sites only unwrapped;
- every operation carried a payload and a branch on the result, which was
  slower than the existing inline trap check;
- the tutorial had to teach `Result` before the first addition.

Its error types, float `div_checked`, `T.MIN % -1 == 0`, the overflow code
generation and constant-divisor branch elimination were kept.

## Decision: an expression block

```text
r = checked:
    a * b + c
```

`checked:` opens an indented suite whose last expression is the value. The
block's type is `Result[T, E]`, where `E` is the
[anonymous sum](../../design/language/03-types/anonymous-sums.md) of
`OverflowError[T]` per operand width and `ZeroDivisionError` that its
operators can produce. The first failure exits the block with `Err`.

Outside any block, arithmetic compiles exactly as before, so the cost is paid
only where it is asked for.

### Lexical scope

Only arithmetic written in the block's suite is checked. Calls, closures
(including closures defined in the block) and other functions keep their own
behavior. A dynamic rule, where a callee's arithmetic depends on its caller,
would need either two compilations of every function or a run-time mode flag
on every operation, and would make a function's failure behavior depend on
code it cannot see. Comprehensions and compound assignment are lowered inline,
so they are checked when written in the block.

### Control flow

`?`, `return`, `break` and `continue` keep their meaning: the block catches
only arithmetic failures. A failure exits like `?`: it releases the locals the
block declared and the temporaries on the operand stack, then produces `Err`.
There is no rollback. An outer variable assigned before the failure keeps the
new value, and a compound assignment whose operation failed does not write.
Rollback would need copies of every mutated value and would hide allocation.

### Settled questions

| Question | Decision | Reason |
| --- | --- | --- |
| Where a block may appear | Binding or assignment value, after `return`, or a function's or case's final expression | The positions where a `match` block produces a value; no new expression grammar |
| `checked` as a keyword | Contextual: `checked` followed by `:` and a newline | Existing names such as `checked: i64 = 0` keep working |
| Nesting | The innermost block catches | Each block's error type stays local and predictable |
| A block with no failing operation | Compile-time error | `Result` with an empty error set would be misleading, and the block is likely misplaced |
| Every path exits | Compile-time error | The block has no value |
| Discarded block | Rejected as an unused `Result` | Same rule as any `Result` |
| Reference result | Rejected | Locals the block owns are released on failure |
| `x += 1`, `items[i] += 1` | Checked | Lexically arithmetic; the earlier rejection of compound assignment is not needed |
| `@newtype` operators (issue #555) | Forwarded integer operators are checked and report `OverflowError` of the field type | Forwarding is inline lowering of the field's operator; user-defined hooks are calls and keep trapping |
| Constant operands | Evaluated at run time inside a block; module constants still reject overflow at compile time | A block's purpose is to produce the `Err` value |
| Constant divisor | Error type unchanged; impossible branches omitted | Types follow operators, not values, so editing a constant does not change a signature |
| Shifts and casts | Unchanged | Out of scope; `try_from` covers casts |
| Negative `pow` exponent | Still traps | It has no integer result to wrap |
| Declared wider error | Kept when it accepts every possible failure | Lets a block be a function body returning a larger error set |

## Code generation

`Op::CheckedBlock` holds the body and the result type; each checked operator
is `Op::CheckedArith`, carrying the operation, operand type and cleanup for
the block's locals. Code generation computes the wrapped result and an
overflow flag with Cranelift's `*_overflow` instructions, tests the divisor
for zero and the signed `MIN // -1` case, and branches to a cold block that
builds the error, releases stack values above the block's base, runs the
cleanup and jumps to the block's exit. The success path wraps the value in
`Ok`. A flag known to be zero, such as the zero test for a nonzero constant
divisor, emits no branch.

Integer `pow` in a block calls `plenty_int_pow_checked`, which returns the
wrapped result and an overflow flag. Float `div_checked` is a one-operation
block.

Programs without `checked:` produce the same objects as before; the
comparison is recorded in the PR for issue #557.

## Follow-up

- Class-destructuring patterns for `OverflowError` (issue #558); use the
  `wrapped` field meanwhile.
- Checked shifts and casts, if a use case needs them.
