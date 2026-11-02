# Structured compiler diagnostics

[Parent proposal](../compiler-refactoring.md). The diagnostic contract here is
proposed; existing CLI spelling remains the compatibility baseline.

## Current conversion chain

`src/frontend.rs` defines `Located` with a location string, message and
optional ownership `Demand`. `Attributed` marks a generic diagnostic already
attributed to its proper instance. `Token::error` constructs located errors.
`sendable` preserves those two concrete types across worker boundaries but
renders other boxed errors to strings.

`src/op.rs::check_with` converts function-check errors with `to_string()`
to collect parallel results. It later selects the first error in program
order. Ownership checking has richer origins and demand information in
`src/ownership.rs`.

In `src/frontend/ide.rs`, `recover` stores deduplicated strings in
`Recorder.diagnostics`. During `finish`, `diagnostic` searches the first
line for a `source:line:column: ` prefix and parses it back into a public IDE
`Diagnostic`. That public structure already exposes file, line, column and
message. `src/lsp.rs::check` converts these into client diagnostics.

The problem is loss of structure between producers and consumers, not the
absence of a public location structure. Human-readable text is functioning as
an internal interchange format.

## Recommended representation

Introduce an internal diagnostic payload shared by frontend/checking/editor
boundaries. Its initial contract should include:

- optional source identity and primary position;
- message and ordered related locations/notes;
- attribution context for generic instantiation;
- typed ownership requirements where callers currently inspect `Demand`.

An end range can be optional: tokens often record a start position, not an
exact expression extent. Do not fabricate a precise span. Diagnostic codes and
fix suggestions can be added when there are stable consumers; neither is a
prerequisite.

Keep source positions in the compiler's current coordinate convention. Convert
to negotiated UTF-8/UTF-16/UTF-32 units only in the LSP adapter. A source identity
must be resolvable without a process-global formatting round trip.

Keep semantic demand information separate from display strings. Preserve
`Attributed`'s protection against attributing an inner function's error to an
outer instantiation. Related definition/use sites must have deterministic order.

### Error-type alternatives

**Preferred incremental option:** a concrete, sendable compiler diagnostic,
plus explicit conversion of I/O/toolchain failures at phase boundaries.
Existing public `Box<dyn Error>` return signatures can wrap it while callers
migrate. A presentation-only error remains possible for external failures
without source positions.

**Alternative:** a broader compiler-error enum distinguishing diagnostics,
I/O and backend/link failures. This provides exhaustive handling, but requires
more API migration and risks turning an editor improvement into a repository-wide
error redesign. Adopt only if actual callers need those distinctions.

Avoid defining the core diagnostic inside the IDE module: ordinary compilation
and worker checking should not depend on editor recording. Do not add a new
diagnostic framework dependency merely to remove these string conversions.

## Migration boundaries

Migrate `Token::error` and generic attribution first, retaining rendered
message snapshots. Add an adapter from structured errors to the existing public
IDE `Diagnostic`; this avoids an immediate public API break.

Then migrate worker return values and operation/ownership errors. Preserve
source/program-order error selection, not completion-order selection. Separate
sendability of a diagnostic from sendability of arbitrary external errors;
`Demand` and any referenced type metadata must be checked before promising
`Send + Sync`.

Finally change recorder storage and deduplication to structured identity, and
remove text parsing for native compiler diagnostics. A narrow textual fallback
may remain for genuinely external errors, but must not guess a source location
from arbitrary message contents.

Deduplication needs a deliberate key: primary location, message and relevant
attribution distinguish repeated failures. Two different source sites with the
same wording must not collapse. Decide whether notes participate; test that
decision rather than relying on derived equality accidentally.

## Acceptance

Use `tests/test_lsp.rs`, `test_python_habit_diagnostics.rs`,
`test_callable_diagnostics.rs`, `test_storage_diagnostics.rs`,
`test_recursive_tail_errors.rs`, and generic/borrowing regressions.
Add focused tests for:

- missing source positions, multiline messages, colons/spaces in paths,
  Windows drive prefixes and Unicode source;
- identical wording at distinct sites and true duplicate recovery;
- generic definition/use attribution and ownership demand inspection;
- the same diagnostic at one and many workers;
- formatting changes that cannot alter the editor location;
- LSP position encodings and related locations, if exposed.

The refactor succeeds when internal text reparsing is removed while ordinary
CLI text, attribution and diagnostic order remain unchanged unless an explicit
wording change is separately approved. No speedup is claimed.
