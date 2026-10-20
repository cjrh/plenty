# Finite error sets as anonymous sums

**Status: Partly implemented.** General anonymous sums have shipped, as described in the
[anonymous-sum reference](../design/language/03-types/anonymous-sums.md).
It adopts `case cause: Type:` at the case position, grouped types and inline
normalization/injection/widening. Existing pattern guards remain unsupported.
[Human-readable reporting](completed/error-formatting.md) has also shipped.
Builtin `Failure` stays as a last resort, for when no precise error can be
written (issue #342). The current
[propagation reference](../design/language/08-errors/01-sums-and-propagation.md)
remains authoritative. Scheduling belongs in B14/B71 in
the [backlog](../backlog.md). Human-readable presentation is specified separately
in [formatting and error reports](completed/error-formatting.md). The
[composition overview](error-composition.md) separates this language feature
from explicit mapping and typed stream APIs.

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

## Agreed initial contract

The following notation describes the implemented composition contract:

```text
def load() -> Result[Config, IoError | ParseError]:
    source = read_source()?
    parse_source(source)
```

`A | B` is a general type expression, available in ordinary value positions
as well as Result errors. Error composition supplies the first acceptance cases;
the type system must not restrict sums to the second argument of Result.

1. Treat `A | B` as a tagged, owned sum of whole nominal types, not a C union
   and not an untagged overlap of values. No boxing is implicit. Storage must
   accommodate the largest payload and its tag; existing recursive-data rules
   still apply. Large payloads can enlarge every enclosing `Result`.
2. Normalize parentheses, order, duplicates, and resolved aliases: `A | B`
   and `B | A` have the same identity; `A | A` is `A`. Preserve distinct nominal
   wrappers. Publish no stable tag-number/FFI-layout guarantee by accident.
3. Permit lossless injection of `E` into a destination containing `E`, and
   widening of a source set into a superset. Do not invoke arbitrary user
   conversions. Apply these rules in expected-type checking for bindings,
   assignments, arguments, `Err`, direct returns, and `?`. Also permit
   `Result[T, E]` to widen to `Result[T, F]` when E is a member/subset of F:
   success moves unchanged and failure is retagged without allocation.
   This does not establish covariance for arbitrary generic containers or
   references, and does not change the success type.
4. Reject propagation when any source alternative is absent. Diagnostics name
   the missing alternatives and source operation. The final design removes
   builtin `Failure` and its special erasure rule; the existing rule remains
   only during migration.
5. Provide exhaustive, unambiguous matching by member type using
   `case cause: MemberType:`. Existing enum constructor patterns cannot identify
   every class payload; existing pattern `|` keeps its separate meaning.
6. Do not implicitly flatten named enum variants. `HttpError | AllocError`
   may still contain `HttpError.Allocation`; deduplicating that semantic overlap
   requires an explicit classifier. Otherwise API abstraction is broken.
7. A first version can match the entire set without automatic residual-type
   inference. Recovery that subtracts a member type is a further feature, not
   free syntax sugar. An uninhabited empty set and conversion to infallible `T`
   also need a decision; do not use inhabited unit as an empty error type.

Function return types remain declared. Do not infer a public error set from
the body or silently widen a declared binding after a later assignment.
Expected types guide injection; inference must not guess between distinct
generic substitutions that satisfy membership. Normalize again after concrete
generic substitution, so a generic set can collapse to a single member.

Injection and widening evaluate the source once and preserve the active payload.
Owned noncopyable payloads move; inactive alternatives are never initialized or
dropped. Early exits and destruction follow the existing sum ownership rules.
Copyability and thread-transfer eligibility require every member to qualify.
Existing restrictions on sum payloads and recursive storage continue to apply;
general sums do not independently introduce stored references.

## Matching and remaining surface decisions

Matching must select a whole member type and bind its payload, including class
payloads. The implemented spelling is `case cause: IoError:` at the case
position. It does not extend existing pattern `|`. An enum member can then
be matched by its constructors normally.

The initial contract requires exhaustive coverage of normalized members, or a
wildcard. Case guards remain unsupported; a future guarded arm alone cannot
establish coverage. Reject duplicate
unguarded member arms, including members that coincide after generic
substitution. Specify those diagnostics on concrete specializations rather than
pretending generic alternatives always remain distinct.

Owned matching follows ordinary move rules; matching through `&` or `&mut`
borrows the active payload with the corresponding permission and cannot let it
outlive the owner. A wildcard does not infer a smaller residual sum in this
version. Empty sums, automatic error-set subtraction, and automatic conversion
to infallible values are deferred.

The implementation puts `|` below references and type applications, uses `(T)`
for grouping and `(T,)` for singleton tuples, and allows typed-member binding
when normalization leaves an ordinary type. Callable brackets delimit their
argument/result types. Diagnostics cover these cases alongside generic inference
ambiguity; ambiguous constructors require an explicitly typed intermediate.

## Replacing Failure

Replace uses of builtin `Failure` with precise errors or finite sums, optionally
named by ordinary type aliases. Keep domain enums where they express policy;
`map_error` remains the explicit way to classify or wrap a cause. Do not replace
erasure with an implicit catch-all member or mandatory reporting bound.

Removal includes the prelude builtin, special checking/code generation, fixtures,
examples, reference and tutorial text, and agent-facing probe guidance. The
mutual-TLS lesson is an acceptance case: configuration and networking errors
propagate with `?`, retaining details; intentional matches still demonstrate
certificate rejection. Migrate only after sum propagation and reporting work.
The final change must reject reliance on the removed builtin rather than silently
reinterpreting old erasure. User-defined types named Failure are ordinary types.

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

Explicit mapping is the implemented baseline. Add ordinary non-error sum values
to the acceptance cases, plus Result widening on both success and failure,
generic normalization, borrowed/mutable matching, and entrypoint reporting of
each active member. Builtin Failure and its erasure stay as a last resort, for
when no precise error can be written (issue #342). Production typed-stream migration remains a
separate change: sums cannot restore causes already discarded by an adapter.
