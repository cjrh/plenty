# Finite error sets as anonymous sums

Status: exploratory alternative within [explicit error composition](error-composition.md),
not accepted syntax. Scheduling belongs in [B71](../backlog.md).

## Motivation and research

The proposed signature
`Result[(), IoError | AllocError | SomeError]` describes one error drawn from
a finite set. It does not mean simultaneous errors, a dynamically typed value,
or automatic recovery. This could remove wrapper-only enum declarations while
retaining payloads and compile-time coverage checks.

Dillon McMahon's article argues for composing precise error sets, widening
them when combining operations, and narrowing them after recovery. It also
separates recovery type information from operational context. Those are useful
design questions, not proof that one implementation is best for Plenty.
[The Missing Piece in Rust Error Handling](https://mcmah309.github.io/posts/the-missing-piece-in-rust-error-handling/)

The `eros` documentation describes typed and erased errors, widening and
recovery. Crucially, version 0.8.1 documents boxed storage for `ErrorUnion`.
Its ergonomics therefore do not demonstrate allocation-free inline error
composition. Plenty should evaluate its own representation rather than copy
that storage choice. [eros documentation](https://docs.rs/eros/latest/eros/)

The reported mixed Reddit reaction is not used as evidence here: no particular
thread was supplied or independently assessed. The following objections are
design analysis, not attributed community consensus.

## A possible deliberately small contract

All notation below is hypothetical:

```text
def load() -> Result[Config, IoError | ParseError]:
    source = read_source()?
    parse_source(source)
```

If this direction is accepted:

1. Treat `A | B` as a tagged, owned sum of whole nominal types, not a C union
   and not an untagged overlap of values. No boxing is implicit. Storage must
   accommodate the largest payload and its tag; existing recursive-data rules
   still apply. Large payloads can enlarge every enclosing `Result`.
2. Normalize parentheses, order, duplicates, and resolved aliases: `A | B`
   and `B | A` have the same identity; `A | A` is `A`. Preserve distinct nominal
   wrappers. Publish no stable tag-number/FFI-layout guarantee by accident.
3. Permit lossless injection of `E` into a destination containing `E`, and
   widening of a source set into a superset. Do not invoke arbitrary user
   conversions. Specify these rules for expected-type checking of `Err`, direct
   returns, and assignments as well as `?`, or surprising inconsistencies result.
4. Reject propagation when any source alternative is absent. Diagnostics name
   the missing alternatives and source operation. Keep `Failure` erasure explicit
   through its existing `?` rule.
5. Provide exhaustive, unambiguous matching by member type. Exact syntax needs
   a separate decision: existing enum constructor patterns cannot identify every
   possible class payload. Do not silently reinterpret existing pattern `|`.
6. Do not implicitly flatten named enum variants. `HttpError | AllocError`
   may still contain `HttpError.Allocation`; deduplicating that semantic overlap
   requires an explicit classifier. Otherwise API abstraction is broken.
7. A first version can match the entire set without automatic residual-type
   inference. Recovery that subtracts a member type is a further feature, not
   free syntax sugar. An uninhabited empty set and conversion to infallible `T`
   also need a decision; do not use inhabited unit as an empty error type.

Restrictions to error positions initially could limit inference scope, but
would introduce special-case type rules. General anonymous sums provide broader
consistency at greater compiler and teaching cost. Decide this before adopting
surface syntax.

## What sets do not solve

An I/O failure while reading and the same I/O type while writing become the
same set member. `std.io.CopyError.Read` versus `.Write` carries useful meaning
that a bare union discards. Use named operation wrappers or structured context.
Likewise, JSON offsets and partial byte counts must remain attached to causes.

A public set exposes its member types and changes when another alternative is
added. Exhaustive callers then need updating. Named domain enums can provide a
more stable abstraction boundary, though adding their variants also affects
exhaustive matching. An erased error hides alternatives but sacrifices static
coverage. Choose based on the caller's recovery contract.

Unions do not automatically fix `Reader` returning `StreamError`: the stream
interface must still become generic or adopt a different envelope. Nor do they
replace allocation-aware context construction in `std.error.Context[E]`.

## Concrete evaluation targets

Try sets on internal composition near `http/body.plenty`'s `read_json` and
`send_json`, and the HTTP client's `framed`/`netted` boundaries. Compare with
explicit mapping into the current named types. Preserve `BodyError.status`
classification, allocation normalization, JSON locations, and retry policy.

Do not start by replacing `CopyError`, `Partial`, `JsonError`, or every public
HTTP error. They carry semantics beyond membership. Require comparisons of
signature readability, diagnostics, representation size, compile time, and
client recovery code. Tests must cover aliases, duplicate types, permutation,
nested sets, ambiguous generic members, missing-member diagnostics, payload
destruction, exhaustive matching, and no implicit allocation.

Recommendation: keep this as an independently evaluated option. Explicit
mapping has a smaller semantic footprint and should provide the first baseline.
