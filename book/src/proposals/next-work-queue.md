# Foundation batch history

This is a historical completion record for the ten items proposed after
`47888d0`. It is not an active queue. All remaining work and priorities now live
in the [backlog](../backlog.md); current contracts and limits live in the
[implementation status](../design/04-implementation-status.md) and reference.

The table describes the original commits, including syntax later replaced.
In particular, `try` and `try_` APIs are obsolete, allocation-capable APIs now
return Result by default, and generic calls can infer type arguments. Validation
counts and timings below belong to that historical batch, not the current tree.

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

## Validation recorded at completion

- All 1,259 workspace tests passed with `runtime-checks`, including native ownership,
  cleanup, module, inference, protocol, and allocation-failure regressions.
- Every tutorial program and expected diagnostic runs through the tutorial test.
- Strict workspace Clippy covers all targets; Rust formatting is checked.
- All 68 existing allocation-checking runtime Miri tests passed, and the new
  full-width unsigned-range Miri regression passed separately.
- Failure-budget sweeps exercise every reached formatting-buffer growth and final
  string allocation. Failed formatting writes no partial output and preserves its
  input. Collection and tuple failure tests check owned-value cleanup.
- A focused debug specialization test verifies that 100 calls plus recursion
  create one concrete function; the measured expansion cost was about 0.34 ms
  on the AMD Ryzen 7 7840HS development machine. This excludes native codegen and
  linking and is not a broad compiler benchmark.
