# Structured compiler diagnostics

[Parent proposal](../compiler-refactoring.md). The diagnostic contract here is
proposed; existing CLI spelling remains the compatibility baseline.

## Current conversion chain

`src/frontend.rs` defines `Located` with a location string, message and
optional ownership `Demand`. `Attributed` marks a generic diagnostic already
attributed to its proper instance, but holds only rendered text. `Token::error`
constructs located errors. `sendable` preserves those two concrete types across
worker boundaries and renders other boxed errors to strings.

`src/op.rs::check_with` converts function-check errors with `to_string()`
to collect parallel results, then selects the first error in program order.
Ownership checking builds richer messages from origins and demand information
in `src/ownership.rs`, but returns them as text.

In `src/frontend/ide.rs`, `recover` stores deduplicated strings in
`Recorder.diagnostics`. During `finish`, `diagnostic` searches the first
line for a `source:line:column: ` prefix and parses it back into a public IDE
`Diagnostic`, which already exposes file, line, column and message.
`src/lsp.rs::check` converts these into client diagnostics.

The problem is loss of structure between producers and consumers, not the
absence of a public location structure. Human-readable text is functioning as
an internal interchange format, and the reparse can misplace a message whose
text happens to contain `name:1:2: `.

## Recommended representation

Introduce one internal diagnostic type, outside the IDE module, carrying:

- the source path, line and column, each optional: an error with no source
  position (I/O, linking, a legacy-syntax error) has none;
- the message text;
- whether generic attribution has already happened (today's `Attributed`);
- the ownership `Demand`, where callers currently inspect it.

Do not record an end range: tokens record a start position, not an
expression's extent, and a fabricated span is worse than none. Related
locations, diagnostic codes and fix suggestions wait for a consumer.

Display stays `path:line:column: message`, so CLI text and existing test
snapshots are unchanged. Keep source positions in the compiler's coordinate
convention and convert to negotiated UTF-8/UTF-16/UTF-32 units only in the LSP
adapter. `Ty`, and therefore `Demand`, is already `Send + Sync`, so the type
can cross worker threads without conversion.

### Error-type alternatives

**Preferred incremental option:** the concrete diagnostic boxed inside the
existing `Box<dyn Error>` results, recovered by downcast at the boundaries
that need structure (worker join, editor recording). External failures stay
ordinary boxed errors without positions.

**Alternative:** a compiler-error enum distinguishing diagnostics, I/O and
backend/link failures. It gives exhaustive handling but means migrating every
`Result` signature in the compiler for no current consumer. Adopt only if
callers need those distinctions.

Do not add a diagnostic framework dependency to remove these string
conversions.

## Migration

1. Replace `Located` and `Attributed` with the new type; `Token::error` and
   `demand_error` construct it. Keep rendered messages identical.
2. Make the worker boundary in `check_with` and `sendable` keep the value
   instead of flattening it. Preserve program-order error selection.
3. Store structured diagnostics in the editor recorder and convert them to the
   public `Diagnostic` directly. An error that is not a compiler diagnostic
   has no position; never guess one from its text.

Deduplication needs a deliberate key: position and message. Two different
source sites with the same wording must not collapse; the same error recovered
twice must. Test both.

## Acceptance

Use `tests/test_lsp.rs`, `test_python_habit_diagnostics.rs`,
`test_callable_diagnostics.rs`, `test_storage_diagnostics.rs`,
`test_recursive_tail_errors.rs`, and generic/borrowing regressions.
Add focused tests for:

- a message whose text contains a `path:line:column: ` lookalike;
- colons and spaces in paths;
- identical wording at distinct sites and true duplicate recovery;
- generic definition/use attribution and ownership demand inspection;
- the same diagnostic at one and many workers.

The refactor succeeds when internal text reparsing is removed for compiler
diagnostics while CLI text, attribution and diagnostic order remain unchanged.
No speedup is claimed.
