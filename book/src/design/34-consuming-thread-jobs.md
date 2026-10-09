# Consuming thread jobs

`with spawn(job)? as task:` moves a concrete, wholly owned zero-argument closure
into a native worker. Reusable and one-shot closures are accepted; use `def once`
when the worker moves a capture into its result. Borrowed captures and generator
frames are excluded. Wrap affine named arguments in an owned closure so they form
one recoverable job.

The start result is `Result[(), SpawnError[F]]`, where `F` is the concrete job
type. `SpawnError[F].Unavailable(job)` returns an unstarted job on resource/start
failure; `SpawnError[F].PermissionDenied(job)` returns it on native permission
denial. These portable categories deliberately do not retain a raw native code.
Borrowed/named starts continue to return `ThreadError.System(code)`.

The error and its environment occupy inline owner storage. No allocation is
needed to return, match, drop, or retry a failed job. Capture expressions have
already run; the worker itself has not. A generic helper returning
`Result[T, SpawnError[F]]` can expose failure for matching and retry. Propagating
into `Failure` deliberately discards the error and drops its retained job.

After successful creation only the worker owns the captures. A one-shot invocation
transfers them into its body; a reusable invocation borrows them and then drops
the environment. Join transfers the result exactly once. An unclaimed result is
dropped after the automatic join. All ordinary scope-exit rules remain unchanged.

Pinned parent-frame task storage holds the relocated environment until join.
The original operand becomes inert on success and becomes the residual owner on
failure. The adapter never reads the original operand. This separates the native
creation decision from the ownership handoff without heap allocation.

`tests/test_consuming_threads.rs` checks moves, result transfer, rejection of
borrowed/uncertified jobs, allocation-disabled failure and retry, and exactly-once
cleanup on both paths.
