# Native async/await spike evidence

Companion to the **unadopted** [proposal](../async-await.md). Evidence collected
on 2026-10-22 against local `main` at `9c994af0`. This is a standalone compiler
experiment under [`experiments/async-await`](https://github.com/cjrh/plenty/blob/main/experiments/async-await/README.md),
not a modification to the Plenty parser, checker, code generator or runtime.

## What actually executes

A small Python program parses a restricted Python-shaped source grammar and
emits C state machines. Python supplies an AST, not coroutine execution. A C11
compiler produces a native executable; a C harness polls it, moves suspended
frames, injects allocation failures and checks cleanup. No Rust futures, pinning,
GC, async library, hidden thread or manually substituted source result is used.
The source really contains async functions and await expressions:

```text
async def leaf(value: i64) -> i64:
    resource = acquire()
    result = await tick(value)
    return result + 1

async def parent(value: i64) -> i64:
    resource = acquire()
    first = await leaf(value)
    second = await tick(first)
    return second + 1
```

This syntax is accepted **only by the spike**. It is neither ordinary Python
(the intrinsics and `i64` are experiment-specific) nor current Plenty. Numeric
error propagation is implicit in this small compiler; the proposed Plenty
`Result`/`?` syntax is not implemented. There is no I/O backend: `tick` returns
pending once and then its value. Real nonblocking I/O and registration lifetimes
remain a separate adoption gate.

Each await has an initialization state and a resume state. Initialization runs
once; pending returns preserve the child frame and all live locals. Ready
results flow to the next statement in the same poll. Cleanup flags distinguish
initialized resources/children from untouched fields; completion and error both
release them. A child frame contains no pointer to its parent. The harness moves
frames by transferring their bytes and invalidating the source, preserving
owned heap handles. C does not enforce ownership; the restricted generator and
trusted harness do. This is mechanical evidence, not a proof about arbitrary
Plenty programs.

## Reproduce and inspect

From the task/repository root:

```sh
python3 -B experiments/async-await/test_spike.py
SANITIZE=1 python3 -B experiments/async-await/test_spike.py
python3 -B experiments/async-await/lower.py experiments/async-await/program.async
```

The first two commands compile and execute temporary artifacts and report test
status. The third prints generated C for inspection. No dependencies beyond
Python 3 and a C11 compiler are required; `CC` selects the compiler. See the
[README](https://github.com/cjrh/plenty/blob/main/experiments/async-await/README.md) for the exact subset and
host/compiler allocation boundary.

Validation passed with the default C compiler and with Clang's address,
undefined-behavior and leak sanitizers. The default GCC sanitizer link failed
because its runtime shared libraries were unavailable; Clang supplied a working
alternative. LeakSanitizer required execution outside the sandbox's ptrace
wrapper. Both Python test methods passed, including twelve rejection subcases
and all native harness assertions. Book link checking and `mdbook build book`
also passed. The production compiler/runtime suite was not run: neither their
code nor executable tutorial examples changed.

## Observations and their limits

| Experiment | Observed result | What it supports |
| --- | --- | --- |
| Create/drop without polling | Zero allocation attempts and no body effects | Lazy inline construction for scalar captures |
| Nested `parent(40)` | Two suspensions, then `42`; two body resource allocations | Await composition needs no additional frame allocations |
| Move while parent and child are suspended | Resumes correctly; child then parent release exactly once | This restricted pointer-free frame layout can move after suspension |
| Drop before start / at each suspension | Zero surviving allocations; drop makes no allocation attempts | Synchronous cancellation works for the experiment's memory-only owners |
| Child failure | Error code `71` survives parent propagation; both resources released | Error and completion cleanup paths can share a state-machine structure |
| Fail allocation 1, 2 or 3 | Admission failure returns intact frame; parent/child failures release acquired owners | Allocation failure need not abort or leak in these paths |
| Fail admission of a suspended frame | Byte-identical source still owns both resources; retry then cancellation releases everything | Reservation-before-transfer works beyond unstarted frames |
| Two roots and a third submission | Both progress; third gets capacity failure without allocation or transfer | Bounded explicit scheduling can preserve failed-submission ownership |
| Close scope while pending and allocator disabled | All remaining resources/tasks released without allocating | A restricted affine scope can cancel synchronously |
| Twelve unsupported-source cases | Rejected before C compilation | Borrowed parameters, recursion, loops, async context managers, escapes/copies and invalid calls are not silently accepted |

On this x86-64 host, frame sizes were: leaf **56 bytes**, parent **128 bytes**,
failing leaf **56 bytes**, propagating parent **96 bytes**; two-slot scope
**96 bytes**. The successful scheduled parent performs three allocations: one
root frame and two explicit 16-byte body resources. Direct polling performs
only the latter two. The generated parent retains separate storage for its two
await sites instead of overlaying disjoint lifetimes. Frame-size reduction is
possible but not demonstrated. These figures are layout observations, **not**
throughput or memory comparisons with Rust, Python, threads or real Plenty.

## What changed our assessment

The basic state machine and failure rollback are small when every suspended
value is owned and destruction is synchronous. That makes a constrained
language extension plausible. The experiment also exposes why the restriction
matters: admitting even one child borrow into its parent would invalidate its
simple relocation test. A stable allocation, offset-based references with
careful external-pointer rules, or a pin-like invariant would then be necessary.

The substantial unanswered work is at the library boundary. A buffer-owning
read API must return owners on failure and cancellation; a reactor must never
retain a movable frame pointer; an in-flight native operation may outlive a
cancel request. None of those problems is exercised by a synthetic tick.
Likewise, memory-only drop cannot establish protocol completion. The result is
a reason to evaluate owned-operation ergonomics, not evidence that we have
eliminated the hard parts of asynchronous I/O.

Production work would touch parsing/AST, concrete coroutine identity and generic
specialization, suspension-aware loans, return/error lowering, partial
initialization and cleanup, native-effect certification, diagnostics/cache
identity, runtime wake/task storage, every exposed async I/O adapter and the
tutorial. Existing generator machinery may be reusable, but its absence of
body `?`, return values and nested await means this is not just a new keyword.
The spike omits branches, loops, arbitrary captures/results and user-defined
destructors; it estimates no calendar schedule or compiler performance.

Until a realistic owned I/O prototype and a comparison with current workers
justify those costs, leaving async/await out remains the conservative choice.
