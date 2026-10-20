# Consuming thread jobs

`with spawn(job)? as task:` moves a concrete wholly owned zero-argument closure
into a worker. Reusable and one-shot closures are accepted; `def once` permits
moving a capture into the result. Borrowed captures and generator frames are
excluded. Wrap affine named arguments in an owned closure to form one recoverable
job. The [scoped-thread lifetime and join rules](threads.md) still apply.

The start expression has result `Result[(), SpawnError[F]]`, where `F` is the
concrete job type. Its one variant, `SpawnError[F].Unavailable(job)`, returns
the complete unstarted job after any
[native start failure](threads.md#start-failures).

The error does not retain the native code. Returning, matching,
dropping, and retrying the error and job allocate nothing. Capture expressions
have already run, but the worker has not. `?` widens the error into an enclosing
sum containing `SpawnError[F]`, which keeps the job; erasing it into `Failure`
deliberately drops the recovered job. A kept job's endpoints stay alive through
the joins that the `?` exit performs, so the
[live-endpoint join rule](channels.md#joins-and-live-endpoints) counts them. A generic signature names the job as
`F`. A nongeneric one writes its signature, such as
`SpawnError[OnceClosure[[], list[i64]]]`, alone or in a sum; `?` resolves that
member to the job it propagates, as for a
[returned closure](../../language/06-functions/04-consuming-and-stored-closures.md).
Each such member names one producer, so start several jobs from one factory
function or a generic helper.

After success, only the worker owns the captures. One-shot invocation moves them
into the body; reusable invocation borrows them once and then drops the environment.
Join transfers the result exactly once; automatic join drops an unclaimed result.
Job storage and the ownership handoff require no heap allocation.
