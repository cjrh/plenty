# View-class implementation and qualification

**Implemented in local PR #1254; accompanies the approved [view-class decision](../view-classes.md).**
This records acceptance boundaries and source evidence, not an additional
work queue. Issue #945 and B11 in the [backlog](../../backlog.md) track the work.

## Existing mechanisms and the new boundary

The approved proposal inspected local `main` `caf7c247`; implementation is
based on integrated `main` `9f69b5c1`. These are the integration boundaries:

| Source | Current mechanism | Required treatment |
| --- | --- | --- |
| `src/frontend/classes.rs` | Declaration field storage barriers, generated/explicit construction, field moves and method calls | Recognize direct borrowed fields, enforce initialization-only assignment and one origin, reject forbidden lifecycle hooks and capture the constructor result loan. |
| `src/op.rs` | `Ty::carries_loan`, mutable-loan predicates, storage barriers and inline layout | View classes must carry loans recursively through the permitted standard success wrapper, without making general classes storable references. |
| `src/frontend/references.rs` | Caller result loans, return origins, holder propagation and projection origins | Preserve the actual data origin and separate receiver-record dependencies; do not recover aggregate origin from an incidental final loan. |
| `src/frontend/generators.rs` | Borrowing frame argument/return tracking | Reuse the loan-holder precedent conceptually; reject view classes at suspension boundaries in this version. |
| `src/record.rs`, `src/codegen.rs` | Class fields and existing inline class operations | Store borrowed fields without retaining or dropping referents; preserve ordinary class relocation and owned-field cleanup. |
| `src/frontend/threads.rs` | Scoped worker argument and lifetime checks | Carry underlying source dependencies through the lexical join boundary and use borrowed start-failure handling. |

The borrowing generator reference documents a broader all-argument dependency
set. That cannot be copied mechanically: this proposal intentionally has one
constructor origin and strict receiver-result lifetimes. The returned-view
review and issue #974 also demonstrate why a passing consumer suite does not
establish sound origin propagation. Audit every new producer and consumer.

## Representation decision

Reuse [inline class layout](../../design/toolchain/02-architecture/values.md).
Ordinary one-field classes without cleanup use that field's representation.
View classes always use their existing inline record storage, 128-bit value
slot and private address relocation. Borrowed reference/view fields use their complete two-word
representation and no retain/release of their referents. Plain owned fields
retain their existing layout and destruction. Construction adds no heap
allocation; constructing an allocating ordinary field still has its normal
error contract. A view class does not create a stable C ABI.

Do not promise a multi-field view is passed wholly in registers. Existing
records may be copied into local or argument storage. A native call, move,
return, sum extraction and scoped worker transfer must relocate only private
record addresses; borrowed pointers remain pointed at the checked source.
A method-result pointer into the receiver's own state prevents that receiver
record moving. A forwarded view-class value may move its record when no such
receiver borrow remains live.

Use the existing inline sum layout for one `Option`/`Result` layer: its tag is
outside the complete class payload. If a one-field wrapper optimization would
instead steal a view length bit, disable that optimization for view classes;
never silently narrow a length or change the source's address. Size/depth
limits produce a diagnostic before code generation. Type facts, metadata,
cache identities, formatting and cleanup must agree with the chosen layout.

Bounds and arithmetic checks stay unchanged. A constructor's assertion is not
a new trusted fact, and no unchecked public operation is introduced.

## Review and test gates

During implementation use focused tests and small probes. After an initial
implementation, perform self-review and fix every blocking finding; repeat
until the final producer/consumer and omission audits have no blockers. Only
then run the full workspace suite with `just test`. A failed broad check leads
to focused fixes and another self-review before a broad rerun. This follows
the user's requested validation order.

Every rejection below needs a negative compile test in both Plenty
optimization modes, with a nearby accepted control where meaningful:

| Boundary | Required negative cases |
| --- | --- |
| Origin | Independent constructor origins; later-argument projection hidden behind an index/call; returning a local/owned-parameter origin; conditional wrong-origin branches. |
| Source stability | Move, drop, reassign and resize the actual source while a view, shared copy, wrapped payload or derived method result remains live; inline-text slot relocation. |
| Exclusive permission | Copy or `@copy` an exclusive view; competing source loans; derive `&mut` through shared `&View`; multiple or mixed borrowed fields; aliasing through a borrowed helper. |
| Fixed fields | Rebind or replace a borrowed field after initialization, assign twice, move it out, swap it, mutate through a helper, or leave it uninitialized on a success branch. |
| Strict receiver lifetime | Use a returned element/view after moving, dropping or reassigning the receiver; mutate the receiver while a result lives; return a pointer into a consumed receiver. |
| Storage and erasure | Every forbidden collection, tuple, class, box, shared handle, channel, actor, user enum and callback environment, also through generic aliases/wrappers. |
| Sums and cleanup | Borrow in `Err`; nested success wrappers; borrowing a sum holder; lifecycle hooks; partial initialization and copied/dropped payload cleanup errors. |
| Workers | Two tasks using one exclusive view; parent source mutation through task scope; borrowed result or non-scoped transfer; start failure attempting an owning job payload. |
| Temporaries and suspension | Bound/returned temporary owner or receiver borrow; frame parameter/local live across yield; yielded view-class item; unsupported pipeline adapter result. |

Positive tests cover generic shared/exclusive cursors, ordinary owned fields,
constructor/default argument order, field projection, same-origin forwarding,
shared `@copy` holders and last-use release, by-value affine moves, same-origin
holder reassignment, fallible construction and match/propagate/unwrap, mutable
method reborrows, scoped thread inputs and start failure. Tests must establish
that failed construction and discarded successful payloads drop owned fields
exactly once without retaining or destroying the referent.

Run focused suites with runtime/allocation checks where applicable. Soundness
is established by compile-time rejection; runtime-checks coverage exercises the
accepted controls and verifies rejected resize/move programs cannot become
executable in either mode. Do not execute a suspected dangling-access program
as a substitute for the negative test.

Inspect emitted operations/layout for record moves, large plain fields,
one-field view wrappers, maximum-range view lengths, empty/subview values,
field/element origins and inline text. A full compiler/runtime suite, Clippy,
formatting, executable reference/tutorial checks and complete docs CI are final
handoff gates. Record actual counts and failures; a source-link check alone
is not the complete published-site build.

## Documentation and dependent features

The implementation updates class/ownership/returned-reference references,
optional/fallible view-class rules, layout and worker boundaries. Add a small
runnable cursor lesson using taught features and revisit packet readers that
currently pass position separately. Teach the simple named-local pattern
before its optional/fallible variants. Update the cheat sheet and Plenty skill
limits, narrow B11 to actual remaining scope and record the delivered subset.

Issue #950 separately delivers builtin window/chunk loop lowering; it does
not require the user iterator protocol.
Issue #945 step 3 chooses a user `next` protocol and borrowed-item lifetime;
this decision does not add it. SIMD view helpers can become classes only after
this representation is implemented and measured. Existing free helpers remain
supported; no performance or check-elimination result is claimed by this
change.

## Implemented checker boundaries

View classes are recognized from direct borrowed fields; no new public type
constructor or runtime borrow flag is added. Parameter referents reuse the
carried-loan mechanism. Constructor field writes prove the first non-self
origin before writing an uninitialized descriptor. The generated constructor
attaches that checked origin only after initialization succeeds. Initialization
validation intersects definitely initialized fields and unions possibly
initialized fields at continuing branch joins. It rejects a second borrowed
field write after any possibly initializing branch, including writes hidden
inside checked/match value blocks, while preserving normal exactly-once
branch alternatives. Repeated loop contexts reject borrowed-field writes.

Borrowing a named view record retains its carried source loan as a dependency.
Borrowed field projections load the stored descriptor and reborrow its content;
they never expose a writable descriptor slot. Consuming parameters distinguish
the external data origin from addresses into the record. Returned method
results retain the whole receiver, even when existing return-field inference
could narrow an ordinary class method's loan.

Nominal definitions are checked again after forward declarations finalize, so
user enums and nested owners cannot hide view-class fields. Standard success
wrapper loan queries use the completed payload rather than only an early
restricted-storage flag. Named-argument wrappers preserve source parameter
order for origin and permission selection while evaluating arguments in their
written order; this applies to concrete generic specializations too. Appended
hidden lent-pipeline parameters follow the declared parameters rather than
indexing beyond a source wrapper's permutation.

Whole-record replacement through a reference is rejected because it could
change fixed borrowed fields. Reassigning a named holder retains the existing
same-source rule. Generator suspension checks reject live view-class holders
even when their source is a caller referent that ordinary builtin views could
keep across a yield. Pipeline map outputs and C exports reject view classes
explicitly. Owned fields continue through the existing class cleanup path.

## Accepted constructor qualification

The implemented fieldwise constructor supports one borrowed field. Several
shared fields require an explicit initializer taking one borrowed parent;
its body derives every field from that parent. Distinct formal fieldwise
inputs remain independent origins even when a particular caller supplies
same-source projections. The user accepted the explicit one-parent initializer
as the step-2 contract in issue #945 comment #3037. No caller-alias equivalence
proof is implemented or required by that accepted contract. The separate
step-3 iterator protocol remains outstanding.
