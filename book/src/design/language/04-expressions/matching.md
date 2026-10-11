# Pattern matching

`match` evaluates an enum scrutinee once and selects a `case`:

```python
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

A case names a variant and binds its fixed payload positions to immutable locals
or `_`. A whole-value `_` covers remaining variants. Arm bindings stay within the
arm and may shadow outer names. User-defined variants require qualification;
standard `Some`, `Nothing`, `Ok`, and `Err` patterns use the scrutinee's type.

Coverage must be exhaustive. Duplicate variants, redundant wildcards, incorrect
payload arity, and wrong enum identities are errors. Nested patterns, guards,
OR patterns, and scalar matching are unsupported. A literal pattern such as
`case "text":` or `case 1:`, or a scrutinee that is not an enum, is rejected
with advice to use an `if`/`elif` chain.

Payload bindings cannot be declared `mut`. To change an owned payload, move it
into a mutable local inside the arm (`mut local = payload`); to change it in
place, match `&mut value`.

Continuing arms must agree on their result type. Arms ending in `return`, `break`,
or `continue` contribute no join value. A function-tail match supplies the final
expression of its selected arm, just like a tail conditional.

Owned matching binds payload values. Matching a shared or exclusive reference
binds shared or exclusive payload references instead; see
[borrowed matching](../05-ownership/index.md). Unit payload bindings produce unit
expressions and can be returned from a unit-returning function.
