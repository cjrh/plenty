# Frontend and session boundaries

[Parent proposal](../compiler-refactoring.md). All structures described as
proposed are design sketches, not implemented APIs.

## Current source map

| File / symbol | Responsibility and coupling |
| --- | --- |
| `src/frontend.rs`: `Lexer`, `Parser`, AST types | Streaming item tokenization and syntax construction share the root module with lowering and diagnostics. At the reviewed revision the file is about 7,400 lines. |
| `src/frontend.rs`: `Lower` | Holds function names/slots, mutable function and generic tables, heap, immutable aliases/access, loans, captures, pipeline state, scopes and source provenance. |
| `lower`, `lower_function`, `lower_body` | Coordinate declaration expansion, registration, on-demand specialization, body lowering and finalization. |
| `src/frontend/functions.rs`: `Functions`, `Symbol` | Existing indexed function table: signatures, declarations and returned-field facts. `complete` releases eligible syntax bodies. |
| `src/frontend/generics.rs`: `Engine` | Instance preparation and pending/compiled work interact with ordinary lowering. |
| `src/frontend/references.rs`, `provenance.rs`, `retention.rs` | Borrow origins and lifetime-related facts used during expression lowering. |
| `src/frontend/pipelines.rs` and its children | Pipeline plans, callbacks, generator adaptation and specialized lowering. |
| `src/frontend/ide.rs` | Ambient per-thread recording and error recovery alter analysis behavior. |

File length is evidence of navigation cost, not a reason by itself to rewrite
the frontend. Child modules already exist; adding files without changing
dependencies would not address the main concern.

## Recommended decomposition

Separate state by lifetime and responsibility, not by matching every syntax
variant with a trait:

1. **Compilation environment:** aliases, access policy, source identity and
   shared function/type tables. Distinguish read-only inputs from mutable
   specialization/string services. Do not place every service behind a mutex
   just to make the context shareable.
2. **Function state:** current function identity, parameters, local slots,
   bindings, return inference and scope stacks. Preserve the different meanings
   of module scope, unit return and a value return; `Option<Type>` currently
   encodes those distinctions.
3. **Borrow/provenance state:** loans, reference-local associations, return
   origins, value origins and slot names. Expose operations that update related
   facts together rather than public mutable field collections.
4. **Pipeline state:** plans, resumed/end flags, lent parameters, callback
   attribution and consumption state. Give this state explicit entry/exit
   operations before moving pipeline lowering methods.

A suitable first extraction is passive pipeline state: group its owned fields
and initialization while retaining methods on `Lower`. Establish reset and
scope invariants in tests, then move operations only when their actual
dependencies are understood. Borrow/provenance state is another possible
starting point, but is more correctness-sensitive.

The goal is that a reviewer can identify which service a lowering operation
may mutate. Avoid a giant context with unrestricted mutable access, cosmetic
substructures whose callers still mutate every field, or an object-safe
visitor/trait framework without multiple real implementations.

Keep independent `op::check` validation. A future validated-program wrapper
could make backend entry requirements explicit, but must account for passes
that transform operations after checking. A type name alone does not prove
that transformed IR remains valid.

## Ambient state and lifetime policy

Three mechanisms deserve separate treatment:

- `src/frontend.rs`: process-global `FILES: RwLock<Files>` maps paths to
  compact `FileId`s. It intentionally avoids atomic path clones per token.
  It retains each distinct path for the process lifetime.
- `src/frontend/ide.rs`: thread-local `ACTIVE` and `RECORDER`; `analyze`
  enables recording, catches unwinds, disables recording and takes the result.
  This supports ordinary calls, but nested analysis on the same thread would
  need explicit save/restore semantics.
- `src/lib.rs`: public `skip_teardown()` sets a process-global atomic
  policy. `src/compiler_cli.rs` uses it for one-shot checks; project, LSP
  and library operations are documented to retain normal destruction.

These are design constraints, not newly reproduced leaks or crashes.

### Options

**Smallest change:** retain the file interner; introduce scoped IDE recorder
installation and invocation-specific teardown policy. Ensure guards restore
previous state on unwind. Reject nested recording clearly if nesting is not a
supported operation. This avoids introducing a full public session API.

**Broader change:** a compilation session owns source identities and analysis
services, with per-invocation options. Default library APIs create a normal,
fully reclaimed session; the LSP can intentionally reuse one. Token IDs remain
compact and every escaped analysis/artifact retains or translates its source
identity.

Prefer the smallest change unless bounded source retention or session reuse
has a measured consumer. A session is not automatically a cache, a scheduler,
or a requirement to make lowering concurrent.

### Required invariants

No file ID may resolve through the wrong session or outlive its source table.
Do not recycle IDs while retained diagnostics, ASTs or operations refer to them.
Keep cached artifact identities independent of process-local file numbers.
Distinguish source-path interning from retaining source contents.

Teardown skipping must remain an explicit one-shot CLI optimization, never a
default for embedding. Changing the public helper requires a compatibility
decision and documentation of migration; replacing an atomic with an ordinary
flag is not sufficient.

Test repeated success/failure in one process, independent analyses on multiple
threads, unwind restoration and the chosen nested-analysis policy. Measure
retained paths/allocations separately from allocator RSS, which may remain high
after correct destruction.
