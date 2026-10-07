# Sum types and enums

Status: implementation proposal, not a statement of implemented syntax. This
proposal was prepared against the AOT compiler after loop-control support.

## Contract

Add nominal concrete enums, compiler-known `Option[T]` and `Result[T, E]`, and
exhaustive `match` suites. Preserve one native value per Plenty value and existing
independent-value semantics. This does not require structs, user generics,
traits, dynamic dispatch, a new optimizer, or a borrow checker.

```python
enum Reading:
    Missing
    Value(i64)
    Invalid(str)

def describe(reading: Reading) -> str:
    match reading:
        case Reading.Missing:
            "missing"
        case Reading.Value(value):
            if value < 0:
                "negative"
            else:
                "available"
        case Reading.Invalid(reason):
            reason

reading = Reading.Value(42)
print(describe(reading))
```

An enum declaration is module-scoped and contains one or more variants. Variants
have either no payload or a fixed sequence of statically typed positional
fields. Construction always names the enum and variant. Nullary variants are
values (`Reading.Missing`); reject `Reading.Missing()` with a useful diagnostic.
Payload variants require exactly the declared argument count and types. Arguments
evaluate left to right. No implicit integer widening is introduced.

Reserve enum/type names in the same declaration namespace as aliases and built-in
type names. Variant names need only be unique within their enum; they do not
introduce globals. A binding must not silently change a qualified type constructor
into a method call: resolving a type-qualified expression uses the type namespace
and rejects ambiguous shadowing, consistently with existing type-constructor
rules. Enums do not have methods in this slice.

## Type identity and resolution

Two enum declarations with identical variants remain different types. A type alias
is transparent: `type Input = Reading` names the same enum and may qualify its
constructors and patterns. Forward references to aliases and nonrecursive enum
types are accepted, following the existing module-wide alias behavior.

Collect declaration names before resolving any field types. Resolve a dependency
graph containing aliases and enum fields, with explicit visiting/resolved states.
Reject recursive enum declarations for the first slice, including recursion through
collections. Give the dependency path in the error. This intentionally avoids
accidentally promising recursive destruction, cyclic metadata, or mutable cyclic
values; recursive sum types can follow with separately bounded implementation.

`Ty::Enum(Rc<EnumDef>)` is a small integration option for the current compiler.
`EnumDef` contains a compilation-local nominal ID, display name, and resolved
variants with field types. Its equality and hash use the ID, not structural field
equality. Built-in instantiations are interned by constructor and concrete type
arguments, so independently resolving `Option[i64]` produces the same identity.
The resolver owns the intern table; code generation only consumes immutable
definitions. A later global type-ID table can replace these references without
changing source semantics.

## Option and Result

Use compiler-known type constructors with ordinary enum behavior:

```python
def find_positive(value: i64) -> Option[i64]:
    if value > 0:
        Option[i64].Some(value)
    else:
        Option[i64].Nothing

def checked_divide(left: i64, right: i64) -> Result[i64, str]:
    if right == 0:
        Result[i64, str].Err("division by zero")
    else:
        Result[i64, str].Ok(left // right)
```

The variants are `Some(T)` / `Nothing` and `Ok(T)` / `Err(E)`. `Nothing` is a
variant of one concrete option type, not a universal null value or a `None` type.
Writing `type Lookup = Option[i64]` permits `Lookup.Some(3)` and
`Lookup.Nothing`, reducing repetition without inference rules.

Require explicit type arguments or an alias initially. In particular, do not
guess the absent side of `Result` from a constructor argument. Expected-type
inference for abbreviated constructors is an ergonomic follow-up; it is not
needed for a usable type-safe first implementation. No `?`, implicit unwrapping,
exception conversion, or truthiness is introduced.

The present frontend represents `()` as the absence of a stack result rather
than a `Ty`. Keep that restriction explicit in the first implementation: payload
fields and Option/Result arguments require stored value types, just as collection
elements do today. Thus `Result[(), E]` initially diagnoses an unsupported unit
payload. A concrete `enum Outcome: Success; Failure(E)` (written as indented
variants) already expresses that result. Before claiming a complete standard
`Result` facility, add unit as a first-class value type or erased field metadata;
this must be a deliberate follow-up, not an accidental parser limitation.

## Matching and control flow

Introduce `Statement::Match { scrutinee, arms }` and a frontend pattern AST.
Patterns are qualified variants with bare immutable bindings or `_` for each
payload position, plus an optional whole-value wildcard `_`. The initial slice
does not include guards, OR patterns, nested destructuring, literal payload
patterns, or implicit capture patterns. These restrictions make finite coverage
checking exact and linear in the number of variants and arms.

```python
def unwrap_or(value: Option[i64], fallback: i64) -> i64:
    match value:
        case Option[i64].Some(number):
            number
        case Option[i64].Nothing:
            fallback
```

Evaluate the scrutinee exactly once. Each case gets a fresh local-name scope;
bindings may shadow outer locals, but duplicate names within one pattern are an
error. Restore the outer name scope after each arm and after the match. A binding
copies an independent value; matching never partially moves its scrutinee in the
first slice. A payload containing a collection therefore retains the existing
independent-update behavior.

A variant arm covers that entire variant. A repeated variant is unreachable. A
wildcard covers all remaining variants and must be last; diagnose a redundant
wildcard after explicit complete coverage. Missing coverage lists missing qualified
variants. Constructors and patterns from a different nominal enum are errors,
even if their variant names and payload types happen to match.

Use the existing `BlockResult::Exits` rules: continuing arms must agree in result
type; arms ending in `return`, `break`, or `continue` contribute no join value.
If every arm exits, the match exits and subsequent statements are unreachable.
A match in a function's tail position yields its continuing arm's final expression,
exactly as `if` does today. Elsewhere the match result is discarded. Arbitrary
inline `x = match ...` is deferred with the same limitation as multiline `if`;
do not introduce a second expression-block grammar just for this feature.

Loop exits inside a case target the enclosing source loop. A `continue` in a
`for` case runs the existing iterator step exactly once before jumping. Preserve
tail-call marking for calls that are final expressions or explicit returns inside
case arms.

The backend already has `Op::Match`, scalar patterns, an independent match
checker, and Cranelift dispatch/join lowering used by the legacy syntax. Reuse
them rather than synthesizing nested source `if`s. Scalar patterns may be exposed
in the new frontend at the same time: booleans require both values or `_`, whereas
integers and strings require `_`, matching the existing checker. Do not expand
scalar coverage algorithms as part of enum support.

## Native representation and operation checking

Initially all enum values use a pointer-sized immutable tagged record. A concrete
record carries a `u64` tag followed by one `u64` slot per active payload field.
Small integers and booleans use the same pack/unpack rules as collection elements;
strings, collections, and nested enums occupy pointer slots. Tags are assigned in
declaration order and are private to the compiler/runtime ABI. Do not promise a
C ABI, stable cross-version layout, unboxed layout, or niche optimization.

This representation fits current `clif_type`, function parameters/results, local
variables, branch block parameters, collection storage, and tail calls. It avoids
flattening aggregates throughout the operation stack or adding hidden result
pointers. Allocation is a runtime cost accepted to keep the first implementation
small and compilation fast. Shared immutable records mean assignment does not
copy the payload or permit mutation through another binding.

Add a checked constructor operation carrying enum identity and variant index.
It pops the declared fields in order and pushes the enum type. Extend match arms
with payload destination slots, or equivalently a checked binding prologue whose
field types are derived from the selected variant. Prefer arm-local binding
metadata: it prevents an unchecked payload projection from being emitted outside
a successful variant test. The independent operation checker verifies identity,
tag bounds, arity, destination types, coverage, and each arm's stack result.

A smaller integration alternative, preferred for the first implementation, stores
the scrutinee in a hidden typed local, lowers a checked enum-tag operation to an
integer, and reuses existing `Op::Match` with integer patterns. Each selected arm
loads its payload from the hidden local with checked enum/variant/field metadata.
Frontend coverage checking must run before this erasure; ordinary integer coverage
does not know the finite enum domain. Emit an explicit invalid-tag trap fallback
rather than treating an arbitrary integer tag as the final enum variant. Document
the independent checker's boundary: it verifies projection identity/index/type,
while the structured frontend guarantees the projection follows its variant test.

Native dispatch loads the tag once, branches using the existing match chain, and
only loads payload fields inside the selected case. A verified exhaustive match
still traps on an invalid runtime tag to expose compiler/runtime bugs. Keep a
compare chain initially; choosing a Cranelift switch/jump table is an isolated
later optimization.

Allocation and enum type metadata should live behind a small runtime interface.
An implementation can use generated read-only metadata or per-type compiler
generated callbacks for equality, printing, and destruction. Do not bake display
names into the nominal type identity or require source parsing at runtime.
The shared runtime decision is a 16-byte managed header containing `u64 refs` and
`void (*destroy)(void *)`, followed by the enum tag and active payload slots.
Immortal objects use `UINT64_MAX` references and a null callback. Generic retain/
release calls the object's destructor at its last release; each enum destructor
uses active-variant field metadata. This callback boundary also prevents scalar-
only programs from acquiring link dependencies on every aggregate runtime module.
Ownership/strings implementation owns this header contract; enum support reuses
it. Until reclamation actually lands, explicitly document process-lifetime
allocation as for current collections; tagging a record does not itself implement
ownership.

## Collections, strings, and ownership interfaces

Permit enums in list elements and dictionary values, including enum fields that
contain collections. Continue to reject enums as set elements and dictionary
keys initially, following the current closed hashable-type set. This avoids
accidentally committing to public hash/eq traits or deriving hashability.

Current collection descriptors are recursive prefix strings and runtime operations
perform generic equality and printing. Merely adding an enum pointer to `Ty` is
not enough: `list[Reading]` must print and compare its active payload correctly.
Extend descriptors with an enum identifier mapped to static variant metadata, or
replace descriptors with shared static type metadata in one coordinated change.
The enum descriptor must encode a concrete identity and field descriptor table;
a bare opaque-pointer descriptor would silently give pointer equality, which is
incorrect for independent values. Do not enable collection-containing enums until
these runtime paths have tests. Equality requires the same nominal type, same tag,
and pairwise equal active fields. No ordering operators are derived.

Printing uses qualified variant names, for example `Reading.Missing` and
`Reading.Value(42)`, with existing collection element/string formatting conventions.
String payloads reference the single `str` runtime representation; enums do not
create another string type. Their equality must compare the complete counted
string, including embedded zero bytes, once the string runtime is updated.

All currently admitted enum fields have value semantics. Ownership refinement:
an enum remains value-copyable only if all its fields are value-copyable; a future
resource-owning or generator payload makes the enum move-only. Initially exclude
those payload categories. Shared enum storage owns its fields, so final release
destroys only the active variant. Match bindings retain/copy independently; no
borrowed pattern bindings, references in enums, or partial moves are promised.

Generator `next` can return a concrete `Option[T]`, and exhaustion is `Nothing`.
The enum type/constructor ABI must therefore be usable from compiler-generated
resume functions, without source-level generic functions. Generator state is not
an enum language value and does not need to use this allocated record layout.

## Implementation sequence and acceptance checks

1. Add enum declaration parsing, name collection, type resolution and interning;
   diagnose duplicate names, bad type arguments and cycles. Keep this logic in a
   focused frontend/type module rather than growing the existing expression match.
2. Add qualified constructors and typed operation checking; implement immutable
   record construction, printing, and equality in the runtime/native lowering.
   Verify enum arguments, results, aliases, and nominal type mismatches.
3. Add frontend `match` suites, payload slots, exhaustive coverage and backend
   projection. Test all combinations of normal results and early exits.
4. Instantiate Option/Result through the same enum metadata path. Verify nested
   options/results, aliases, explicit type argument errors and empty variant values.
5. Connect collection descriptors and managed-object ownership hooks. Add nested
   enum/collection tests before advertising those combinations.
6. Update DESIGN.md's implemented/proposed boundary and TUTORIAL.md with runnable
   enum, Option and Result lessons. Its existing executable-guide test checks them.

Required regression cases include multiple fields with different integer widths;
zero-field and string payload variants; constructors with side effects; repeated
matches of the same value; zero bytes in string payloads; nested concrete enums;
independent updates after extracting a collection; list equality/printing with
distinct-but-equal enum allocations; wildcard coverage and arm-local shadowing;
non-exhaustive/repeated/wrong-type variants; missing and excess payload arguments;
wrong payload binding arity; all-exiting and partially exiting matches; `break` and
`continue` in nested loops; direct and mutual tail calls inside case arms.

Run frontend diagnostics, native executable tests, the executable tutorial, the
full existing suite, formatting and Clippy. Add a large concrete enum/alias input
to catch accidental exponential resolution or repeated specialization; do not
introduce a specialization optimizer or broad benchmark framework for this slice.
