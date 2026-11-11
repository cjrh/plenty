# Packed `std.simd` list transfers and view accesses (issue #929)

`load_*` and `store` read or write whole list rows as one packed vector on
optimized x86-64 builds. A loaded `Option[Vector]` stays native through an
unborrowed local, `match` and `unwrap()`. Bounds, the `Nothing`/`False`
results and the scalar fallback are unchanged.

`get_*` and `set` index a slice view by vector and trap outside its whole
vectors. They lower to one comparison and one packed move; a `range` loop over
views bound before it tests each count once
([indexed access through views](../../book/src/design/stdlib/simd.md#indexed-access-through-views)).

| File | Contents |
|---|---|
| `magnitude.plenty` | `dt / (d2 * sqrt(d2))` over three lists: scalar, `f64x2` transfers and `f64x2` view accesses; prints a checksum |
| `transfers.clif` | The same loop over list owners in five guard shapes, for the [kernel harness](../kernels/README.md) |
| `measure.py` | Counts the kernel and five n-body variants; takes the compiler path |

```sh
cargo build --locked --release --bin plenty
python3 -B experiments/simd-transfers/measure.py
target/kernel-harness/release/plenty-kernel-harness experiments/simd-transfers/transfers.clif \
  | python3 experiments/kernels/loops.py
```

## Kernel: magnitude, instructions per two pairs

| Build | Scalar source | `f64x2` source |
|---|---|---|
| GCC -O3 and the packed-pointer [CLIF probe](../nbody-codegen/magnitude.clif) | | 16 |
| Plenty main `d6ed6f91` | 36.13 | 4,194 (nested `match`), 223 excluding callees (`unwrap` and `check`) |
| Plenty with packed transfers | 36.13 | 79 (nested `match`), 141 (`unwrap` and `check`) |
| Plenty with view accesses | 36.12 | 17 (`get_f64x2` and `set`), 75 (nested `match`) |

Executed counts are the difference between 100,000 and 200,000 repeats over
1,000 elements; the two unwrap figures are per-pass counts from `loops.py`
over [the older probe](../nbody-codegen/magnitude-simd.plenty), which also
calls `std.test.check` each pass.

| Cause | Instructions | Notes |
|---|---|---|
| Useful work (also in C) | 15 | 4 packed transfers, 7 arithmetic and one register copy, loop counter |
| Header reloads | 16 | owner, rows, bytes and removed count for each of 4 transfers |
| Length arithmetic | 12 | `(bytes >> 3) - removed - 2` for each transfer |
| Bounds checks | 16 | `offset < 0` and `offset > limit` for each transfer |
| `Option` retest | 9 | each load merges its two outcomes before `match` tests the tag again |
| Overflow check | 3 | `pair * 2` |
| Spills and moves | 6 | three list references reloaded and saved each pass |
| Loop layout | 1 | unconditional jump to the header |

The causes sum to 78 by reading the assembly; `loops.py` and `perf` count 79.

### View accesses

The versioned loop of `magnitude_views` runs 17 instructions per two pairs
(17.21 executed, with the per-call count tests and the empty scalar tail):

| Cause | Instructions | Notes |
|---|---|---|
| Useful work (also in C) | 16 | 4 packed moves, 7 arithmetic and one register copy, counter increment, compare and branch |
| Index scaling | 1 | `lea (i, i)`: element offset of vector `i`; C steps a byte offset instead |

Before the loop, each view's vector count (`count >> 1`) is compared with the
range end once; a failed test runs the original loop, which keeps one compare
and branch per access and calls the scalar body to report the trap. Converting
a list to a view inside the loop also keeps the per-access test. The transfer
kernel falls from 79 to 75 because `splat_*` now lowers inline.

## Guard variants (`transfers.clif`)

Each variant reads every list header per transfer unless it says hoisted.

| Variant | Guard | Per pass |
|---|---|---|
| a | `band` of `offset >= 0` and `offset <= limit`, one branch | 59 |
| b | two branches | 56 |
| c | sign of `offset \| (limit - offset)`, one branch | 60 |
| d | c, headers read once before the loop | 45 |
| e | a, headers read once before the loop | 44 |

The lowering uses b. Variant c is also wrong for a list shorter than the
vector with an offset near `i64.MAX`, where `limit - offset` wraps; it would
need `limit` in the sign test as well.

## n-body

`measure.py` replaces the magnitude loop of the scalar benchmark source with
public transfers; outputs are identical. Instructions at 500,000 steps:

| Variant | Main `d6ed6f91` | Packed transfers | View accesses |
|---|---|---|---|
| Scalar benchmark source | 323,092,366 | 323,092,270 | 323,092,347 |
| `unwrap()` loads, `check(store)` | 4,542,092,165 | 1,435,591,845 | 1,409,591,992 |
| Nested `match` | 11,304,092,928 | 984,591,940 | 944,091,787 |
| `get_f64x2` and `set` over views | | | 827,591,840 |
| Scalar magnitude loop in a view function | | | 858,591,916 |

The SIMD variants remain slower than the scalar source. The scalar `advance`
qualifies for [guarded numerical state](../../book/src/design/toolchain/02-architecture/numerical-loops.md),
which runs a whole step in one 645-instruction pass; a body containing a
transfer does not qualify, so every other loop of the step keeps its checks
and header reloads. The magnitude loop itself is 77 instructions per two
pairs with transfers and 17 with view accesses. The last row isolates the
cause: the scalar magnitude loop moved into a function over views, with no
vectors, costs 859M, more than the view-access variant. Any call in `advance`,
a vector operation included, abandons the guarded-state candidate. The
benchmark source therefore stays scalar.

## Remaining

- Header reloads and length arithmetic (28 of 79): transfers do not take part
  in the loop list-header cache.
- The `Option` retest (9): needs the load's two outcomes to reach the `match`
  or `unwrap` without merging.
- Guarded numerical state does not accept loops containing transfers or view
  accesses: it would need to treat `get_*`/`set` on a view of a promoted list
  as vector cell reads and writes (`src/codegen/fixed_state.rs`).
- View accesses: the index scaling instruction (1 of 17).
- A `match` over a tuple of loads still copies through runtime payload
  helpers: 4,813 instructions per two pairs (issue #944).
