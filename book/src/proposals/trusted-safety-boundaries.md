# Stronger safety boundaries for native code and recovery

Status: **Proposed; not implemented.** This proposal strengthens enforcement
around existing trusted operations; it does not identify a confirmed compiler
or runtime defect. Current behavior is defined by the
[Rust comparison](../design/comparisons/rust.md#unsafe-code),
[C interoperability reference](../design/facilities/04-interop/index.md), and
[actor trap contract](../design/stdlib/actor.md#traps). Remaining scope and
priorities belong to B98 in the [backlog](../backlog.md).

The companion pages cover [foreign ownership and trust](trusted-safety-boundaries/native.md)
and [actor recovery and qualification](trusted-safety-boundaries/recovery.md).
Names and contract descriptions here are design sketches, not available syntax.

## Motivation

Microsoft FORGE's [Memory Safety Bugs in Rust Standard Library](https://forge-blog.github.io/posts/rust-std-memory-safety/)
describes three recurring sources of unsoundness: panic unwinding through
temporarily invalid state, trusting incorrect user implementations, and relying
on shared operating-system state. The article motivates examining Plenty's
boundaries; it does not establish that Plenty has the reported Rust defects.

Plenty excludes ordinary panic unwinding and many low-level extension points.
However, having no `unsafe` blocks does not mean every operation is checked.
A correctly declared raw load can receive a dangling pointer, and a correctly
declared release function can receive the same native resource twice. A binding
and its callers must currently uphold contracts that the compiler cannot prove.
Actor recovery adds a separate obligation: continuing after discarding a heap
must leave surviving state safe without the abandoned frames' destructors.

The aim is to reduce the source that needs manual safety reasoning, make that
source visible to application authors, and enforce more contracts through
ownership, lifetimes, effects, and checked representations.

## Safety guarantee

Adopt a single rule across ordinary checked Plenty APIs:

> Incorrect application logic, including inconsistent callback results and
> protocol implementations, must not cause undefined behavior or corrupt memory.

An API may return a documented error, trap, or produce an unspecified logical
result when its semantic requirements are violated. It must preserve valid
storage, initialization, ownership, and aliasing. It may not silently turn a
semantic requirement into a memory-safety precondition.

This guarantee assumes a correct compiler, runtime, and approved native
dependencies. Their defects remain defects to fix. Native contracts explicitly
delegated to trusted source or a foreign host remain manual obligations.
The proposal does not promise termination, deadlock freedom, bounded resource
use, or protection against arbitrary native code in the same address space.

## Current design and proposed enforcement

| Boundary | Current behavior | Proposed improvement |
| --- | --- | --- |
| Fatal errors | Ordinary traps terminate without language cleanup; runtime Rust panics abort | Preserve this behavior; qualify native shutdown independently of panic policy |
| Native access | Trusted declarations expose native calls and unchecked raw operations | Explicit trusted modules, checked public wrappers, and explicit delegation for unchecked exports |
| Foreign owners | Affine Plenty wrappers conventionally own copyable opaque pointers | Compiler-recognized foreign owners and borrowed handles; raw escape remains trusted |
| Actor traps | Selected traps discard the actor heap and continue without destructors | Separate certification for abandonment, surviving references, and callback crossings |
| Dependencies | Native bindings add manually trusted contracts | Transitive trust report and application-controlled admission policy |
| User callbacks | Specific APIs already preserve safety under inconsistent answers | State the guarantee uniformly and require native consumers to enforce it |

## Treat callback answers as data

Existing [list key sorting](../design/facilities/01-builtins/lists.md) keeps
every element under inconsistent keys and after a fallible key returns an
error. [Heap comparators](../design/stdlib/collections.md#limits) may give
unspecified ordering but cannot compromise memory safety. These are the model
for other APIs, including future user-defined ordering or iterator extensions.

A callback-provided count is not proof that storage of that size exists.
A returned range is not proof that it belongs to the original input. Repeated
calls need not return the same answer. Native implementations must retain the
specific value they validated, check results against the actual storage being
accessed, or use a checked type that ordinary code cannot forge.

For example, reserving from a producer's size estimate does not authorize an
unchecked write for every yielded element. Each write needs established
capacity. Likewise, replacing bytes in text must preserve UTF-8 through checked
construction or a representation that guarantees validity. An ordinary protocol
implementation cannot supply a trusted proof merely by naming its method
`length` or `valid`.

Allocation failure and `Result` propagation also leave functions early. Native
algorithms must preserve cleanup invariants on these paths even though panic
unwinding is absent. User destructors remain user code with reachable effects;
they are not automatically pure, allocation-free, or safe to abandon.

## Operating-system assumptions

Preserve the existing exclusion of running-process environment mutation from
[`std.env`](../design/stdlib/env.md#variables), and prefer child-specific
environment and directory configuration. Native hosts must still avoid
concurrent environment mutation while Plenty reads it. Process-wide signal,
locale, and shutdown behavior need explicit native contracts rather than an
assumption that borrow checking controls them.

No-unwind behavior alone does not prove safe termination. The runtime's
ordinary trap paths currently call Rust's `std::process::exit`; reviewing
termination must include native cleanup handlers, runtime dependencies, and
calls from worker threads. A low-level immediate process exit may reduce that
surface, but changing it requires a separate decision about output flushing,
foreign cleanup, platform behavior, and embedding. This proposal does not
silently replace the existing exit contract.

## Adoption and compatibility

Prefer trusted-module confinement and actor recovery certification as the
first enforcement decisions, with a trust report helping identify migration
scope. Foreign owner types can then replace repeated wrapper conventions.
This is a dependency recommendation, not a scheduled implementation sequence;
B98 owns scheduling and remaining work.

Exact module markers, unchecked export spelling, owner declarations, and
manifest fields need small prototypes before adoption. Existing native APIs
must be classified as checked wrappers or explicit trusted exports. A migration
diagnostic may precede rejection, but an opt-in strict policy must reject
violations rather than silently waive its guarantee.

Keep the reference, tutorial, generated interface contracts, and Plenty skill
aligned when behavior ships. This proposal alone changes no accepted program,
public standard-library signature, or runtime behavior.

## Alternatives and tradeoffs

Documentation alone preserves maximum flexibility but leaves caller obligations
easy to miss. Trusted modules add a review boundary without requiring per-call
blocks; a module can still contain a defective wrapper, so they identify rather
than prove the manually trusted source. Function-level unsafe markers could be
more precise but would introduce another source-language mechanism; assess them
only if module-level delegation becomes too coarse.

Removing raw access would simplify the model but prevent native protocols that
need explicit buffers and callbacks. The preferred direction retains a narrow
raw escape and gives normal clients checked owners and views. Running native
plugins in a separate process provides stronger fault containment, at the cost
of serialization, process management, and incompatible in-process ABIs. It is
an application architecture option rather than a guarantee of these changes.
