# Boxed protocol qualification criteria

**Implementation note:** the [current reference](../../../design/language/07-generics/boxed-protocols.md) describes delivered behavior. This page preserves the original design and qualification plan; research-era implementation statements below are historical.
These are acceptance criteria for the [boxed protocol design](../boxed-protocols.md),
not a second work queue or a claim of implementation. B29 in the
[backlog](../../../backlog.md) owns scheduling. The
[research evidence](evidence.md) records what was actually run.

## Suggested implementation sequence

An internal spike can start with one shared receiver method and two concrete
payload layouts. Prove construction, list iteration, indirect calls and correct
drop before generalizing to several methods. Reuse descriptor-driven storage
and generated adapters rather than building a second owner runtime.

Then implement eligibility, parameterized protocol identity, exact conformance,
generic-bound integration and mutable receiver loans. A shared-only spike is
useful evidence but does not meet the proposed shared-and-mutable contract.
Complete type walkers, effects, visibility, diagnostics and tooling before
presenting the feature as supported.

Keep these stages inside one agreed implementation scope or explicitly narrow
the public contract before shipping a subset. Do not silently omit a method
from the table because its ownership handling is unfinished.

## Required behavior and regression coverage

| Area | Acceptance evidence |
| --- | --- |
| Heterogeneous storage | A class and an enum with different layouts share a list, dictionary values, fields, enum payloads and factory result type. Borrowed iteration calls the correct implementation without matching. |
| Shared state | At least two methods, including a mutable method, operate on one instance. Repeated calls and moving the box preserve that state. Shared references cannot call mutable methods. |
| Generic use | `Box[P]` works through `T: P`, fixed protocol arguments infer correctly, aliases preserve identity, and extra concrete methods remain inaccessible. |
| ABI | Scalar/unit/inline aggregate results; owned and reference/view arguments; fallible results; receiver modes; parameter count limits and argument evaluation order. |
| Loans | Nested projections, mutable list iteration, dictionary `get_mut`, temporaries, reentrant argument evaluation, and rejection of moves/removals/replacement during live loans. |
| Lifetime and destruction | Collection growth, reorder, extraction, replacement, early return and ordinary drop destroy each concrete payload exactly once, including custom destructors and nested owners. |
| Allocation failure | Inject failure at box construction and collection growth/insertion; verify the existing consumed-input contract, no leaks/double drops and valid remaining owners. |
| Eligibility diagnostics | Reject generic requirements, consuming receivers, all returned-borrow forms, and implementing-type occurrences outside the receiver, including through aliases and nested types. Existing static generic uses remain valid. |
| Privacy | Cross-module public protocol/private implementation, inaccessible required methods, hidden signature types, imported aliases, and no capability gained from adapter emission context. |
| Unsupported operations | Reject bare protocol values, implicit erasure, boxed-input construction, extraction, copying/equality/hashing, keys/set elements, downcasts and protocol conversions. Nested aggregate operations obey the same restrictions. |
| Capabilities and effects | Reject uncertified worker/channel transfer at every nesting level. Audit `Shared` cycles, hidden executor/future ownership, indirect blocking effects, locks and actor/task handoff. |
| Artifacts and tooling | Retain every adapter and destructor through pruning/linking; correct IR dumps, diagnostics/navigation, deterministic table layout and cache invalidation after protocol or implementation changes. |

Independent IR tests should reject malformed protocol calls even if normal
frontend lowering would not emit them. Runtime tests should protect the owner
header, table address, payload relocation and destruction invariants, mirroring
the concrete descriptor tests for boxed callables.

## Performance questions

Compare equivalent enum-wrapper, concrete boxed, boxed callback and boxed
protocol workloads. Keep payloads, allocation counts and method work comparable;
an inline enum versus one allocation per object measures storage as well as
dispatch. Separate construction, iteration, mutation and destruction costs.

Measure payload-size variation, homogeneous versus mixed implementations,
method count and number of concrete implementations. Record native code/table
size and compilation time as well as runtime and allocation counts. Verify
that moves, method dispatch and compiler-provided cleanup introduce no extra
allocations. Do not promise a precise instruction count or overhead before
measurement; optimizers may remove some indirect calls in particular programs.

## Documentation and release qualification

On implementation, update protocol and box references, ownership/returned-borrow
limits, generic conformance, architecture/support and runtime representation
notes. Update broad statements such as “no dynamic dispatch” to distinguish
static generic calls from explicitly erased owners. Teach runnable list and
dictionary-value examples after classes/enums, retaining enum wrappers as the
clear finite-alternative option. No new heterogeneous key/set lesson is implied.

Use the repository's required formatting/lint and compiler/runtime test commands,
including allocation-checks coverage; test executable lessons and all docs CI
checks. Qualify native object relocations and calls on supported targets before
claiming portability. Keep proposal status, delivery records and B29's remaining
scope aligned with the exact subset that ships.

## Decisions to revisit only with evidence

The recommended initial contract makes concrete choices: explicit `Box[P]`,
shared/mutable methods, no consuming/returned-borrow methods, and no certified
thread transfer. Implementation may uncover reasons to revise those choices;
record such revisions before relaxing a safety boundary.

The exact internal layout, table-sharing cache and diagnostic wording remain
implementation choices. Bare borrowed protocol views, allocation-free erasure
of existing boxes, cross-protocol conversion, method-generic dispatch,
downcasting, copying and erased error reporting are outside this scope. Their
absence does not block iteration and ordinary protocol method calls.
