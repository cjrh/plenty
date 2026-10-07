# Next milestones

The [proposed next work queue](../proposals/next-work-queue.md) expands the
remaining priorities after the current text-stream work into concrete tasks.

For a useful basic feature set, prioritize these capabilities. This is a proposed
sequence; API syntax and the reference contracts still require design work.

Floating-point types, unit payloads, and the standard sum-type prelude are now
implemented. The new design review changes the recommended priority:

1. Explicit binary `main`, absolute imports, module-private declarations with
   `pub`, qualified dependency identities, and multi-file tutorial examples are
   implemented. Package management and separately cached module objects remain
   future work; neither is required for the next language features.
2. `?` for `Result` and `Option`, allocation-free standard sum wrappers, and
   immutable runtime metadata are implemented. Error payload construction still
   matters: scalar error codes need no allocation, while user-defined enum
   records currently do. Recoverable OOM needs allocation-free error payloads
   as well as fallible runtime operations; wrapper layout alone does not promise it.
3. Fallible allocation now covers collection construction,
   reservation, insertion, explicit copying, and text concatenation/joining/splitting
   and checked character lookup, slices, replacement, and dictionary snapshots through
   ordinary Result-returning APIs. Numeric parsing/formatting, console I/O, argument snapshots,
   and Linux whole-file text helpers now have explicit failure contracts. Continue
   with public allocator lifetimes and buffer allocator selection. Default checked
   literals/comprehensions and aggregate formatting are implemented, as is internal
   object allocator provenance.
   Class/enum/generator checked constructors and list/set iterator collection are
   implemented. Add allocation
   failure injection and checks for valid state/cleanup on every failure path.
4. Element borrowing, borrowed owned-list iteration, dictionary item loops, and
   precise direct-getter return summaries are implemented. Concrete context managers now
   use the same cleanup machinery; stored references remain a later extension.
5. Explicit generic functions and structural protocol constraints are implemented,
   with direct inherent-method lookup and measured instantiation caching. No import-sensitive
   method activation, specialization search, or implicit dynamic interface values.
6. Introduce C ABI adapters and trusted interface declarations, then library output
   and typed runtime loading. Reserve these boundaries during steps 1–3; do not
   expose current internal object layouts as a public foreign ABI.

Move the tutorial to independently runnable literate sources and generated
Markdown alongside the entrypoint migration if convenient, retaining reviewed
expected results. Recursive types, closures, and richer standard-library
APIs remain useful follow-on work. Threads and explicit parallel operations need
thread-transfer rules and a compatible runtime before automatic parallelization
is considered. SIMD needs a target/portable-lowering design of its own. Async/await
remains out of scope. See the linked proposals for scope, tradeoffs, and open
decisions; this ordering does not mean the future features are already approved
down to their syntax.

Improve diagnostics and measure compilation latency throughout these steps,
including archive extraction and native linking. Extend ownership regression,
allocation, and sanitizer checks as reference support grows. Evaluate additional
Polonius-style precision against compilation cost rather than treating a complete
Rust-like borrow checker as a prerequisite for a usable first version.

Tests must distinguish proposed syntax from executable examples. Every current
example should run. Native execution tests exercise output, errors, side-effect
ordering, branch-local mutation, integer widths, and deep tail recursion.
Early-return tests also cover guard fallthrough, all-path returns, mixed
explicit/implicit results, unit returns, nested-frame cleanup, skipped side
effects/errors, unreachable code, and explicit direct/mutual tail calls.
`tests/test_tutorial.rs` reads the tutorial pages in `book/src/tutorial/`
directly, in `SUMMARY.md` order: every `plenty` fence must have a following
`output` fence and runs through compile-and-run and an explicitly compiled
binary; every `plenty-error` fence has an `error` substring and must fail in
both commands without executing effects. It also fails if a book page is
missing from `SUMMARY.md`. Do not maintain a separate copy of tutorial source
in tests. Update the guide as part of each learner-visible language change.
Native legacy regression tests specify their expected output independently.
Performance results must name the build mode, machine, input size, and whether
archive extraction and linking are included; no latency claim without measurement.
