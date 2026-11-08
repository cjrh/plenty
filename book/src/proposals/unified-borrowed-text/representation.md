# Borrowed text: representation and migration

**Status: Unimplemented assessment accompanying [One borrowed text type](../unified-borrowed-text.md).**
The public contract and generic-reference exception there are recommendations.
This page records layout alternatives and acceptance evidence; it does not
choose an ABI, schedule implementation, or change the current reference.
The delivered separate-view design keeps ordinary `&str` and the existing
two-word `&strview` (`&substr` alias), without a cached scalar count. Range
operations, direct byte access and read-only standard-library inputs are
described by the [current reference](../../design/facilities/01-builtins/text.md)
and [input inventory](../../design/stdlib/text-input-views.md). The ABI changes
and generic specialization discussed below apply only if unification is adopted.
Planned work remains in B12/B11 in the [backlog](../../backlog.md).

## Representation choices

The [current architecture](../../design/toolchain/02-architecture/values.md#strings)
uses a one-word owning string handle. A heap/literal handle points at a prefix
containing lengths and storage metadata. Up to seven UTF-8 bytes fit inside the
handle itself. A shared reference addresses the handle's slot; a substring view
instead holds a byte address and byte length. Neither borrowing form retains
storage.

All candidate layouts below keep the owned representation unchanged and avoid
allocation and retains when borrowing. Word counts are conceptual native-word
payload sizes, not measurements of generated call sequences or complete slots.

| Candidate | Payload | Benefits | Costs and risks |
| --- | --- | --- | --- |
| A: current substring layout | Byte address, byte length (two words) | Reuses the existing view machinery; compact and directly usable by byte consumers | Scalar `len` scans even for whole-string borrows; ASCII detection and negative indexing may need new scans. |
| B: eager scalar count | Byte address, byte length, scalar count (three words) | Constant-time `len`; ASCII can be recognized by equal byte/scalar lengths; whole borrows reuse owned counts | Larger values and calls; substring creation may scan more text to establish the count; requires support beyond the existing two-word view operand. |
| C: tagged whole/range | Distinguishes an owning-slot borrow from a byte-range borrow | Can reuse cached counts and ownership provenance for whole strings | More branches and representation cases; range counts still need storage or scans; exact packing and spill costs unproven. |
| D: origin plus offsets | Origin handle-slot address and range bounds, with optional scalar count | Keeps origin explicit; can address inline bytes relative to their stable slot | Extra indirection, wider ranges/metadata, and special handling of nested views; no cheap owned substring exists merely because its origin is known. |

**Baseline: A, the current two-word view.** Keeping scalar semantics does not
require caching scalar length. Prototype B only when workloads justify the
larger payload and additional substring-construction work. Cached counts help
repeated length checks and ASCII detection; they do not supply scalar-to-byte
offsets. See the [length and indexing comparison](evidence.md#length-indexing-and-cached-counts)
for Plenty, Rust and CPython.

For candidate B, computing a substring's count may require scanning its selected bytes, including
an open-ended suffix for which the current view only finds a start boundary.
Whole-string borrows should read the owner's cached count. Subview construction
can reuse known counts for whole ranges or count during boundary traversal;
measure positive, negative and clamped bounds rather than claiming construction
is always constant-time or that the count is always free.

Candidate B exceeds the two-word view shape used today. The compiler must
account for operands, locals, calls/results, carried references, runtime helper
arguments and formatting metadata. A temporary stack descriptor could keep a
call operand small but introduces its own lifetime/indirection costs; it is not
a cost-free substitute for value-layout support. Do not cram counts into
pointer bits or narrow a length without a proved range and portability contract.

Candidate C must not restore a public `*r -> str` only for whole-string values.
An optimizer may exploit known provenance internally while the source contract
remains uniform. Supporting an owned string that shares an arbitrary subrange
would instead change the owning representation, retention and copying contracts;
that is outside this proposal.

## Byte stability and loans

For a heap-backed origin, a live view protects the owning storage. For an inline
origin, it protects the actual bytes in the owning slot. Taking a copy of a
handle into a helper-local slot and returning a pointer into that copy is invalid.

Loan propagation must cover local owners, object fields, list/dictionary
elements, shared-owner content, exclusive-slot reborrows, literal storage, and
full-expression temporary owners. Replacing, moving or relocating an origin
must remain forbidden while any of its text views is live. Runtime helper
calls must not retain a borrowed byte pointer beyond its checked lifetime.

If returned views are implemented, their analysis must distinguish a live
origin from a transient view descriptor. Narrowing or forwarding a view must preserve the
origin loan, not borrow only the callee's descriptor slot. The same distinction
applies when `Option`/`Result` carries a successful borrowed result. Arbitrary
stored references and multi-origin lifetime inference remain separate B11 work;
they need not be implemented to return a view of the supported single origin.

## Operations and library boundaries

Unification is useful only when callers can inspect substrings without copying.
Cover existing whole-string operations and text arguments, not just methods
already implemented for `&substr`:

- Length, byte length, indexing, iteration, ordering, equality, membership,
  prefix/suffix searches, reverse searches and character classification.
- Parsing, splitting, joining, case conversion, replacement and trimming.
  These keep their existing owned results and error/allocation behavior;
  unification does not silently turn `strip` or `split` results into views.
- `std` text readers, parsers, encoders, regex, formatting and output paths.
  Their borrowed inputs operate on a range directly; APIs that deliberately
  store text keep owning inputs or explicitly copy it.
- User protocol methods, generic functions, compiler intrinsics and generated
  interfaces. Matching source spelling alone is insufficient when their
  implementations still expect a headed owning handle.

Native interoperability also needs an audit. The current
[`utf8` adapter](../../design/facilities/04-interop/borrowing-text.md#text-representations)
can naturally pass a unified view as address plus byte length. The `c_string`
adapter must still reject embedded NUL and create its fallible terminated
copy, now from the selected range. Public C adapter contracts can stay stable
even though Plenty's internal function ABI changes. Compiler/runtime artifacts,
compiled std bodies and affected caches must agree on the new layout; existing
private machine-code interfaces cannot be assumed compatible.

Formatting retains separate whole-text and substring metadata; its existing
`std.fmt` helper names now accept the separate text view. Consolidation should preserve escaping, Unicode
format widths, precision and fallible output contracts. Remove redundant helper
spellings only through an explicit source migration.

## Migration and delivery boundaries

This is one candidate scope under B12, with returned-text support drawing on B11.
The following are dependency/acceptance boundaries, not an additional work queue
or a commitment to implement the design.

1. **Resolve semantic and layout gates.** Use the [current inventory](evidence.md),
   audit owner-retaining helpers, and establish A as the baseline. Decide the
   generic exception, `copy(r)` rule and public performance contracts. Do not rename the public
   type before these decisions are reviewable.
2. **Establish range-based text operations.** Teach existing helpers and std
   input paths to consume checked ranges without allocating an intermediate
   owner. Keeping the current source types during this work permits independent
   validation of operation parity.
3. **Resolve returned-reference migration.** Prefer owned `str` results for
   whole-string getters. Decide generic `&V` results explicitly; do not silently
   require returned substring views or globally replace references with owned
   values. General returned views remain B11 work. Establish the narrower byte-
   view loan support needed for `as_bytes()` as part of byte access.
4. **Change the public spelling and migrate together.** Treat `&substr` as a
   temporary deprecated alias if a compatibility period is chosen; both
   spellings must mean exactly the same view. Such an alias cannot preserve
   old `&str` ownership extraction, so diagnostics must explain that break
   directly. No editions mechanism is assumed.

Migration should identify `*r`, `copy(r)?`, implicit reads that materialize an
owned `str`, and `&T` specializations over `str`, not merely replace spelling.
When a callee keeps an owner, prefer an owned `str` parameter and handle sharing;
when it intentionally copies a substring, retain visible fallibility. Read-only
uses should use the view directly.

Update the Text and slice references, ownership/returned-reference rules,
architecture, FFI and formatting guidance, cheat sheet, Plenty skill, generated
stdlib signature index, and affected tutorial/application examples in the
implementation PRs. Revisit examples that currently allocate a slice just to
pass it to a reader. Keep simplified teaching examples deliberate. Record only
actually shipped scope in completed-work records and narrow B12/B11 accordingly.

## Acceptance evidence

Correctness coverage must distinguish literal, inline and heap strings; ASCII,
multibyte scalars, combining marks, empty ranges and embedded NUL; positive,
negative, extreme and clamped bounds; and whole versus narrowed borrows. Check
that every supported read operation agrees on owned and borrowed text and that
allocating operations propagate allocation failures normally.

Borrow tests must cover migrated getters, forwarding and nested calls,
supported `Option`/`Result` success/failure paths, full-expression temporary owners,
exclusive-slot replacement conflicts, generic iteration over `list[str]`,
borrowed matching, and field/element origins. Add rejection cases for local-owner
escape and dangling inline bytes. Exercise external adapters and custom protocol
implementations rather than relying only on builtins.

Measure optimized kernels for whole-string calls, repeated scalar `len`, ASCII
indexing, negative indexing, scanning, substring construction, nested subviews,
formatting, and migrated getters. Test returned subranges only if that capability
is included separately. Compare any layout prototype against current
whole-string and substring behavior. Include tiny values and many simultaneous
borrows to expose copy/spill costs; include large Unicode suffixes to expose
counting costs. Report compile time, code size, stack/register pressure and
retains/releases as well as throughput. Follow the repository's
[emitted-code method](../../development/emitted-code.md) for generated-program
performance: isolate kernels, compare with C and attribute instruction counts
before changing the compiler. No performance measurements were performed for
this design record.

## Source evidence

The assessment rechecked these files at `6bfe23c2`; paths are implementation pointers,
not a stable public interface:

| Source | Relevant boundary |
| --- | --- |
| `src/frontend/views.rs` | Whole-string conversion, temporary loans, supported operations and local-only view diagnostics. |
| `src/text.rs` | Separate text/view operations and the two-word view signatures. |
| `src/frontend/references.rs`, `src/op.rs` | Reference lowering, reference/view type identity and storage restrictions. |
| `src/codegen/metadata.rs`, `src/codegen/views.rs` | Distinct view formatting descriptors and byte/count operand use. |
| `plenty-runtime/src/strings.rs` | Owned header, inline representation, cached lengths and existing string primitives. |
| `plenty-runtime/src/aggregates/text_views.rs` | Byte-range packing, scanning scalar length, narrowing and fallible copying. |

The [evidence assessment](evidence.md) adds regex byte-adapter, library, generic
reference and getter pointers, executed probes, and external comparisons.

Local issue #595 and PR #738 record the original separate-view decision; issue
#847/PR #1028 added implicit whole-string borrowing. Issue #802 is closed
(fixed by local PR #1030); its diagnostic fix is no longer an outstanding
independent prerequisite for this proposal.
