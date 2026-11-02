# Actor stack-switching validation

Test-only Rust experiment using the production heap router and actor trap
recovery. It adds no scheduler or public API to compiled Plenty programs.

On x86-64 GNU/Linux, from this checkout:

```sh
cargo test --locked -p plenty-runtime actor_stack_switching_probe -- --ignored --nocapture --test-threads=1
```

Run alone: actor heaps belong to one native thread for the process lifetime.
The ignored test is included inside `actor_heap::native` to access its private
TLS state without adding public hooks. The harness uses glibc contexts and
explicit checkpoints as a model of compiler-inserted checks. It tests retained
nested calls and actor-local borrows, interleaving and allocator identity,
contained traps after resumption, cooperative cancellation and reclamation.

See the [design assessment](../../book/src/proposals/actor-budget-scheduling.md)
and [evidence and limitations](../../book/src/proposals/actor-budget-scheduling/evidence.md).
Compiler insertion, `std.actor.Stage` integration, shared-context ownership,
production stack switching, overflow handling, and performance are unvalidated.
