# Checked arithmetic by default

> **Status: Not implemented; proposed for discussion.** Today arithmetic outside
> [`checked:`](../design/language/03-types/numbers.md#checked-blocks) traps.
> This proposal makes its recoverable arithmetic behavior the function default
> and introduces `unchecked:` as an explicit opt-out. Implementation and
> migration remain under **B14** in the [backlog](../backlog.md).
> All examples here describe proposed behavior.

## Recommendation

Make every ordinary function body a boundary for recoverable integer arithmetic
failures. Operators still produce ordinary numbers on success; their failure
returns `Err` from the function, whose declared `Result` error type must include
it. Callers handle that ordinary `Result` with `?`, `match`, or another existing
operation. There is no `?` after each arithmetic operator.

Keep `checked:` as an explicit, value-producing local recovery boundary, both
in ordinary code and inside `unchecked:`. Give `unchecked:` the value of its
suite, without adding a `Result` wrapper. Inside it, affected arithmetic failures
terminate the process using today's arithmetic traps.

This improves recoverability by default and makes deliberate arithmetic traps
searchable. It does **not** remove all machine checks: guaranteeing termination
on overflow requires detecting it. “Unchecked” means unchecked by the typed
arithmetic error mechanism, not undefined behavior or silent wrapping.

## Function contracts

```text
def scale(value: i64, factor: i64) -> Result[i64, OverflowError[i64]]:
    product = value * factor
    Ok(product)

def invoice(quantity: i64, price: i64) -> Result[i64, OverflowError[i64]]:
    Ok(scale(quantity, price)?)
```

The multiplication has type `i64`; failure returns
`Err(OverflowError[i64](wrapped=...))` from `scale`. A signature of `-> i64`
is rejected for this body. The compiler reports the failing operation and the
missing error type, with suggestions to declare a `Result`, handle a local
`checked:` result, or deliberately opt out.

The implicit function boundary controls failure routing, **not success-value
conversion**. Keep ordinary explicit `Ok(...)`, `Err(...)`, tail expressions,
and `return` rules. Do not wrap an existing `Result` into another `Result`,
auto-unwrap calls, infer public return types, or accept a bare `i64` as the
success return of `Result[i64, E]`. This is deliberately more precise than
textually wrapping an entire function in today's `checked:` expression.

Only failures that can escape this function boundary must appear in its error
type. A locally handled `checked:` error need not appear; an error propagated
with `?` does. Wider declared error unions remain permitted. Functions with no
escaping arithmetic failures can still return a plain type, including `()`.
An `Option`-returning function cannot silently turn overflow into `Nothing`:
handle the arithmetic locally, change the return contract, or opt out.

The initial scope is exactly today's checked integer operation set:

| Operation | Error contributed |
| --- | --- |
| Integer `+`, `-`, `*`, unary `-`, `pow` | `OverflowError[T]` |
| Signed `//` with a plain or `NonZero[T]` divisor | `OverflowError[T]` |
| `//` and `%` with a plain integer divisor | `ZeroDivisionError` |

Unsigned division has no overflow case. `NonZero[T]` removes the zero error;
`Positive[T]` also removes signed division overflow. Compound assignment and
forwarded integer `@newtype` operations follow the same rules and field error
types as today. Type checking follows operators and operand types, not general
range analysis: even `n // 4` keeps today's conservative error set. Optimizers
may remove proven-unnecessary branches without changing that contract.

Float arithmetic, float `div_checked`, shifts, casts, negative integer `pow`
exponents, indexing, allocation, I/O, and explicit `unwrap()` retain their
existing contracts. Module constants still reject overflow at compile time.
This is a proposal for recoverable integer arithmetic by default, not a promise
that all runtime failures become recoverable.

## The opt-out block

```text
def scale_or_terminate(value: i64, factor: i64) -> i64:
    unchecked:
        value * factor
```

On success this returns an `i64`. Overflow terminates the process. It cannot
reach an outer checked boundary as an `Err`; there is no unwinding or rollback.
Required guards remain in debug and optimized builds alike. No build flag may
silently change this contract to wrapping or undefined behavior.

`unchecked` is contextual, like `checked`: only `unchecked:` followed by a
newline opens a suite; an existing typed binding `unchecked: i64 = 0` remains
valid. Support value positions already available to `checked:`: a binding,
assignment, return value, function tail, or match-arm tail. Also support a
standalone suite for mutations and control flow. Locals remain block scoped.

A value-producing suite yields its final expression's type `T`, not
`Result[T, E]`. If that expression already has type `Result`, it stays a
`Result` and must be handled normally. A statement suite follows ordinary
statement/discard rules; it cannot silently discard an owned value or a
`Result`. An empty suite with `pass`, or an all-exiting suite, is allowed in
statement position. No reference-valued block extension is proposed initially.

`?`, `return`, `break`, and `continue` retain their ordinary function/loop
targets. In particular, `unchecked:` does not catch or trap a callee's `Err`,
disable allocation handling, or make an unchecked call variant:

```text
def caller(value: i64) -> Result[i64, OverflowError[i64]]:
    answer = unchecked:
        scale(value, 2)?
    Ok(answer)
```

`scale` still returns its declared error and `?` still propagates it. To get
trapping arithmetic in a helper, the helper's implementation must explicitly
contain the opt-out. Inlining cannot change that behavior.

## Keeping `checked:`

Retain its current `Result[T, E]` expression semantics and lexical callback
behavior. It remains useful for recovering or mapping errors before they reach
the function signature, even when the default is checked. Within `unchecked:`,
it also restores recoverability for a smaller region:

```text
def mixed(a: i64, b: i64) -> Result[i64, OverflowError[i64]]:
    result = unchecked:
        offset = a + 1
        sensitive = checked:
            offset * b
        sensitive
    result
```

The addition traps on overflow; multiplication produces the local `Err`, which
is returned normally. The outer unchecked suite passes through that `Result`
without trapping or wrapping it. A `match` on `sensitive` could instead recover
locally and let `mixed` return a plain `i64`.

| Lexical context of an integer operation | Failure destination |
| --- | --- |
| Function body, without a nearer explicit mode | Function's declared `Result` |
| Explicit `checked:` | That block's `Result` |
| Explicit `unchecked:` | Process termination |
| `checked:` inside `unchecked:` | Inner block's `Result` |
| `unchecked:` inside `checked:` | Process termination |

Nested `unchecked:` is redundant but legal. Leaving any suite restores its
outer mode. A failure in an explicit checked block releases that block's locals;
an implicit function failure releases function-owned values as `?` does.
Earlier mutations are retained, and a failing compound assignment performs no
write. Neither boundary is transactional.

Keep today's explicit `checked:` restrictions initially: unused results,
reference results, blocks with no failing arithmetic, and blocks whose every
path exits are rejected. Do not apply the last two restrictions to the implicit
function boundary: ordinary infallible and all-returning functions remain valid.

## Auditability and cost

Search for `unchecked:` to locate explicit arithmetic opt-outs, then review the
reason each region accepts process termination and whether its extent is minimal.
Use a source-aware search to distinguish syntax from comments and text. Prefer
comments explaining bounds or measured hot paths over generic “faster” claims.
Real-world confidence in bounds is useful evidence, but a check still traps if
that confidence proves wrong.

This search is not a complete trap audit: indexing, `unwrap()`, negative powers,
foreign code and existing runtime primitives remain separate trap sources. Std
migration must expose deliberate arithmetic opt-outs in source or documented
native contracts; hiding unchanged trapping reductions behind safe-looking calls
would weaken the proposal's benefit.

Default code can reuse today's straight-line arithmetic plus cold error exits.
It need not materialize a `Result` for each operation. `unchecked:` can avoid
error payload construction, recoverable cleanup paths and some return ABI costs;
it cannot promise check-free arithmetic or a speedup. Retaining `checked:` does
not itself imply runtime mode switches: the mode is lexical.

See [implementation and migration](checked-by-default/implementation.md) for
callback boundaries, compatibility risks and the evidence needed before adoption.
