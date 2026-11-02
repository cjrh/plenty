# Actor stack-switching probe evidence

Companion to [Actor budgets and retained stacks](../actor-budget-scheduling.md).
Status: **Test-only native probe; compiler insertion and stage integration are
not implemented.** Source: `experiments/actor-stacks/probe.rs`, included inside
the runtime's actor heap module only in x86-64 GNU/Linux test builds.

## What executes

Two callbacks run on separate fixed 64 KiB stacks, with an inaccessible page
at each end. A small round-robin harness alternates them whenever an explicit
checkpoint exhausts a budget of 17. Checkpoints model intended compiler output;
no compiler pass inserts them. Each callback enters the actual
`plenty_actor_run`, so its production recovery record lives on its own stack.
The probe uses the real routing allocator, heap limits and unwrap trap helper.

The harness saves/restores allocation routing, recovery, caller-location and
last-trap TLS, plus the production drop queue. It refuses checkpoints during
arena calls or active destruction. The drop queue remains empty in these
fixtures; nonempty queue switching is not tested. Caller-location identities
are distinct and checked after resumption. Retained heap allocations are checked
for ownership and contents, and an oversized allocation returns null within
each actor's limit. A borrowed stack local survives nested helper suspension.

The harness uses stable raw-pointer records rather than keeping an exclusive
Rust reference to scheduler storage across a switch. It does not model a sound
replacement for Plenty's shared `Context`, compiler adapter storage, mailbox
delivery, readiness, timers, supervision or restart. It cannot establish the
borrow checker's acceptance of a new context design.

## Reproduce

From the task checkout:

```sh
cargo test --locked -p plenty-runtime actor_stack_switching_probe -- --ignored --nocapture --test-threads=1
```

The probe is ignored during ordinary suites because it owns the process-wide
actor heap table; run it alone. An external timeout is appropriate for detecting
switch failures. Its scheduler also asserts a finite maximum turn count.
The default fixture has finite loops; it does not execute an infinite loop.

## Checks and interpretation

| Case | Assertion |
| --- | --- |
| Budget disabled | Peer first runs after the first actor completes all 10,000 iterations |
| Budget enabled | Peer first runs after 17 iterations; each actor completes 10,000 iterations with 588 yields |
| Trap after resumption | First actor traps after three yields, returns `Unwrap`, runs no local destructor; peer completes and runs its destructor once |
| Cooperative cancellation | Scheduler requests cancellation after eight dispatches; both callbacks resume, observe it and return through cleanup before completing their loops |
| Allocation routing and limits | Small allocations belong to the running actor before/after switches; oversized allocation is refused |
| Reclamation | Each case discards both heaps and unmaps both stacks; live heap count, used bytes and committed heap bytes return to zero |

These are deterministic work/progress checks, not throughput or latency
benchmarks. The cancellation flag is an experiment-only mechanism, not
`Mail.Stop` or a new public cancellation API. The trap fixture deliberately
leaves its allocation and destructor behind; existing actor heap discard
reclaims that allocation without unwinding the stack. Runtime panics are not
recovered.

Observed on x86-64 GNU/Linux on 2026-11-02: all four cases passed in debug,
release and debug with `allocation-checks`. The budgeted pair performed 588
yields each; the trap case returned `Unwrap` at iteration 52 while its peer
completed 10,000 iterations. Cancellation returned both callbacks at iteration
68 with one destructor each. The oversized-allocation check calls `Router`
directly because an unused standard Rust allocation can be optimized away.

Validation also passed the ordinary runtime suite (187 passed, 5 ignored),
workspace/all-target Clippy with warnings denied, workspace formatting and an
explicit `rustfmt` check of the included probe. Three networking tests initially
failed under sandbox restrictions; the runtime suite passed when rerun with
local networking available. Documentation validation passed source links,
151 script tests (3 skipped), and the complete site using mdBook 0.5.4
(1,444 default and 722 development HTML pages). No compiler integration suite
was run: production code generation and executable Plenty examples are unchanged.

No stack admission fault injection, guard-page overflow, real socket workload,
platform register conformance, signal delivery, blocked-native-call latency or
complete sanitizer coverage is established. The stackful runtime uses glibc
contexts, so it does not benchmark the cost of a future minimal register switch.
Only a compiler prototype can validate coverage of optimized loops, recursion,
tail calls, callbacks, generators and cached objects.
