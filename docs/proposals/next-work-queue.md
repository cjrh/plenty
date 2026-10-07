# Proposed next work queue

Status: proposed priorities after commit `4f63036`; these are not implemented
features or a promise that each item fits one implementation cycle.

Update after the next ten-cycle batch: the allocation audit, fallible class/enum
and generator construction, partial initializer cleanup, and fallible list/set
collection are implemented. Ordinary literals/comprehensions, remaining implicit
allocation paths, and allocator provenance are still the next priorities. The
numbered list below records the original queue rather than current status; see
`DESIGN.md` for implemented contracts.

The next priority is to finish the recoverable-allocation foundation, then expand
borrowing and generic programming. Text file I/O now has a useful baseline;
buffering and additional stream conveniences can wait.

1. **Audit the remaining allocation paths.** Inventory literals, comprehensions,
   classes, user enums, generators, formatting, and runtime bookkeeping. Identify
   which failures still abort, and specify how each operation exposes failure.
   Decide the source syntax before changing existing construction behavior.
2. **Make class and user-enum construction recoverable.** Reclaim already
   initialized fields on failure, preserve ownership correctly, and ensure that
   reporting an allocation error does not itself need an allocation.
3. **Make generator creation recoverable.** Cover frame allocation and captured
   owners, including failures before iteration starts and early abandonment.
4. **Close the literal, comprehension, and remaining runtime allocation gaps.**
   Provide a consistent propagation story, including cleanup after partial
   construction. Failure-injection tests should establish the precise guarantee;
   having fallible methods alone does not make an entire program OOM-recoverable.
5. **Introduce allocator provenance.** Design how storage remembers its allocator
   and how growth, movement, copying, and destruction preserve that association.
   Start with a small allocator interface and one explicit construction path;
   global selection and per-container selection must have clear lifetime rules.
6. **Borrow collection elements.** Add shared and mutable element access, reject
   invalidating mutations while a reference is live, and define bounds/missing-key
   behavior. Start conservatively before supporting disjoint element loans.
7. **Iterate over owned elements by borrowing.** Allow inspecting lists of classes
   and other owned values without moving or copying every element. Build this on
   the element-reference rules, then improve returned-reference precision.
8. **Add tuples, unpacking, and dictionary item iteration.** This unlocks ordinary
   Python-style key/value loops and convenient multiple return values. Define
   ownership of tuple components and distinguish borrowed items from snapshots.
9. **Implement typed ranges and contextual numeric inference.** Support the
   proposed `range[u8](8)` and annotated comprehension examples. Specify overflow,
   argument compatibility, and inference boundaries so errors remain predictable.
10. **Implement explicit generic functions, then structural protocols.** Begin
    with explicit type arguments and cached concrete instantiations. Add checked
    protocol requirements without import-sensitive method activation. Numeric
    constraints such as the proposed `IntType` should fit this design.

Items 2–5 and 10 may each require several focused implementation/test/commit
cycles. The first audit may change their boundaries or reveal dependencies; this
list is a priority order, not ten predetermined commits.

## Following these foundations

- C ABI adapters and trusted interface declarations, followed by shared-library
  output and loading. Keep internal runtime layouts private in the meantime.
- Multiline anonymous functions and closures with explicit capture/borrow rules.
- Recursive data types with a clear indirection and destruction model.
- Fallible file iteration, buffering, and binary I/O.
- Runnable literate lesson files that generate `TUTORIAL.md`; the current guide's
  examples already run in tests, so this is a workflow improvement rather than a
  missing correctness check.
- Thread-transfer rules, threads/channels, explicit parallel operations, and SIMD.
  Automatic parallelization comes after those foundations.

Update `DESIGN.md` and the tested tutorial with every learner-visible change.
Measure compilation latency as inference, borrowing, and instantiation become
more capable. Keep cleanup and allocation-failure regression checks alongside
the feature work.
