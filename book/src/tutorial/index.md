# Learning Plenty

Plenty combines Python-shaped syntax with explicit types and native compilation.
This guide teaches the language that works today. It grows alongside the
compiler; the runnable examples and expected errors below are tested with
`cargo test --test test_tutorial`.

You do not need to know the old stack-based Plenty. This is a new language.
If you know Python, the indentation and function syntax will look familiar,
but values have fixed types and function interfaces always declare their types.

For functions as values, the short path is [named callbacks](63-pass-functions-as-values.md),
[multiline functions and ordinary captures](65-multiline-anonymous-functions.md),
then [one generic callback API](69-write-a-shared-callback-api.md). Local closure
types are inferred: you need not learn `Closure` or `OnceClosure` to use them.
Capture lists reuse familiar rules: a value moves or copies normally, `mut`
permits changes, and `&` borrows.

Read [borrowed captures](66-borrow-in-a-closure.md) when a callback must access
surrounding state, [factories](67-return-a-closure.md) when returning captured
state, and [consuming callbacks](70-consume-a-capture.md) when handing owned
resources out of a callback. These are advanced paths, not prerequisites for
building a named-function registry or using a local closure.

For a registry with per-callback state, [store explicit state beside a named
function](76-store-stateful-callbacks.md) using an ordinary generic class.
You can also [store owned closures](80-store-owned-closures.md) directly, build
[named registries](81-name-stored-callbacks.md), and queue
[consuming jobs](82-store-consuming-jobs.md).

To inspect enums and recursive chains without consuming them,
[match borrowed values](78-match-borrowed-values.md).
For chains and trees, [build recursive data](77-build-recursive-data.md) with
ordinary classes, enums, and ownership-driven cleanup.

For parallel work with checked borrows and automatic joining,
[run scoped threads](79-run-scoped-threads.md).
[Stored jobs](83-run-stored-jobs.md) compose that model with callback owners and
show how workers return concrete environments without an extra allocation.
Then [move jobs into workers](84-move-jobs-to-workers.md),
[send values between threads](85-send-values-between-threads.md), and
[handle backpressure and early shutdown](86-handle-channel-backpressure.md).
For many owned jobs, [reuse a fixed worker pool](87-reuse-workers-with-an-executor.md)
and [handle executor outcomes](88-handle-executor-outcomes.md).
For workers that return errors, [map fallible work in parallel](89-map-fallible-work-in-parallel.md).
