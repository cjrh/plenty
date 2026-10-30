# Slice syntax implementation plan

**Status: Not implemented.** This page plans the
[slice syntax proposal](../slice-syntax.md) (issue #579). Its code fences show
proposed syntax and are not compiled. File names refer to local `main` at
`f681a0d5`.

## Reviewable slices

Each slice is one local PR. Each updates the reference pages, cheat sheet rows
and tutorial lessons for what it ships, following the
[development workflow](../../development/workflow.md).

1. **Parsing, diagnostics and copyable lists.** Add `Expression::Slice { base,
   start, stop, step }` in `src/frontend.rs`; the postfix loop that builds
   `Expression::Index` recognizes a top-level `:`. Reject slice assignment
   targets (`Statement::SetIndex` and augmented assignment), commas, `&` and
   `&mut` applied to a slice, and unsupported receivers,
   with the messages in the [proposal](../slice-syntax.md#diagnostics).
   Generalize the runtime's `slice_bounds` (`plenty-runtime/src/aggregates.rs`)
   to the stepped algorithm, and extend `try_list_slice` to copy strided rows of
   copyable elements, including `str` retains. The step uses the nonzero
   integer type, so S1 depends on issue #584 for written steps; slices without
   a step can land first if #584 is not ready.
2. **Lists of affine elements.** Reuse the runtime's `try_copy` for each
   selected element, and `share()` for `Shared[T]`. Elements with nested
   `Shared` fields work once issue #582 lands. Check copyability per
   specialization, with a diagnostic that names the element type. Owned
   temporaries may transfer selected elements instead of copying them.
3. **Strings.** Extend `try_slice` in `plenty-runtime/src/strings.rs` to steps:
   byte offsets for ASCII text, one bounded scan for other text, an exact-size
   result. Retain the source for a whole-string step-1 slice.
4. **`Bytes` and `bytes`.** Lower `Bytes` slices to the native storage copy used
   by `Bytes.slice`, without its range rule. Make `bytes` slices produce a
   `Bytes`, loading `std.bytes` when the program does not import it.
5. **Tuples and columnar lists.** Compute tuple positions in the checker from
   literal bounds and build the result tuple type. Add a strided column copy
   for `clist[T]`.

The first slice alone covers most uses (`xs[1:3]?` on lists of numbers or
strings). Slices 2 to 5 depend on the first but not on each other.

## Tests

Extend `tests/test_slicing.rs`, which holds the existing `slice` method tests:

- **Normalization model.** A Rust model of `PySlice_AdjustIndices` inside the
  test, compared against compiled programs for list lengths 0 to 6, every bound
  from -8 to 8 and omitted, steps -3 to 3 except zero, and the extremes
  `i64.MIN` and `i64.MAX` for bounds and steps. The table of examples in the
  proposal is a separate `rstest` case list, with results checked against
  CPython when it was written.
- **Runtime unit tests** in `plenty-runtime` for the generalized
  `slice_bounds`, including a step of `i64.MIN` and lengths 0 and 1.
- **Element duplication.** Nested lists, dicts and tuples of lists are
  independent after the slice; mutating the copy leaves the source unchanged.
  `Shared[Mutex[i64]]` elements are shared: a change through one list's handle
  is visible through the other.
- **Allocation failure** under the `runtime-checks` feature, following
  `every_list_slice_allocation_failure_preserves_borrowed_sources`: for every
  allocation budget of a nested-list slice, the result is
  `Err(AllocError.OutOfMemory)` or the full slice, and the source prints
  unchanged. Under `--all-features` the allocation-checks runtime confirms at
  exit that partial results were freed.
- **Evaluation order.** Bounds with printing side effects run once, left to
  right, after the receiver; bounds may read the receiver.
- **Parsing.** `TABLE[n:]` on a module constant works while issue #562 is open;
  conditional and call expressions as bounds; `xs[1:3]?[0]` and
  `len(xs[1:3]?)`.
- **Diagnostics.** One `plenty-error` style case per row of the proposal's
  diagnostics table. The `&xs[a:b]` rejection is a phase-guard test with a
  comment naming issue #581, B11 and B39, to be replaced when views land.
- **Step type.** Literal steps convert at compile time; literal and constant
  `0` are compile errors; a plain `i64` step is a compile error whose hint shows
  the issue #584 conversion; a converted run-time step slices correctly.
- **Strings.** ASCII and mixed text, negative steps, combining marks,
  results of seven and eight UTF-8 bytes (inline versus heap), and the
  whole-string case.
- **Tutorial and reference fences**, run by `test_tutorial::` and the design
  example tests.

## Documentation and tutorial

A new reference page, `design/language/04-expressions/slices.md`, holds the
syntax, the normalization algorithm, the examples table, the step type and
the reserved `&` form. Receiver pages link to it and state their result types
and costs:

| Page | Change |
| --- | --- |
| `design/facilities/01-builtins/lists.md#slices` | Lead with `xs[a:b:c]?`; keep `slice()` as the transfer from an owned temporary |
| `design/facilities/01-builtins/text-building.md` | Add `text[a:b:c]?` to the table; remove "Slice syntax and steps are unsupported" |
| `design/stdlib/bytes.md` | Replace "There is no slice syntax" with the clamping syntax and its `AllocError`; keep the method range rule |
| `design/language/03-types/bytes.md` | Replace "no slicing" with the `Bytes` result |
| `design/language/03-types/tuples.md` | Literal-bound slices and their types |
| `design/facilities/01-builtins/clists.md` | Row slices; `items[:]?` as the duplicate |
| `design/examples/cheatsheet.md` | Use `name[1:3]?` and `items[1:3]?` in the examples; replace "There is no slice syntax"; change the Python row to `xs[1:3]` → `xs[1:3]?` ("allocates a copy") |
| `design/examples/collections/list-slices.md`, `design/examples/text-files/text-23-slice-text-by-unicode-scalar-position.md` | Show the syntax first, then the methods |
| `design/toolchain/02-architecture/support.md` | List slice syntax |

Tutorial:

- New lesson "Take part of a list or text" in
  `tutorial/04-collections/`, after
  [Transfer or copy ownership](../../tutorial/04-collections/moves-and-copies.md),
  because a slice is a copy. It teaches `xs[a:b]?`, omitted bounds, negative
  positions, `[::-1]`, clamping, and the cost of copying; it shows nested lists
  being deep-copied, and recommends a `range` loop or pipeline over the source
  when the copy is only iterated.
- [Build text with recoverable allocation](../../tutorial/03-absence/allocating-text.md)
  mentions `text[a:b]?` as another allocating text operation.
- [Byte buffers](../../tutorial/08-resources/24-byte-buffers.md) keeps
  `frame.slice(1, 3)?` where the function's error type is `BytesError`, and
  notes the syntax.

Review other lessons and `stdlib/std/*.plenty` for `.slice(` calls and manual
sub-range loops that the syntax states more clearly, following the
[tutorial guidelines](../../tutorial/index.md). The standard library has 45
`.slice(` calls; migrate only where the result is clearer, as a separate PR.

Status updates when the work lands: this proposal's status blocks, the
[string design](../completed/strings.md) note "slice syntax remains a B12
candidate", and the [proposals index](../index.md).

## Backlog note

The backlog is edited only after merge. Proposed B39 wording, replacing
"Slice syntax is deliberately omitted: it would hide a copy.":

> Slice syntax (`buffer[a:b:c]?`, issue #579) copies with Python's clamping and
> returns `Result[Bytes, AllocError]`; `bytes` slices produce a `Bytes`. See
> `[slices](design/language/04-expressions/slices.md)`. Remaining: borrowed
> sub-range views `&buffer[a:b]` (issue #581; needs B11 stored references).

B12 lists "borrowed sub-range views (B11/B39)"; add that their spelling is
`&xs[a:b]`.
