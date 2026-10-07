# Development workflow

The [backlog](../backlog.md) is the only maintained list of future work and
proposed priorities. This page describes how work is carried out, not what comes
next. For implemented capabilities and current limits, use the
[implementation status](04-implementation-status.md).

## Completing a feature

Keep the implementation, reference contract, and executable tutorial aligned.
Update the selected backlog entry when its scope changes or work completes.
Proposals retain design rationale and alternatives; they do not acquire their
own active task queues. Improve diagnostics and measure compilation latency as
inference, specialization, and backend work evolve.

Tests must distinguish proposed syntax from executable examples. Native tests
exercise output, errors, evaluation order, mutation, numeric widths, ownership,
borrowing, and cleanup. Check allocation failure and valid partial-state cleanup
where allocations are involved. Use sanitizer and runtime allocation checks
where relevant; extend borrowing precision against measured compilation cost.

## Runnable documentation

`tests/test_tutorial.rs` reads tutorial pages in `book/src/tutorial/` directly,
in `SUMMARY.md` order. Every `plenty` fence has a following `output` fence and
runs through compile-and-run and an explicitly compiled binary. Every
`plenty-error` fence has an `error` diagnostic substring and must fail without
executing effects. A `plenty-file` companion module stays on the same page as its
example. Every book page must appear in `SUMMARY.md`.

Do not maintain another copy of tutorial sources in tests. Update the lessons
as part of each learner-visible language change. Native legacy regression tests
specify their expected output independently.

## Performance evidence

Measure checking, code generation, archive extraction, and native linking as
appropriate. Report build mode, machine, input size, and included phases with
performance results. Fast compilation remains a design goal; timing claims need
measurements rather than assumptions about an implementation strategy.
