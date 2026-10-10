# Concurrent and parallel programs

Use scoped threads for work that borrows local data, or an executor for many
owned jobs. Captured jobs use the [functions-as-values lessons](../10-functions/index.md);
generic recovery helpers also use [type parameters](../09-generics/index.md).

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
