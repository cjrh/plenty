# One borrowed text type

**Status: Not implemented; proposal for discussion.** This page explores making
`&str` the borrowed text view currently spelled `&substr`, while retaining the
existing owning `str`. The [Text reference](../design/facilities/01-builtins/text.md),
[substring views](../design/language/04-expressions/slices.md#substring-views),
and [returned references](../design/language/05-ownership/04-returned-references.md)
define current behavior. B12 tracks text-view extensions; returned views relate
to B11. Recording this design does not approve implementation or choose its
runtime layout. See [representation and migration](unified-borrowed-text/representation.md)
for the implementation tradeoffs and adoption gates.

## Motivation

A function that reads text should usually accept either a whole string or a
substring without the caller allocating. Today, authors choose between `&str`,
which exposes all whole-string operations, and `&substr`, which accepts both
forms but offers fewer operations. Passing `&s[a:b]` to an ordinary `&str`
parameter fails even when the callee only inspects the text.

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

These facts are from source/reference inspection at local `main` `d42e1a18`
(Plenty 0.8.14), not newly executed compiler probes.

| Form | Current meaning |
| --- | --- |
| `str` | Copyable owner of immutable UTF-8. Copying a heap-backed handle shares storage without allocating. |
| `&str` | Shared reference to an owning string slot. `*r` obtains an owned handle without copying text. |
| `&substr` | Borrowed byte range on scalar boundaries. No owning handle can be recovered from its address and length. |
| `&mut str` | Exclusive reference that can replace an owning string binding. It cannot edit string bytes. |
| `s[a:b]?` | Fallible owned copy, independent of the original storage. |
| `&s[a:b]` | Allocation-free borrowed substring with scalar-position bounds. |

Owned strings cache their scalar count. Substring views contain only their
first byte's address and byte count, so `len(view)` scans. A whole-string
`&substr` parameter already accepts an owned argument through implicit
borrowing, including a temporary held for the full expression.

Whole-string references can be returned, including through the supported
`Option`/`Result` forms. Substring views cannot currently be returned. Neither
kind can be stored freely in aggregates. These are separate capabilities:
renaming a view does not implement returned-view analysis or stored lifetimes.

## Recommended public contract

The recommendation is to have one shared borrowed text type, `&str`, regardless
of whether it covers the whole owner or a range. The following are proposed
rules, subject to the adoption gates rather than implemented promises.

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
`byte_len()` remains constant-time. Preserving constant-time scalar `len` for
shared text is a preferred adoption requirement; its layout cost must be
measured rather than assumed negligible.

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
a separately justified meaning; no such new bound is proposed here.

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

Returned shared text must follow the existing first-reference-origin rule,
including returning a subrange, forwarding calls, and fields borrowed from an
origin object. An existing getter returning `&str` must remain usable. Supported
`Option[&str]` and `Result[&str, E]` results must keep their current control-flow
and loan behavior after their payload becomes a text view.

A returned view into a short inline string must point into the surviving origin
slot, never a callee's temporary copy. Owned parameters and local temporary
owners cannot escape. Call-scoped temporary arguments remain alive for the full
expression; their borrowed results cannot be saved beyond it. Shared views must
also retain the existing restrictions on storage and suspension until those
features are implemented independently.

The [representation and migration assessment](unified-borrowed-text/representation.md)
compares layouts, identifies compiler/std boundaries, and sets the evidence
required before this contract can replace the current one.
