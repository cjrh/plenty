# Standard sums and propagation

`Option[T].unwrap_or(default: T) -> T` selects a present payload or the default.
`Option[T].ok_or(error: E) -> Result[T, E]` converts `Some(value)` to `Ok(value)`
and `Nothing` to `Err(error)`. Both consume owned receivers, evaluate receiver
then argument exactly once, and drop the unused argument. They add no
allocation and support affine payloads and arguments; their construction
expressions retain their own allocation contracts. Unit payloads and unit
errors are supported. Bare absence needs enough type context to determine `T`.
Conversion is explicit: use `option.ok_or(error)?` in a Result-returning
function; `?` alone does not invent an error for absence.

## Defaults and recovery

`Result[T, E].unwrap_or(default: T) -> T` returns the `Ok` payload or the
supplied default, discarding the error. Like the Option form, it evaluates the
receiver and default eagerly, in that order. In
`option.unwrap_or(compute()?)`, `compute` runs and can propagate an error even
when the option is `Some`. Choose an eager default only when that is intended.
For a borrowed payload, as in `Option[&Item]`, the default is a borrow too, and
the result keeps both owners borrowed; see
[optional references](../05-ownership/05-optional-references.md#using-the-result).

The lazy alternatives consume an owned callback and invoke it at most once:

| Method | Callback | When called | Result |
|---|---|---|---|
| `Option[T].unwrap_or_else(f)` | `() -> T` | `Nothing` | `T` |
| `Result[T, E].unwrap_or_else(f)` | `E -> T` | `Err(error)`, passing the error | `T` |
| `Option[T].ok_or_else(f)` | `() -> E` | `Nothing` | `Result[T, E]` |

Named functions, reusable closures, and owned one-shot closures use
[`OnceCallable`](../06-functions/04-consuming-and-stored-closures.md).
Constructing a callback and evaluating its captures still happens eagerly;
only its body is conditional. Unused callbacks and their captures are dropped.
These operations add no allocation, deep copying, or implicit error propagation.
The existing restrictions on passing borrowed environments by value apply.
Use `match` for callbacks that cannot be passed as owners. With a borrowed
payload, as in `Option[&Item]`, the callbacks take the payload reference and
the method lowers as the `match` it abbreviates; see
[optional references](../05-ownership/05-optional-references.md#using-the-result).

Both sums also support `map_or(default, mapper)` and
`map_or_else(fallback, mapper)`. The mapper consumes `T` and returns `U`.
`map_or` evaluates its default `U` eagerly. `map_or_else` invokes only the
selected callback: Option's fallback takes no arguments, Result's fallback
consumes `E`, and both return the same `U` as the mapper. Receiver and callback
expressions evaluate from left to right, once. No automatic flattening occurs.

For a fallible default, choose `U = Result[T, E]`, wrap a present value with
`Ok`, and propagate **after** selection:

```plenty
def default_number() -> Result[i64, ParseError]:
    i64.parse("23")

def choose(value: Option[i64]) -> Result[i64, ParseError]:
    Ok(value.map_or_else(default_number, Ok)?)

def main() -> Result[(), ParseError | IoError]:
    print(choose(Some(7))?, choose(Nothing)?)?
    Ok(())
```
```output
7 23
```

These fallback semantics follow Rust's
[Option](https://doc.rust-lang.org/std/option/enum.Option.html#method.map_or_else)
and [Result](https://doc.rust-lang.org/std/result/enum.Result.html#method.unwrap_or_else)
operations, with Plenty's explicit allocation and callback contracts.

## Deliberate assertions

`Option[T].expect(message: str) -> T` and
`Result[T, E].expect(message: str) -> T` return the success payload or trap with
the supplied message and the call site's source location. The message is
evaluated eagerly, including on success. Reporting adds no allocation;
constructing a dynamic message retains its ordinary allocation contract.
The trap reports the message, without formatting the error payload, and uses
Plenty's existing trap behavior rather than Rust panic unwinding. Prefer `?`
and explicit recovery for expected failures.

## Inspection and propagation

`Result[T, E].is_ok() -> bool` and `is_err()` report which variant a result
holds; `Option[T].is_some() -> bool` and `is_none()` do the same for an option.
They observe the receiver, so a named value remains usable afterwards and a
temporary is dropped at the end of the full expression. They take no arguments,
ignore the payload, and allocate nothing. Asking a `Result` for `is_some` or an
`Option` for `is_ok` is a compile-time error that names the available pair.
Classes may define methods with the same names.

`Option[T]` has `Some(value)` and `Nothing`; `Result[T, E]` has `Ok(value)`
and `Err(error)`. These prelude variants are ordinary typed values. `Some`
can infer its type from its payload; `Ok`, `Err`, and `Nothing` need expected
context for their missing type arguments. Qualified constructors and aliases
are supported. Payload storage follows [enum rules](../03-types/enums.md).

Constructing, passing, returning, matching, and propagating standard sum
wrappers allocates nothing. Payload operations retain their own costs and
ownership rules. Wrapping a generator or closure does not make it copyable.
References cannot be sum payloads. Nested sums preserve all scalar values;
there is no source-visible null or numeric sentinel reserved for absence.

Postfix `value?` evaluates once. `Some(value)` or `Ok(value)` produces its
payload. `Nothing` returns `Nothing` from the enclosing function;
`Err(error)` returns `Err(error)`. The function must return the same sum
family; Result errors must be identical or widen losslessly into a declared
[anonymous sum](../03-types/anonymous-sums.md), except for explicit `Failure`
erasure below. Success types may differ. There are
no implicit Result/Option conversions. A unit success produces unit.

For explicit error conversion, [`result.map_error(mapper)`](../../stdlib/error.md#explicit-error-mapping)
consumes `Result[T, E]` and returns `Result[T, G]`, invoking the mapper only
for `Err`. It forwards `Ok` without copying and adds no allocation. A payload
variant is a mapper, as in `print(line).map_error(AppError.Io)?`. The
`std.error.map_error(result, mapper)` function has the same behavior. Neither
changes the propagation rule.

```plenty
def first(values: &list[i64]) -> Option[i64]:
    Some(values.get(0)?)

def checked_first(values: &list[i64]) -> Result[Option[i64], OverflowError[i64]]:
    match first(values):
        case Some(value):
            Ok(Some(value + 1))
        case Nothing:
            Ok(Nothing)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    values = [3]?
    print(checked_first(&values)?)?
    Ok(())
```
```output
Option[i64].Some(4)
```

Here `first` is a focused demonstration of Option propagation; an ordinary
first-element lookup can simply return `values.get(0)`. Absence and arithmetic
failure are distinct: `checked_first` returns `Ok(Nothing)` for an empty list
and propagates overflow as `Err`, never as absence. `?` does not implicitly
convert Option to Result.

Propagation may appear in arguments, conditions, loops, and comprehensions.
It composes with other postfix operations: `values?[0]` indexes the unwrapped
value. Failure releases already evaluated pending operands and initialized
locals, including hidden iterators/builders and entered resource scopes, in
ordinary cleanup order. Later operands and statements do not run. Error
payload ownership transfers to the caller. Generator bodies reject `?` because
their declared return family is `Generator[T]`; explicitly match instead.

Prefer a concrete error or a declared sum when combining fallible operations,
so callers can inspect or report the cause. Builtin `Failure` is the last
resort, for when no precise error can be written. It is a copyable
allocation-free builtin enum containing only `Failure.Unspecified`. In a function returning `Result[T, Failure]`, `?`
accepts any Result error type. Failure consumes and drops the original error
payload, then returns `Err(Failure.Unspecified)` after normal cleanup. Success
keeps its original payload. Erasure keeps no details, allocates no wrapper,
does not catch traps, and does not convert `Nothing`. User cleanup retains
its own effects and allocations.

Erasure applies only at `?`. Assignments, arguments, direct returns, and
explicit `Err(payload)` still require the declared error type or lossless
anonymous-sum membership/widening. Use
`Err(Failure.Unspecified)` to construct that error and `Ok(())` for unit
success. An allocating expression propagated this way still has its own
`AllocError` type before propagation.

An expression statement cannot implicitly discard a `Result`, including in
non-final branches, loops, context blocks, or generator bodies. Handle it,
return it, or use explicit `drop(result)`. Unused bindings, `Option`, and
aggregates containing Results are not rejected by this rule. Builtin `print`
also rejects a direct or borrowed Result; wrapper inspection uses
`str.repr(result)` with its formatting Result handled separately.
