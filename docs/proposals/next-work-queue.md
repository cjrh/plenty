# Foundation queue: completed work and remaining limits

The ten foundation items proposed after `47888d0` now have implemented paths.
The commits below are local; no push was requested. `DESIGN.md` describes the
current contracts, and the learner-visible features have runnable tutorial
examples. The limits listed here remain intentional parts of the initial scope.

| Item | Implemented | Commits |
| --- | --- | --- |
| 1. Fallible literals | `try [...]`, `try {...}`, contextual empty displays, ordered evaluation and partial cleanup | `8676622` |
| 2. Fallible comprehensions | Checked output growth, stopped nested loops and generators on failure, cleanup of owned prefixes | `a83dca9` |
| 3. Formatting/allocation audit | `str.try_repr`, `try_print`, checked formatting buffers, and explicit remaining aborting paths | `e1a12dd`, `4e60ee9` |
| 4. Allocator provenance | Object allocations remember their allocator; internal explicit allocation uses a process-lifetime callback table | `c0b07ce` |
| 5. Element borrowing | Shared/mutable list elements and dictionary values, nested field projections, invalidation checks | `2b27849` |
| 6. Borrowed owned-element iteration and returned references | Shared/mutable list loops, returned element references, and precise direct-getter field summaries | `e8a02b0`, `d752f03` |
| 7. Tuples, unpacking, dictionary items | Tuple values/types, checked tuple construction, flat loop/comprehension unpacking, borrowed dictionary item loops | `6c0fbb5`, `65b9834`, `20f82ee` |
| 8. Typed ranges and numeric context | `range[T]`, all integer widths including full u64 bounds, contextual literals and direct range comprehensions | `9bf1208` |
| 9. Explicit generic functions | Explicit type arguments, cached concrete instances, bounded expansion, and `IntType` | `77105b7` |
| 10. Structural protocols | Method requirements checked at specialization, exact borrowing/signature matches, and ordinary module visibility | `08988d3` |

## Validation

- All 1,259 workspace tests pass with `runtime-checks`, including native ownership,
  cleanup, module, inference, protocol, and allocation-failure regressions.
- Every tutorial program and expected diagnostic runs through the tutorial test.
- Strict workspace Clippy covers all targets; Rust formatting is checked.
- All 68 existing allocation-checking runtime Miri tests pass, and the new
  full-width unsigned-range Miri regression passes separately.
- Failure-budget sweeps exercise every reached formatting-buffer growth and final
  string allocation. Failed formatting writes no partial output and preserves its
  input. Collection and tuple failure tests check owned-value cleanup.
- A focused debug specialization test verifies that 100 calls plus recursion
  create one concrete function; the measured expansion cost was about 0.34 ms
  on the AMD Ryzen 7 7840HS development machine. This excludes native codegen and
  linking and is not a broad compiler benchmark.

## Boundaries that remain

- **Allocation:** ordinary construction/printing and some runtime bookkeeping may
  abort. Checked operations do not provide a process-wide OOM guarantee. Public
  allocator selection, allocator state/lifetimes, and custom container buffers
  remain to be designed; the internal provenance mechanism is groundwork.
- **References:** collection loans cover the whole collection. Returned references
  still require one reference parameter; only direct getter bodies have precise
  field summaries. Stored references, disjoint indexed loans, and general lifetime
  relationships remain unsupported.
- **Tuples/items:** tuple storage currently allocates. Unpacking is flat;
  dictionary `items()` is a loop/comprehension intrinsic, not a storable iterator.
  Snapshots must be requested explicitly, including copies of owned values.
- **Inference:** numeric context stays within supported expressions; it does not
  flow backward through arbitrary calls, filters, or stored range values.
- **Generics/protocols:** calls require explicit type arguments. Bodies are checked
  per concrete instance, with a 256-instance compilation limit. Protocols currently
  constrain classes, using exact method signatures. Generic classes/methods,
  multiple bounds, associated types, inheritance, and runtime interface values
  remain deferred.

## Follow-on work

These are the next design/implementation areas, rather than unimplemented pieces
of the ten-item foundation batch:

1. C ABI adapters and trusted interface declarations, followed by shared-library
   output and loading. Keep internal object layouts private.
2. Multiline anonymous functions and closures with explicit capture/borrow rules.
3. Recursive data types with a clear indirection and destruction model.
4. Public allocator lifetimes and per-container selection, building on provenance
   and the new protocol machinery.
5. Fallible file iteration, buffering, and binary I/O, following Python's familiar
   model where it fits Plenty's ownership and explicit error handling.
6. Standalone literate lessons that generate `TUTORIAL.md`; its current examples
   already run in tests.
7. Thread-transfer rules, threads/channels, explicit parallel operations, and
   SIMD. Automatic parallelization follows those foundations.

Continue updating the design and executable tutorial alongside implementation.
Measure compilation latency as inference and specialization expand.
