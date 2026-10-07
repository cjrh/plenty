# Proposed next work queue

Status: proposed priorities after commit `47888d0`. Each item may take several
implementation/test/commit cycles; this is not a promise of ten commits.

Completed in the last batch: the allocation audit, fallible class/enum/generator
constructors, partial initializer cleanup, generators inside Option/Result,
shared generator resume code, and fallible list/set collection from iterators.
See `DESIGN.md` for the implemented contracts.

The next priority is to finish the recoverable-allocation foundation, then expand
borrowing and generic programming. Text file I/O now has a useful baseline;
buffering and additional stream conveniences can wait.

1. **Fallible collection literals.** Choose a concise, explicit construction
   syntax for lists, dictionaries, and sets. Today, passing a literal to
   `list[T].try_from(...)` still builds that literal using ordinary allocation
   first. Cover evaluation order, failure propagation, and partial cleanup.
2. **Fallible comprehensions.** Build on the literal and iterator machinery so
   output allocation failures can be handled without rewriting comprehensions as
   manual loops. Specify separately how failures inside element expressions,
   filters, and iterator bodies propagate.
3. **Remaining implicit allocation and formatting gaps.** Provide a recoverable
   path for common output/aggregate formatting and finish the runtime bookkeeping
   audit. Preserve a precise distinction between fallible APIs and convenience
   operations that can abort. Failure injection should verify cleanup at every
   new boundary; these APIs alone cannot guarantee recovery from all process OOM.
4. **Introduce allocator provenance.** Design how storage remembers its allocator
   and how growth, movement, copying, and destruction preserve that association.
   Start with internal plumbing and an explicit construction path. Decide allocator
   lifetimes before exposing global or per-container selection; the general
   user-defined allocator interface may depend on protocols below.
5. **Borrow collection elements.** Add shared and mutable element access, reject
   invalidating mutations while a reference is live, and define bounds/missing-key
   behavior. Start conservatively before supporting disjoint element loans.
6. **Iterate over owned elements by borrowing.** Allow inspecting lists of classes
   and other owned values without moving or copying every element. Build this on
   the element-reference rules, then improve returned-reference precision.
7. **Add tuples, unpacking, and dictionary item iteration.** This unlocks ordinary
   Python-style key/value loops and convenient multiple return values. Define
   ownership of tuple components and distinguish borrowed items from snapshots.
8. **Implement typed ranges and contextual numeric inference.** Support the
   proposed `range[u8](8)` and annotated comprehension examples. Specify overflow,
   argument compatibility, and inference boundaries so errors remain predictable.
   This can begin with the compiler-known range operation while reserving syntax
   compatible with user generics.
9. **Explicit generic functions.** Begin with explicit type arguments and cached
   concrete instantiations. Measure compilation cost and give useful diagnostics
   before broadening inference or adding more generic declaration forms.
10. **Structural protocols.** Check required methods and their ownership/borrowing
    signatures at compile time, without import-sensitive method activation or
    implicit dynamic dispatch. Numeric constraints such as the proposed `IntType`
    need a deliberate builtin constraint design alongside ordinary protocols.

The immediate recommendation is items 1–2. Allocator work should establish the
necessary ownership rules without holding up all borrowing and generic-language
work until a complete custom-allocator ecosystem exists.

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
