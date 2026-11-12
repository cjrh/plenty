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
| `measure.py` | Interleaved timing of scalar, exact and estimate at `x86-64` and `x86-64-v3`, eight layouts each |
| `ulp.c` | Error of the refined magnitude against the exact expression |

```sh
cargo build --release --bin plenty
target/release/plenty --compile benchmarks/game/n-body/plenty/nbody_simd.plenty -o simd
target/release/plenty --compile experiments/nbody-rsqrt/nbody_exact.plenty -o exact
gcc -O3 -march=native experiments/nbody-rsqrt/nbody.c -o c_est -lm
gcc -O3 -march=native -DEXACT experiments/nbody-rsqrt/nbody.c -o c_exact -lm
perf stat -r 5 -e instructions:u,cycles:u taskset -c 3 ./simd 5000000
python3 experiments/nbody-rsqrt/measure.py --build
python3 experiments/nbody-rsqrt/measure.py 3 5
gcc -O2 -ffp-contract=off experiments/nbody-rsqrt/ulp.c -o ulp -lm && ./ulp
```

## Results

Ryzen 7 7840HS (Zen 4), core 3, 5,000,000 steps, `PLENTY_CACHE=off`, release
compiler. `measure.py` compiles each variant at both levels in eight code
layouts and runs all 48 programs once per round, five interleaved rounds,
reversing the order each round. Cycles and instructions are per step, medians
over 40 runs; the bracket is the range of the eight per-layout medians. Load
average 2.8 to 3.3, no build running.

| Program | Level | Cycles | Layout range | Instructions | Time |
|---|---|---|---|---|---|
| scalar (`nbody.plenty`) | `x86-64` | 220.5 | 219.6-224.0 | 652 | 262 ms |
| scalar | `x86-64-v3` | 216.6 | 216.4-217.1 | 552 | 259 ms |
| `f64x2`, exact (`nbody_exact.plenty`) | `x86-64` | 214.7 | 214.3-214.8 | 621 | 255 ms |
| `f64x2`, exact | `x86-64-v3` | 199.9 | 199.0-200.4 | 540 | 238 ms |
| `f64x2`, estimate (`nbody_simd.plenty`) | `x86-64` | 211.7 | 211.3-215.6 | 723 | 251 ms |
| `f64x2`, estimate | `x86-64-v3` | 203.2 | 201.9-205.3 | 600 | 242 ms |

For reference on the same machine: Rust `reference/nbody.rs` 158 cycles per
step and GCC `-O3` 197. In `nbody.c`, measured earlier with `perf stat -r 5`,
the estimate saves 10% (GCC 16) and 23% (Clang 22) over exact `sqrtpd` and
`divpd`.

The estimate saves 1.4% at `x86-64` and costs 1.6% at `x86-64-v3`; the layout
ranges do not overlap at either level. Every program prints `-0.169075164`,
`-0.169087605` at 1,000 steps and `-0.169059907` at 50,000,000, at both levels.
`ulp.c` measures the refined magnitude `dt / d2 * y` against
`dt / (d2 * sqrt(d2))` over 2*10^7 values of `d2`: at most 125 ulp (88 ulp
for `y` against `1 / sqrt(d2)`), the same from `1e-30` to `1e30`.

The magnitude instructions match GCC's: `cvtpd2ps`, `rsqrtps` (`vrsqrtps` at
`x86-64-v3`), `cvtps2pd`, then five multiplies and a subtract per refinement
step, and the `dt / d2` division the C entries keep. The cost is around them.
Guarded numerical state holds the list cells of `advance` as scalar values: 16
vector registers cannot hold them, so most are spilled and reloaded (`movdqu`
to and from the stack), each `f64x2` is assembled from two scalars (`pshufd`,
`movlhps`) and split again after the step. A step is one dependent chain
through those reloads. On Zen 4 the exact path's latency is about `sqrtpd`
plus a multiply plus `divpd`; the estimate path is the conversions,
`rsqrtps` and two refinement steps of three dependent operations each, about
as long. The estimate frees the divider, which helps only a loop limited by
throughput, and adds 12 instructions per vector. At `x86-64-v3` the VEX forms
drop the register copies, the exact loop gets 7% faster, and the added
instructions outweigh what the estimate saves.

Intel is not measured. Its 128-bit divider and square root unit are slower
relative to multiplies than Zen 4's, so the estimate should gain more there;
that is an expectation, not a figure.
