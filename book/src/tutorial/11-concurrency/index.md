# Concurrent and parallel programs

Follow this optional path when independent work should overlap or several
workers should process a workload. Start with scoped threads and joining, then
channels with bounded queues, then executors and futures. Shutdown, backpressure,
cancellation, and timed waits are part of using those operations safely.

Named worker functions use the core language. Jobs that capture or store data
use the [functions-as-values lessons](../10-functions/index.md); generic recovery
helpers also use [type parameters](../09-generics/index.md). The final lessons
add explicit parallel operations and coordination between channels.

- [Run scoped threads](scoped-threads.md)
- [Borrow local data for a worker](borrowed-workers.md)
- [Move jobs to workers](owned-worker-jobs.md)
- [Run stored jobs](stored-worker-jobs.md)
- [Send values between threads](channels-and-shutdown.md)
- [Handle channel backpressure](channel-backpressure.md)
- [Bound your waits](channel-timeouts.md)
- [Reuse workers with an executor](executors-and-futures.md)
- [Handle executor outcomes](executor-outcomes.md)
- [Cancel queued work and bound a future wait](future-cancellation-and-waits.md)
- [Stop work cooperatively](cooperative-cancellation.md)
- [Recover a job when submission cannot proceed](submission-backpressure.md)
- [Map fallible work in parallel](parallel-fallible-map.md)
- [Combine values in parallel](parallel-reduction.md)
- [Receive from either channel](channel-selection.md)
