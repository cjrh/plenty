# Native stackful-task spike evidence

Companion to the [Go-style task exploration](../stackful-tasks.md).
The isolated source lives in `experiments/stackful-tasks/` at the repository
root. It is a C experiment, not accepted Plenty syntax or production code.

## What actually executes

Linux/glibc `getcontext`/`makecontext`/`swapcontext` run two task call stacks on
one native thread. Each task owns a 64 KiB usable stack, surrounded by two
inaccessible pages, plus a separate heap task record. Ordinary C helper calls
perform channel waits and nested cooperative suspension. The scheduler returns
to its own stack between turns. No native thread is created; there is no GC,
async library, frame relocation, custom forced unwinding or language compiler.

The scope supports two lifetime task slots and one explicitly allocated channel
slot. It has preallocated task identities, implicit ready order by table scan,
and states for ready, send wait, receive wait and done. A send on a full channel
or receive on an empty channel parks, and communication wakes the opposite
waiters. This is a tiny test scheduler, with no slot reuse, endpoint closure,
selection or general-purpose channel API.
Messages are integers; affine-value transfer, failed-send ownership, endpoint
closure races and linear obligations are not tested.

Admission allocates a task record, maps the guarded stack, makes its usable
pages accessible and prepares its native context. The input pointer changes
ownership only after those operations succeed. The failure injector covers
queue allocation, task-record allocation and stack mapping; real `mmap`,
`mprotect` and context-setup failures are checked, but only allocation sites are
injected. Capacity failure allocates nothing and retains the job. Stack mapping
failure releases partial task storage, and the original job can be retried.

Cancellation marks a task and makes parked tasks ready. Channel operations
observe the request and return normally. The cleanup worker also yields from a
nested helper holding a reference to a caller's volatile stack local, mutates
that local on resumption, and checks the result before exiting. It explicitly
releases initialized heap resources in reverse order on success, cancellation
and body allocation failure. Destruction refuses to reclaim any unfinished
stack. These C cleanup paths model the candidate control flow; they do not
implement Plenty destructors, its borrow checker, linearity or Result lowering.

## Reproduce

From the task checkout, with a C11 compiler and Linux/glibc:

```sh
python3 -B experiments/stackful-tasks/test_spike.py
CC=clang SANITIZE=1 python3 -B experiments/stackful-tasks/test_spike.py
```

The runner creates a temporary executable, compiles with `-O2 -Wall -Wextra
-Werror -pedantic`, runs assertions with a timeout, and removes the executable.
`SANITIZE=1` adds UndefinedBehaviorSanitizer; it does not claim complete
alternate-stack instrumentation or AddressSanitizer coverage. The bounded
experiment is intended as a mechanically inspectable counterexample to needing
movable frames for every suspension, not a benchmark.

## Observations

Observed on x86-64 Linux/glibc:

| Check | Observed result |
| --- | --- |
| 1,000 values through capacity-one channel | Sum 500,500; 999 send parks and 999 receive parks |
| Reference to stack local across nested suspension | Same retained local is read/modified after resume |
| Task record / usable stack / full mapping | 1,016 bytes / 65,536 bytes / 73,728 bytes with 4 KiB pages |
| Queue / task-record / stack allocation failures | Failure returned; partial admission storage released; unstarted job retained and retried |
| Full task table | Job retained, allocation attempt count unchanged |
| Cancellation before body entry | Captured job dropped by scope; body never starts |
| Cancellation of parked sender and receiver | Task resumes and returns cancellation before scope reclamation |
| Success / cancel / either body allocation failure | Initialized resources released once in reverse order; tracked live allocations and mappings return to zero |
| No ready task | Step reports no progress; explicit cancellation wakes test waiters |

Two 64 KiB stacks are enough for these tiny call chains. This is not a safe
general default. At 10,000 tasks the demonstrated mapping size alone would
reserve about 703 MiB of virtual address space, excluding task records,
allocator metadata and other storage. This is arithmetic from the observed
mapping size, not a scalability run or resident-memory measurement. Successful
mapping does not guarantee later physical-memory availability. The guards are
installed, but no overflow-recovery experiment is claimed; overflowing or
skipping guards is outside the safe experiment contract.

The finite no-yield example returns without scheduling a peer; it exercises no
preemption mechanism. There is no real blocking-I/O demo, readiness backend,
fairness/latency measurement, syscall handoff, FFI migration, or scheduler
performance comparison. The source's explicit swap points already establish
that a blocked native call cannot yield by magic.

## What this changes in the design discussion

Actual stack retention makes an ordinary borrowed helper chain possible across
suspension without a public pinning type. Synchronous cancellation needs
resumption and return; stack retention does not provide safe arbitrary stack
destruction. The experiment consequently supports a narrow scoped library
candidate, while leaving the most consequential deployment questions open:
stack budgets and exhaustion, real I/O lifetime, transitive suspension checks,
thread eligibility, bounded cancellation and tooling portability.

The sibling async experiment avoids whole-stack reservations and demonstrates
moving restricted owned frames, while prohibiting surviving references. This
experiment retains references by making the stack immovable and paying for it.
The experiments expose different costs; they do not establish a winner.
