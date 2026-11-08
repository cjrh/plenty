# One borrowed text type

**Status: Not implemented; background alternative for discussion.** The implemented
design preserves ordinary `&str` references and names the separate text view
`&strview`, with `&substr` retained as an alias. See the
[Text reference](../design/facilities/01-builtins/text.md). Unification remains
unapproved. This page explores making `&str` the borrowed text view while
retaining the existing owning `str`. The [Text reference](../design/facilities/01-builtins/text.md),
[substring views](../design/language/04-expressions/slices.md#substring-views),
and [returned references](../design/language/05-ownership/04-returned-references.md)
define current behavior. B12 tracks text-view extensions; returned views relate
to B11. Recording this design does not approve implementation or choose its
runtime layout. See [representation and migration](unified-borrowed-text/representation.md)
for the implementation tradeoffs and adoption gates, and the
[evidence and naming assessment](unified-borrowed-text/evidence.md) for current
implementation pointers and migration examples.

## Motivation

A function that reads text should usually accept either a whole string or a
substring without the caller allocating. The original motivation was the gap
between ordinary `&str` parameters and the limited `&substr` view. The selected
design addresses that usability gap through a complete separate `&strview`
reader, while retaining ordinary string references. Passing `&s[a:b]` to an
ordinary `&str` parameter still fails; read-only APIs use `&strview` instead.

The distinction follows the current representation, but need not be a public
language distinction. The desired interface is:

```text
def inspect(text: &str) -> i64:
    len(text)

# Proposed: all three calls borrow without copying text or retaining storage.
inspect(s)
inspect(&s)
inspect(&s[a:b])
```

Owning and borrowed text remain different. An owning parameter is appropriate
when the function needs an independent value to store or return; a borrowed
parameter is appropriate when it only reads the text.

## What exists today

The original investigation inspected local `main` `6bfe23c2`. The current
separate-view implementation is described in the [Text reference](../design/facilities/01-builtins/text.md)
and [text input inventory](../design/stdlib/text-input-views.md). The
[evidence assessment](unified-borrowed-text/evidence.md) preserves the original
probes and records the delivered route; this unification proposal remains unimplemented.

| Form | Current meaning |
| --- | --- |
| `str` | Copyable owner of immutable UTF-8. Copying a heap-backed handle shares storage without allocating. |
| `&str` | Shared reference to an owning string slot. `*r` obtains an owned handle without copying text. |
| `&strview` (`&substr` alias) | Borrowed byte range on scalar boundaries. No owning handle can be recovered from its address and length. |
| `&mut str` | Exclusive reference that can replace an owning string binding. It cannot edit string bytes. |
| `s[a:b]?` | Fallible owned copy, independent of the original storage. |
| `&s[a:b]` | Allocation-free borrowed substring with scalar-position bounds. |

Owned strings cache their scalar count. Substring views contain only their
first byte's address and byte count, so `len(view)` scans. A whole-string
`&strview` parameter already accepts an owned argument through implicit
borrowing, including a temporary held for the full expression.

Whole-string references can be returned, including through the supported
`Option`/`Result` forms. Substring views cannot currently be returned. Neither
kind can be stored freely in aggregates. These are separate capabilities:
renaming a view does not implement returned-view analysis or stored lifetimes.

## Recommended public contract

The candidate design has one shared borrowed text type, provisionally `&str`, regardless
of whether it covers the whole owner or a range. The following are proposed
rules, subject to the adoption gates rather than implemented promises.
A unified view could instead use the `&strview` spelling; that would change
its relationship to ordinary references beyond the implemented separate-view
name. The [unification alternatives](unified-borrowed-text/evidence.md#naming-and-type-identity)
remain open.

- `str` remains the existing owning, immutable, copyable type. String literals,
  inline strings, shared heap storage, and owned assignment keep their contracts.
- `&s` and `&s[a:b]` both produce `&str`. A shared text borrow owns no storage,
  retains nothing, and keeps its origin borrowed until its last use.
- Bounds continue to count Unicode scalars, clamp as today, and accept negative
  positions. Borrowed ranges remain contiguous and have no step.
- A borrowed-text parameter accepts owned strings through the existing implicit
  borrowing rule. A local borrowed binding still requires an explicit borrow
  or an existing reference; this change does not broaden implicit borrowing.
- Reborrowing a view preserves its range. Borrowing a narrower range cannot
  regain bytes outside the original view.
- All read-only text operations accept whole strings and views. Operations
  producing owned text retain their existing fallible allocation contracts.
- `&mut s[a:b]` remains invalid. Shared text is immutable; `&mut str` retains
  its distinct role of replacing an owning string slot.

The recommendation changes neither Unicode normalization nor what `len` means:
length remains the number of Unicode scalar values, not bytes or graphemes.
`byte_len()` remains constant-time. A two-word view with scanning scalar `len`
is an acceptable baseline; constant-time scalar length is not an adoption
requirement. A cached count is an optional optimization requiring workload
evidence. Iteration yields one-scalar `str` values, as it does today, and can
walk byte boundaries directly without first computing scalar length.

Cheap byte access is part of the proposed contract: `as_bytes()` borrows the
UTF-8 range without allocating, direct byte-copy operations accept text views,
and byte iteration avoids decoding scalars. The [byte-access assessment](unified-borrowed-text/evidence.md#byte-access-and-binary-operations)
records current gaps, the internal regex precedent, and broader ASCII byte
operations. Public byte-view loan propagation still needs implementation; the
internal adapter alone does not make `as_bytes()` available today.

## Obtaining ownership

The principal intentional loss is that a shared text borrow no longer guarantees
access to an owning string handle. An arbitrary substring has no such handle.
Consequently `*r` must not silently allocate, become fallible, or succeed only
when `r` happens to span a whole owner.

The recommended rule rejects using `*r` to obtain an owning `str`; `r` already
supports text operations directly. Use the existing copying slice `r[:]?` when
an independent owned copy of the viewed bytes is needed. Like copying slices
today, long results allocate even if the range covers the whole input. Inline
results need no heap allocation, but the static result type remains fallible.

```text
# Proposed migration when the function only observes text:
def width(text: &str) -> i64:
    len(text)                 # no dereference needed

# Proposed migration when the function deliberately copies borrowed bytes:
def duplicate(text: &str) -> Result[str, AllocError]:
    Ok(text[:]?)             # explicit fallible owned copy

# Existing ownership idiom to retain a whole string cheaply:
def keep(text: str) -> str:
    text                     # another owner; no text-buffer copy
```

Changing a parameter from `&str` to `str` is often the right migration for a
function that keeps a whole string: it preserves cheap handle sharing. A caller
with only a substring must then explicitly copy. This makes the retention cost
visible at the boundary that needs ownership.

`copy(r)` also needs an explicit migration rule: today it can retain a whole
string handle, whereas `copy` rejects substring views. Recommend rejecting it
for unified borrowed text and pointing to `r[:]?`. Copying an owned `str` keeps
its current behavior. An alternative is to define `copy(r)` as a fallible byte
copy; choosing it would require changing diagnostics and documenting its new
cost. Do not leave behavior dependent on hidden whole-string provenance.

## Generic code and exclusive references

This is a semantic change to `&T` when `T = str`, not just a shorter spelling.
Consider a generic body:

```text
def read[T](value: &T) -> T:
    *value
```

For an ordinary copyable `T`, the dereference reads its stored value. Under this
proposal, instantiating it for `str` would instead receive a text view and cannot
produce an owning `str` without allocation. This affects generic utilities,
borrowed iteration over `list[str]`, borrowed matches, and protocol signatures
that mention `&T`. Copyability of owned `str` must not imply an infallible
ownership conversion from its shared text view.

The bounded recommendation is to specialize shared `&T` to the text view at
`T = str` and reject operations requiring an owning dereference at that
instantiation, with an instantiation note. Borrowed collection elements then
expose text views consistently. Generic algorithms that need owners should take
owned inputs or an explicit copying callback. This is a substantial ergonomic
exception; assess real generic code before approving it. An abstract-body bound
that promises dereference-to-`T` must either exclude this specialization or have
a separately justified meaning. The existing `CopyType` bound describes
ordinary value copying, so it cannot by itself promise ownership extraction
from the proposed text view. The [generic-code inventory](unified-borrowed-text/evidence.md#generic-reference-impact)
includes concrete standard-library boundaries, especially returned `&V` values.

`&mut str` should continue to reference the owning slot, so `*out = replacement`
can replace the binding and a read through that exclusive slot can copy its
owning handle. Lending it as shared text creates a view of the current bytes
and freezes replacement until that child loan ends. Shared reborrowing thus
involves conversion, not merely changing a permission bit. The checker must
apply this consistently through generic `&mut T`, field projections, and calls.

Before adoption, decide whether this exception is acceptable or whether Plenty
needs a separate explicit way to borrow an owning slot. Reintroducing a second
routine borrowed-text type would defeat the motivating simplification.

## Returning text and compatibility boundaries

If unification were adopted, existing whole-string getters could return owned
`str`, sharing the existing handle. The selected separate-view design preserves
ordinary `&str` getters and generic references; it requires no such migration. Do not require returned substring views just
to preserve old `&str` signatures. The public `std.mime.MediaType.type_name()`
and `subtype()` getters are concrete examples. Returning `str` avoids copying
the text buffer, but heap-backed handle copies still have retention/release
costs and may keep the whole allocation alive.

Current Plenty **does allow returned references**, under its first-reference-
origin rule; these getters are valid today. Changing them is a deliberate API
migration, not a correction of forbidden code. Callers with explicit `&str`
annotations or dereferences also need migration.

A function receiving only a text view cannot recover an owner by changing its
return annotation. It must accept an owned `str` when whole-owner retention is
its purpose, or explicitly make a fallible copy of the selected bytes. Generic
returned references such as `Option[&V]` need their own decision when `V = str`;
changing all such functions to return `V` would break noncopyable value types.

Returning actual substring views remains separate B11 work. If added, it must
preserve the supported origin through narrowing, forwarding and carried
references, and must never return pointers into copied inline handles or local
temporary owners. A public `as_bytes()` also needs checked origin propagation;
an intrinsic or argument-scoped implementation can establish that narrower
capability without promising general stored or returned text views.

The [representation and migration assessment](unified-borrowed-text/representation.md)
compares layouts, identifies compiler/std boundaries, and sets the evidence
required before this contract can replace the current one.
