# Ownership and reclamation proposal

Status: historical proposal for the first value-runtime implementation. Its
implicit-copy collection semantics have been superseded by
[references and explicit copying](references-and-copying.md). See `DESIGN.md`
for the implemented move semantics, local borrow checker, and cleanup rules.

## Decisions

Keep the user's independent-value collection semantics. Assignment, argument
passing, indexing, matching, and returning ordinary values produce independent
values, with immutable storage shared internally. Copying a handle is cheap;
observable mutation still replaces one binding's value. Do not retroactively make
`b = a` consume a list or make subsequent changes to `b` affect `a`.

There is one public `str`: immutable, owning, valid UTF-8 with explicit byte and
Unicode-scalar lengths. References, when eventually introduced, are ordinary
`&str` or `&mut str` reference types, not distinct owning/view string families.
An exclusive reference to a string binding permits replacement; it does not
expose mutable UTF-8 bytes. No string pointer relies on a terminating zero.

Generators differ from collection values: they contain an advancing execution
position and are affine, meaning they may be moved or dropped but not implicitly
copied. Assignment and by-value calls transfer a generator. Iteration transfers
it into a compiler-owned iterator local. Strings and current collections remain
copyable even after generators arrive.

Automatic reference counting is an implementation technique for immutable value
storage, not shared mutable source semantics and not a borrow checker. Public
references, resource-bearing structs, general partial moves, returned borrows,
and a Polonius-style checker are separate milestones. The initial owned-only
language can reclaim its allocations without waiting for those features.

## Source ownership categories

| Type | Assignment / argument / result behavior | Destruction |
| --- | --- | --- |
| Sized integers, bool, unit | Copy bits | No action |
| `str` | Independent immutable value; retain storage | Release storage |
| Current list, dict, set, range | Independent value; retain immutable storage | Release storage and owned children |
| Concrete enum, `Option[T]`, `Result[T, E]` with value payloads | Independent value; retain immutable record | Release active payload fields |
| `Generator[T]` | Transfer ownership, invalidate source binding | Destroy suspended frame and live captures |

Start by rejecting generators anywhere inside collection element types or enum
payloads. Their implicit-copy and projection rules would otherwise accidentally
copy generator state or require partial-move machinery. Permit generator
parameters, results, locals, and internal frame captures if the generator
implementation supports these locations. Recursively validate this restriction,
including aliases, so `list[Option[Generator[i64]]]` cannot bypass it.

Longer term, an enum or struct is copyable exactly when every field is copyable;
an affine field makes the aggregate affine. This is a structural compiler rule,
not a user-defined trait. Supporting these aggregates requires explicit whole
aggregate moves and match consumption; do not silently apply that future rule
to unsupported payloads now.

`mut` controls replacement of a binding; it does not change copying rules.
Moving from an immutable affine binding is allowed. A moved immutable binding
cannot be reassigned; a mutable one can be initialized again. No user-facing
`clone`, `retain`, `free`, lifetime annotation, or raw pointer is needed for the
first slice.

## Common runtime object header

Use the parent-approved common header on the current 64-bit native target:

```c
typedef struct PlentyObject {
    uint64_t refs;
    void (*destroy)(void *);
} PlentyObject;
```

Expose runtime `plenty_retain(void *)` and `plenty_release(void *)` helpers.
Dynamic objects begin with one owned reference and a destructor callback.
Literal strings use `UINT64_MAX` references and a null destructor, making them
immortal without data-symbol callback relocations. The resulting string prefix
is 32 bytes: this header, byte length, scalar length, then bytes. This layout is
internal and is not the future FFI contract.

Retain/release of internal null handles are no-ops. Null is only the compiler's
empty ownership-slot marker; it is never a language value or an absent option.
Check retain overflow rather than wrapping into zero or the immortal sentinel.
Counts are non-atomic while the language exposes no concurrent execution.
Release invokes the destructor at zero; the destructor frees children and the
object itself. Do not invoke a destructor twice or free the object again after
its callback. This callback design lets scalar-only programs link the basic
runtime without linking the collection runtime.

Strings destroy their single allocation. Collections destroy their backing
buffers and release all occupied elements (and both dictionary keys and values).
Enums release only fields of their active variant. Generators release live
frame slots and free their frame without executing user code after a suspended
yield. Runtime type metadata must either be static/immortal or separately owned
and reclaimed: replacing the allocation arena while leaking a freshly parsed
type descriptor on every collection construction does not meet the goal.

Keep copy-on-update for collections initially. Uniqueness-based in-place updates
are an optimization after ownership is proven; builders already provide efficient
bulk construction. A collection clone retains each nested owned value. Growing
a builder transfers entries into the new buffer and frees the old buffer without
releasing transferred elements. Rehashing frees the old bucket array. Replacing
an existing dictionary value releases the displaced value; duplicate keys/set
elements must not leave retained arguments behind.

These immutable value graphs are acyclic under the initial source restrictions:
there are no references, mutable shared fields, closures, or self-containing
frames accessible through the language. Do not use that claim once those
features are introduced. Deep recursive values will also require bounded-stack
destruction, or a documented implementation bound, before unrestricted
recursive aggregate types are enabled.

## Ownership accounting in native lowering

Choose one invariant: each owned expression operand, live local, object field,
and frame capture accounts for exactly one reference or one affine owner.

| Operation | Ownership action |
| --- | --- |
| Produce a dynamic value | Return one owner |
| Load a copyable managed local | Retain, producing an expression owner |
| Move an affine local | Read handle and clear source ownership slot |
| Store a local | Evaluate RHS first; release old slot; transfer RHS owner |
| Discard expression / stack `drop` | Release operand |
| Stack `dup` | Retain copyable value; reject affine value |
| Ordinary user call | Transfer argument owners into callee parameter slots |
| Return | Preserve result owners, clean local slots, transfer results to caller |
| Scope or control-flow exit | Release each initialized slot leaving its scope |

For simplicity, runtime value helpers *borrow their arguments for the call* and
return an owned result when their result is managed. Native lowering releases
the consumed expression operands after the helper returns. Constructors and
collection insertions retain any borrowed children stored in their result.
Projections (`a[i]`, enum payload bindings) retain a child before the parent can
be released. Equality, hashing, printing, and length helpers only borrow.
Document every exception to this contract in the helper declaration.

The compiler-private builder insertion currently returns its builder. If this
ABI is retained, returning that same pointer must retain it to satisfy the owned
result contract; lowering then releases the input owner. Alternatively give
builder insertion a separate, explicitly consuming ABI. Mixing these two
conventions creates a use-after-free that ordinary literal tests may miss.

Initialize managed local ownership slots to internal null, including locals
declared in conditional paths. This permits guarded cleanup after joins without
reading an uninitialized Cranelift variable. Clear the slot after moving or
dropping. A separate boolean live flag is equivalent; frame layouts need the
same information. Ordinary scalar definite initialization remains a static
frontend requirement.

The easiest reclamation checkpoint drops all live managed locals on every
function exit and releases overwritten locals immediately. This removes the
process-lifetime arena and bounds dead local retention by a function's finite
slots, but it is not the final lexical drop contract. Before resource handles
are exposed, emit scope cleanup for normal block exits, `break`, `continue`,
early return, and completed loop iterations. Source-visible destruction should
be reverse declaration order within each exited scope. Evaluate a return value
or replacement RHS before dropping values it may read.

Match dispatch owns its scrutinee until comparison and payload extraction have
finished. Each selected arm gets retained payload bindings, then releases the
scrutinee owner. A branch join transfers the owners from its one executed arm;
it must not retain every syntactic arm or release owners from unexecuted paths.
The existing Cranelift SSA joins can carry ownership slots, but ownership
decisions must be made before erasing source places and scope boundaries.

## Initial affine checking without a full source CFG

A structured, conservative checker is sufficient for owned generators while
references remain absent. Give each resolved local a stable ID and track
`Available`, `Moved`, or `MaybeMoved`. A read/move requires `Available`; a store
initializes a mutable slot. Source name shadowing creates a different local ID.
Function parameters start available, generator constructor calls consume affine
arguments immediately into the new frame, and function contracts decide argument
ownership without inspecting callee bodies.

Branches are checked from the same incoming state. Join only paths continuing
to that join: identical states stay identical; conflicting states become
`MaybeMoved`. A branch that returns is excluded. `MaybeMoved` reads are rejected
until a valid reinitialization. This is definite ownership, not last-use borrow
analysis.

For a simple sound loop rule, preserve the loop-entry availability invariant on
every reachable backedge. A local available before the loop must be available on
each normal body exit and `continue`; moving then reinitializing it is allowed.
Consuming it and reaching a backedge without reinitialization is rejected.
Consuming it on a path that unconditionally breaks or returns is allowed.
Join recorded break states with the zero-iteration state at loop exit, because
the current language does not prove that `while True` must execute. Locals
created inside a loop body are destroyed before its backedge and do not belong
to the entry invariant. Nested loops maintain separate break/continue records.

This conservative rule avoids a fixed-point analysis initially and rejects some
programs that a richer CFG analysis could accept. State that restriction in the
diagnostic. Do not merely mark a generator moved after one lowering pass through
a loop; subsequent iterations would otherwise read a consumed handle.

`for x in generator` moves the generator once into an internal loop slot before
the loop starts. Resume temporarily borrows that internal owner; it does not
move it on every iteration. `break` drops the internal owner, `continue` retains
it for the next resume, and return drops it while exiting the function. Iterating
the original binding again is a use-after-move error. Collection and string
iteration continues to use independent immutable snapshots.

## Tail calls and generators

A tail call first evaluates and owns all arguments, then drops the caller's
remaining owned locals and temporaries, then emits `return_call`. Arguments
survive because they have already been retained or moved out of local slots.
In the owned-only slice this preserves existing direct/mutual tail calls even
when strings and collections are live. Once borrowed parameters exist, a tail
call that passes a reference into the caller's frame cannot replace that frame.

A generator constructor creates a frame that owns its arguments without
executing its body. Resume borrows the frame exclusively, executes to yield or
completion, and yields a fresh owned value. For value types a yielded local is
copied/retained, leaving the frame's local intact. Start by disallowing affine
yield types. Saved local ownership slots survive suspension. Completion releases
all live captures/locals and records the completed state; later resume returns
the empty option without releasing them again.

Dropping an unstarted, suspended, or completed generator must all work. Dropping
after `break` releases captures but must not resume code after the last yield.
No public reference may be stored across a yield in the initial design. No
automatic frame cloning, shared mutable generator state, exception unwinding,
or implicit finalizer execution is part of this contract.

## Integration points

- `src/op.rs`: explicit copy-versus-move local reads, cleanup operations or scope
  metadata, initialized ownership slots, managed/copyable type classification,
  and affine control-flow verification. Keep legacy `Dup`, `Drop`, `Clear`,
  stack printing, and string match semantics correct under reclamation too.
- `src/frontend.rs`: retain lexical scope/local IDs during lowering, classify
  affine loads, validate definite ownership, and attach cleanup to all exits.
  `finish_return` must preserve result/argument owners while arranging cleanup.
- `src/frontend/collections.rs`: scope the hidden iterator, index, builder, and
  comprehension locals; preserve single evaluation and snapshot semantics.
- `src/codegen.rs`: runtime retain/release declarations, managed-local
  initialization, operand consumption, both explicit and implicit returns,
  branch scrutinee cleanup, user-call ABI, and pre-tail-call cleanup.
- `src/codegen/collections.rs`: borrowed-runtime-input/owned-result convention,
  projection retention, and loop exits cleaning the appropriate hidden locals.
- `runtime/plenty_runtime.c` plus shared runtime header: object header and
  retain/release, string destructors, and length-based string operations.
- `runtime/collections.c`: remove allocation-record arena; free grown buffers;
  retain/release nested entries and type metadata on all update paths.
- Enum and generator modules: concrete destruction metadata and frame/variant
  live-state handling; share the common header rather than creating competing
  ownership schemes.

## Implementation sequence and gates

1. Land common header and length-based strings with retain/release helpers;
   literals remain immortal. Establish helper ownership contracts.
2. Add owned operand accounting and local/function cleanup to native lowering.
   Convert collections/enums completely, including metadata and old buffers.
   Partial adoption may retain the arena temporarily, but must never release an
   object still referenced by an unconverted code path.
3. Preserve lexical cleanup scopes and add the conservative affine checker.
4. Integrate move-only generator frames and verify every suspension/exit state.
5. Introduce a typed source CFG and local borrow subset before references. The
   CFG needs stable places, source spans, edge cleanup, moves, reborrows, loan
   origins, and definite initialization. More precise Polonius-style relationships
   are an optional later improvement, not a prerequisite for immutable ARC values.

The first release should include native memory tests, not just correct printed
output. Use sanitizers when available and test-only allocation/drop counters to
establish balanced dynamic allocation after completed scopes/functions. Literal
and static type metadata should be excluded from dynamic live-byte counts.

| Area | Required checks |
| --- | --- |
| Strings | Literal copies, concatenation, embedded zero, returned local, argument reused after call, repeated replacement, indexing temporary |
| Collections | Nested copies survive source drop, dictionary replacement, duplicate set insertion, growth/rehash frees old buffers, projections from temporaries, snapshot iteration |
| Enums | Each active variant, multiple managed fields, payload projection survives parent drop, nested Option/Result, unchosen branch allocates/drops nothing |
| Control flow | Zero iterations, many iterations with bounded live bytes, nested break/continue, early returns, mixed terminating arms, scoped shadowing |
| Tail calls | Large direct/mutual recursion with owned arguments; captured value survives caller cleanup; no per-tail-call live allocation growth |
| Affine checks | Use after assignment/call/iteration move, one-branch move, terminated branch exclusion, move then reinitialize, move reaching backedge, moved break path joined with zero iterations |
| Generator destruction | Unstarted, each yield state, early break, return from consumer, exhaustion, repeated exhausted resume, nested generator capture |
| Legacy backend | `dup`/`drop`/`clear`, printing without consumption, string equality/matching, ordinary and tail-call string parameters |

Runtime traps currently terminate the process. They need not unwind source
scopes in this first contract; process termination reclaims address space.
Normal completion, early control-flow exits, and abandoned generators must use
the deterministic paths above. No claim of a general memory-safe borrow checker
should appear in the tutorial until public-reference analysis exists.
