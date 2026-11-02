# Frontend and session boundaries

[Parent proposal](../compiler-refactoring.md). The syntax/IR split, pipeline
state and scoped ambient state are implemented (issues #692–#695); the
session and further lowering-state options remain design sketches.

## Source map

The table records the layout these steps started from, at `6477751e`.

| File / symbol | Responsibility and coupling |
| --- | --- |
| `src/frontend.rs`: `Token`, `FileId`, `Lexer` | Tokens, file interning and streaming item tokenization, about 400 lines at the root of the frontend. |
| `src/frontend.rs`: `Function`, `TypeRef`, `Stmt`, `Expr` | The syntax tree, plus `TypeRef::resolve`, which turns written types into `Ty` against aliases and so belongs to resolution rather than syntax. |
| `src/frontend.rs`: `Parser` | Core statement/expression parsing, about 1,650 lines. Feature modules (`classes.rs`, `enums.rs`, `patterns.rs`, `collections.rs`, `fstrings.rs`, `protocols.rs`, `annotations.rs`) add their own `impl Parser` blocks beside their `impl Lower` blocks. |
| `src/frontend.rs`: `Lower` | Holds function names/slots, mutable function and generic tables, heap, immutable aliases/access, loans, captures, pipeline state, scopes and source provenance: about 50 fields. |
| `lower`, `lower_function`, `lower_body` | Coordinate declaration expansion, registration, on-demand specialization, body lowering and finalization. |
| `src/frontend/functions.rs`: `Functions`, `Symbol` | Indexed function table: signatures, declarations and returned-field facts. `complete` releases eligible syntax bodies. |
| `src/frontend/generics.rs`: `Engine` | Instance preparation and pending/compiled work interact with ordinary lowering. |
| `src/frontend/references.rs`, `provenance.rs`, `retention.rs` | Borrow origins and lifetime-related facts used during expression lowering. |
| `src/frontend/pipelines.rs` and its children | Pipeline plans, callbacks, generator adaptation and specialized lowering. |
| `src/frontend/ide.rs` | Ambient per-thread recording and error recovery alter analysis behavior. |
| `src/op.rs` | The operation IR (about 1,350 lines), the historical stack-syntax compiler (about 600 lines, with `src/lexer.rs`) and the independent checker (about 1,250 lines). |

At `6477751e`, `src/frontend.rs` was about 7,650 lines. After the moves it is
about 4,950, with `frontend/lexer.rs`, `syntax.rs` and `parser.rs` beside it;
`src/op.rs` keeps the IR (about 1,500 lines), with `op/check.rs` and
`op/legacy.rs` (which now holds the stack-syntax lexer). File length is evidence
of navigation cost, not a reason by itself to rewrite the frontend. A move is
worthwhile where it turns an existing independence into one the compiler
enforces; adding files without changing dependencies would not address the main
concern.

## Syntax and IR files

The core parser does not read `Lower`, the heap, the generic engine or module
tables. Move tokens and the lexer, the syntax tree, and the core parser into
child modules of the frontend (for example `frontend/lexer.rs`,
`frontend/syntax.rs` and `frontend/parser.rs`). Keep the per-feature
`impl Parser` blocks where they are: grouping a feature's parsing and lowering
together is deliberate. `TypeRef::resolve` and the builtin-type tables stay
with resolution. Visibility then shows which syntax items lowering uses.

Split `src/op.rs` into the IR (`op.rs`), the checker (`op/check.rs`) and the
legacy stack-syntax compiler (`op/legacy.rs`, with `src/lexer.rs`). The checker
then depends on the IR's public surface only, which is the contract a second
backend would also consume. Removing the legacy syntax entirely (the CLI's
`--legacy` flag and the backend tests written in it) is a separate product
decision, not part of this split.

These are pure moves: no behavior change, and the diff should review as one.

## Lowering state

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

The first extraction, `frontend::pipelines::State`, is passive pipeline state. Its nine fields are read and
written almost only by `src/frontend/pipelines/` (plus three sites in
`references.rs`, `generators.rs` and the generator bridge), and every function
starts with them empty. Group them in one struct owned by the pipelines module,
with a `Default` initializer, while retaining methods on `Lower`. The struct's
documentation then says what the pipeline code owns, and `Lower`'s
construction loses nine lines. Move operations only when their actual
dependencies are understood. Borrow/provenance state is used throughout
expression lowering and is more correctness-sensitive; extract it only if the
pipeline step shows a clear reduction in coupling. Grouping pipeline state
documented ownership but did not change who calls what, so borrow/provenance
state was not extracted.

The goal is that a reviewer can identify which service a lowering operation
may mutate. Avoid a giant context with unrestricted mutable access, cosmetic
substructures whose callers still mutate every field, or a visitor/trait
framework without multiple real implementations.

Keep independent `op::check` validation. A future validated-program wrapper
could make backend entry requirements explicit, but must account for passes
that transform operations after checking. A type name alone does not prove
that transformed IR remains valid.

## Ambient state and lifetime policy

Three mechanisms deserved separate treatment; the recommended smallest change
below is implemented (issue #692). This list records the starting state:

- `src/frontend.rs` (now `frontend/lexer.rs`): process-global `FILES: RwLock<Files>` maps paths to
  compact `FileId`s. It intentionally avoids atomic path clones per token.
  It retains each distinct path for the process lifetime.
- `src/frontend/ide.rs`: thread-local `ACTIVE` and `RECORDER`; `analyze`
  enables recording, catches unwinds, disables recording and takes the result.
  A nested `analyze` on the same thread would overwrite the outer recorder and
  then switch recording off for the rest of the outer analysis.
- `src/lib.rs`: public `skip_teardown()` sets a process-global atomic.
  `src/compiler_cli.rs` calls it for one-shot checks; the language server,
  `plenty build`, `plenty test` and library callers must not.

These are design constraints, not reproduced leaks or crashes.

### Options

**Smallest change (recommended):** retain the file interner. Install the
recorder through a guard that saves the previous `ACTIVE`/`RECORDER` and
restores them on drop, so nesting and unwinding are both correct. Replace
`skip_teardown()` with a `CheckOptions` field set by the CLI; a library caller
then cannot change another caller's policy. This changes a public function, so
the PR must say so; the CLI is its only caller in this repository.

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
default for embedding.

Test nested analysis and unwind restoration of the recorder. Measure retained
paths/allocations separately from allocator RSS, which may remain high after
correct destruction.
