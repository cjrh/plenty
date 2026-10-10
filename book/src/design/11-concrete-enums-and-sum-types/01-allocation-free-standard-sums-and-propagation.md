# Allocation-free standard sums and propagation

`Option` and `Result` use an inline 128-bit representation: one 64-bit terminal
payload and one 64-bit path of binary tags, outermost tag first. Nested standard
sums add tag bits without boxing; the existing 64-level type limit bounds the path.
The terminal payload is scalar bits, a handle to an independently owned heap
value, or an internal address of owner-local inline data: a range, a closure
environment, a class's fields, or a multi-field variant's fields. This
representation preserves all integer and float bit patterns; it does
not reserve a null pointer or numeric sentinel as a source-level value.

`LoadError` is also an allocation-free builtin. Its seven fieldless variants use
three terminal tag bits, preserved inside nested Option/Result values and normal
storage. Those bits count toward the tag-path limit. Loader failures can therefore
be constructed, matched, compared, returned, or propagated when allocation is
disabled; printing them retains the ordinary fallible formatting cost. See
[runtime loading](../24-c-interfaces/06-runtime-loading.md) for the variants.

Construction, passing, returning, matching, and `?` do not allocate a standard
sum wrapper. Copies of scalar-only sums and their equality comparisons are also
allocation-free. Payload operations retain their existing costs: creating a list,
concatenating strings, copying mutable contents, boxing a value, or formatting
output can allocate. Cleanup may execute user code that allocates.
This is a wrapper guarantee, not yet a guarantee of recoverable allocation failure.

Native calls carry standard sums as integer pairs through Cranelift's internal
calling convention. Addressable locals, collection entries, record fields, and
generator slots use 16-byte storage for scalar/pointer payloads and sum tags.
Slots whose types keep inline data add its bytes after the 16-byte word; only
those slots grow. Native functions returning such a value receive caller-owned
storage, and copies relocate its private address while preserving sum tags.
No owner is allocated. The
runtime aggregate helper takes pointers to aligned input/output slots rather than
depending on a platform's C ABI for `u128`. Neither representation is a public FFI ABI.

Postfix `value?` evaluates its operand exactly once. `Ok(value)` and `Some(value)`
produce the payload. `Err(error)` returns `Err(error)` from the enclosing function;
`Nothing` returns `Nothing`. The enclosing function must return the same sum
family, and `Result` error types must be identical after alias resolution unless
the enclosing function explicitly returns `Result[T, Failure]`. Success types can
differ. There are no other implicit error conversions or Result/Option
conversions. `?` on a unit success payload is a unit expression.

An expression statement cannot implicitly discard a `Result`; explicit
`drop(result)` is the opt-out. Builtin `print` also rejects a direct `Result`
argument, including a borrowed one. Handle the result before printing its payload.
For intentional wrapper inspection, format it with `str.repr(result)` and handle
that formatting Result too, for example `print(str.repr(result)?)?` in a function
returning `Result[(), Failure]`. `Option` and Results inside printable aggregates
remain printable. Diagnostics suggest `?` only when the enclosing function can
propagate that error and its success payload fits the use.

`Failure` is a builtin, allocation-free, copyable enum with one nullary variant,
`Failure.Unspecified`. It deliberately retains no error details. In a function
returning `Result[T, Failure]` (including aliases), `?` accepts any Result error
type. On failure it consumes and drops the original error payload, including any
custom destructor, then returns `Err(Failure.Unspecified)` after the normal
pending-operand, context-manager, and local cleanup. On success it extracts the
original success payload unchanged. Erasure and its Result wrapper do not
allocate; user-defined cleanup keeps its own behavior and allocation costs.
This rule applies in helpers as well as `main`; it requires no error protocol,
boxing, or dynamic dispatch. It does not catch traps or convert `Nothing`.

Erasure happens only at `?`: assignment, argument passing, direct returns, and
`Err(payload)` still require the declared error type. Explicit construction uses
`Err(Failure.Unspecified)`. Success remains explicit (`Ok(())` for unit success).
An allocating literal can use the expected success type through `?`, but its
own Result still carries `AllocError`. `main` keeps its existing exit-status
rules: `Ok(())` is zero, `Ok(i32)` supplies the status, and `Err` is one without
printing a diagnostic. Typed error unions and general conversions remain deferred.

Propagation is an expression and can appear in calls, conditions, loops, and
comprehensions. It binds with other postfix operations, so `values?[0]` indexes
the unwrapped value. An early exit drops already-evaluated pending operands and
initialized locals, including hidden iterators/builders, in the normal cleanup
order. Later operands and statements do not run. The error payload transfers
ownership to the caller. Generators reject `?` because their declared return type
is `Generator[T]`, not a propagatable sum; explicit matching remains available.

The frontend emits a checked propagation operation with concrete source and
return types. The independent checker verifies family/error compatibility and the
enclosing signature. Native lowering branches before evaluating later operations,
returns the residual after cleanup, and continues with the success payload.
