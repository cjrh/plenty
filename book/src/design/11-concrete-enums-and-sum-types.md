# Concrete enums and sum types

Constructing an enum value never allocates. `Enum.Variant(payloads)` and its
`Enum.Variant.new(payloads)` alias return `Enum` directly; nullary variants use
`.new()` with no arguments. Payloads evaluate first and move into the value.
Allocations inside payload expressions keep their own `Result`.

```python
enum Reading:
    Missing
    Value(i64)
    Invalid(str)

def describe(reading: Reading) -> str:
    match reading:
        case Reading.Missing:
            "missing"
        case Reading.Value(number):
            "positive" if number > 0 else "nonpositive"
        case Reading.Invalid(reason):
            reason
```

Enums are nominal module-level types. Variants have zero or more fixed positional
payloads; nullary variants omit parentheses. Qualified constructors and patterns
use an enum name, a transparent alias, or an explicit builtin instantiation such
as `Option[i64]`. Type/alias declarations may refer forward;
[recursive enums](30-recursive-data.md) reach themselves through `Box` or a
collection. Enum names
share the type declaration namespace. A binding shadowing a type qualifier is
diagnosed rather than silently selecting different behavior.

Type nesting is limited to 64 levels, expanded builtin type argument names
to 16,384 bytes, and a type's inline storage to 64 KiB, with diagnostics when
these implementation limits are exceeded. Compiler-emitted metadata links shared
type nodes, and layouts are computed once per type, so shared enum dependencies
do not expand compile-time work exponentially. There is no runtime metadata
parsing or allocation.

`Option[T]` and `Result[T, E]` are compiler-known concrete enum constructors,
without user generics or traits. Payloads may be integers, floats, bool, str,
collections, classes, other enums, generators, or unit. References
cannot be payloads. Enums without generator payloads can be list elements and dictionary values, but are
not dictionary keys or set elements in the initial closed hashable-type set.

`Some`, `Nothing`, `Ok`, and `Err` are compiler-known prelude names. Constructors
use the expected type from an annotation, argument, return, or enclosing typed
constructor/collection. `Some(value)` can infer its complete type from its payload;
`Ok`, `Err`, and `Nothing` require context because their missing type arguments
cannot be guessed. Inference is local and left-to-right, not a search across
later statements or other functions. Qualified forms and aliases remain supported.
Prelude names follow other built-ins: top-level function/type redefinitions are
rejected; local bindings can shadow expression names. Unqualified patterns always
denote the standard variants and get their type from the scrutinee. User-defined
variants still require qualification.

Unit payloads evaluate their argument for effects, then store a zero marker in
the ordinary runtime field slot. Matching may ignore or bind that payload; reading
a unit pattern binding yields the no-register unit expression, so it can be
returned from a unit-returning function. It does not introduce nullability.

`match` accepts enums and [enum references](31-borrowed-enum-matching.md).
Owned matching binds values; shared/exclusive matching binds shared/exclusive
payload references. Each `case` names a variant and binds payload
positions to immutable locals or `_`; a whole-value `_` covers the remaining
variants. Coverage is exhaustive and checked before lowering. Duplicate variants,
redundant wildcards, incorrect payload arity, and wrong enum identities are
errors. Nested patterns, guards, OR patterns, and scalar matching syntax are
deferred. The scrutinee is evaluated once. Arm bindings are scoped locally and
may shadow outer bindings. Continuing arms agree on result type; arms ending in
return/break/continue do not contribute a join value. A function-tail match
produces its final arm expression, like the existing statement-form `if`.

Every enum value uses the inline representation of `Option` and `Result`: a
payload word and a path of binary tags. A variant with one field keeps that
field's value in the payload word. A variant with several fields keeps them in
typed slots in the owner's storage, after its 16-byte slot word, and the payload
word addresses them. Each slot uses 16 bytes plus the field's own inline bytes,
and the owner reserves room for the largest variant. A tuple is a single-variant
enum. Moving a value copies its bytes; copying a copyable value also retains its
strings. Owned payloads may be mutated through exclusive match loans.

Equality compares nominal type, tag, and payload contents, using IEEE
comparisons for floats. Runtime metadata records whether equality is reflexive;
float-containing values cannot use identity as an equality shortcut because of
NaN. Every value has one owner, so traversal is bounded by the value's size.
Automatic equality and formatting reject recursive data types.
Printing uses qualified variant names. No niche optimization,
stable external layout, or per-instantiation code generation is required.
Frontend coverage lowers to the existing scalar-tag match with an invalid-tag
trap fallback. The independent checker validates construction/projection types;
the structured frontend places projections behind the corresponding tag tests.
