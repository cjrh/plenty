# n-body with a reciprocal square root estimate (issue #946)

The fast Benchmarks Game C and Rust n-body programs replace
`dt / (d2 * sqrt(d2))` with `rsqrtps` on the narrowed squared distances and
two Newton-Raphson steps in `f64`.
[`f32x4.reciprocal_sqrt_estimate()`](../../book/src/design/stdlib/simd.md#reciprocal-square-root-estimate)
and the `f64x2`/`f32x4` lane conversions express that in Plenty.

| File | Contents |
|---|---|
| `nbody.c` | The benchmark's step structure with the estimate; `-DEXACT` selects `sqrtpd` and `divpd` |
| `nbody_exact.plenty` | `benchmarks/game/n-body/plenty/nbody_simd.plenty` with exact `sqrt` and `div` |

```sh
cargo build --release --bin plenty
target/release/plenty --compile benchmarks/game/n-body/plenty/nbody_simd.plenty -o simd
target/release/plenty --compile experiments/nbody-rsqrt/nbody_exact.plenty -o exact
gcc -O3 -march=native experiments/nbody-rsqrt/nbody.c -o c_est -lm
gcc -O3 -march=native -DEXACT experiments/nbody-rsqrt/nbody.c -o c_exact -lm
perf stat -r 5 -e instructions:u,cycles:u taskset -c 3 ./simd 5000000
```

## Results

Ryzen 7 7840HS (Zen 4), one pinned core, 5,000,000 steps, `perf stat -r 5`,
three interleaved rounds, load average 2.8. Every program prints
`-0.169075164`, `-0.169087605` at 1,000 steps and `-0.169059907` at
50,000,000.

| Program | Instructions | Cycles | Time |
|---|---|---|---|
| Plenty scalar (`nbody.plenty`) | 3,211M | 1,188M | 256 ms |
| Plenty `f64x2`, exact | 3,136M | 1,083M | 233 ms |
| Plenty `f64x2`, estimate (`nbody_simd.plenty`) | 3,646M | 1,060M | 229 ms |
| GCC 16 exact | 1,760M | 994M | 214 ms |
| GCC 16 estimate | 2,070M | 891M | 192 ms |
| Clang 22 exact | 1,270M | 1,026M | 221 ms |
| Clang 22 estimate | 1,870M | 793M | 171 ms |
| Rust reference (`reference/nbody.rs`, exact) | 1,146M | 613M | 133 ms |

The estimate saves 2% of cycles in Plenty, 10% with GCC and 23% with Clang.
The Plenty difference holds under three shifted code placements (an unused
function of varying size before `advance`): 1,054M to 1,065M cycles with the
estimate against 1,078M to 1,093M exact.

The magnitude instructions match: Plenty emits `cvtpd2ps`, `rsqrtps`,
`cvtps2pd`, then five multiplies and a subtract per refinement step, as GCC
does, and keeps the `dt / d2` division the C entries keep. The remaining cost
is around it. Guarded numerical state holds all 75 list cells of `advance` as
scalar values: 16 vector registers cannot hold them, so most are spilled and
reloaded (`movdqu` to and from the stack), each `f64x2` is assembled from two
scalars (`pshufd`, `movlhps`) and split again after the step. A step is one
dependent chain through those reloads, which the estimate does not shorten:
on Zen 4 `sqrtpd` and `divpd` take about 13 cycles each, and the estimate
with two refinement steps is not shorter in latency. It gains by freeing the
divider, which helps only a loop limited by throughput. Dropping the division
(`dt * y * y * y`) lengthens the chain and costs 6% in Plenty.

The Rust reference is fastest with exact arithmetic: LLVM keeps the bodies in
registers and computes the magnitudes four at a time with AVX.
