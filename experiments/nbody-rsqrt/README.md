# n-body with a reciprocal square root estimate (issue #946)

The fast Benchmarks Game C and Rust n-body programs replace
`dt / (d2 * sqrt(d2))` with `rsqrtps` on the narrowed squared distances and
two Newton-Raphson steps in `f64`. Issue #946 prototyped that in Plenty as
`f32x4.reciprocal_sqrt_estimate()`, lowered to `rsqrtps` through the
Cranelift fork's `x86_rsqrtps` instruction. **It was declined and is not in
`std.simd`.** The lane conversions it needed (`f64x2.to_f32x4()`,
`f32x4.to_f64x2()` and the `f64x4` forms) were kept.

## Result

- Speed: on Zen 4 the estimate cut n-body's cycles per step by 1.4% at
  `--cpu x86-64` and raised them by 1.6% at `x86-64-v3`, against the same
  program with exact `f64x2` `sqrt` and `div`.
- Determinism: `rsqrtps` is specified only to a relative error of
  `1.5 * 2**-12`. Its bits differ between Intel and AMD and between processor
  generations; the scalar fallback (`1.0 / sqrt`, used by `--fast-compile`
  and AArch64) differs again. The estimate would have been the only
  `std.simd` operation whose results depend on the processor or profile.
  On one machine, `rsqrtps` and `vrsqrtps` gave identical bits.

| File | Contents |
|---|---|
| `nbody.c` | The benchmark's step structure with the estimate; `-DEXACT` selects `sqrtpd` and `divpd` |
| `nbody_exact.plenty` | n-body with `f64x2` view accesses and exact `sqrt` and `div`: the baseline the estimate was measured against |
| `ulp.c` | Error of the refined magnitude against the exact expression |

```sh
gcc -O3 -march=native experiments/nbody-rsqrt/nbody.c -o c_est -lm
gcc -O3 -march=native -DEXACT experiments/nbody-rsqrt/nbody.c -o c_exact -lm
gcc -O2 -ffp-contract=off experiments/nbody-rsqrt/ulp.c -o ulp -lm && ./ulp
target/release/plenty --compile experiments/nbody-rsqrt/nbody_exact.plenty -o exact
```

The Plenty estimate variant replaced the exact magnitude line of
`nbody_exact.plenty` with:

```text
mut inverse = d2.to_f32x4().reciprocal_sqrt_estimate().to_f64x2()
half_d2 = d2.mul(half)
inverse = inverse.mul(three_halves).sub(half_d2.mul(inverse).mul(inverse.mul(inverse)))
inverse = inverse.mul(three_halves).sub(half_d2.mul(inverse).mul(inverse.mul(inverse)))
step.div(d2).mul(inverse).set(out, pair)
```

It does not build without the removed method.

## Measurements

Ryzen 7 7840HS (Zen 4), core 3, 5,000,000 steps, `PLENTY_CACHE=off`, release
compiler at branch commit `4c45dd77`. Each variant was compiled at both
levels in eight code layouts (0 to 7 statements added to `energy`, as
`experiments/nbody-spills/layouts.sh` does; issue #437). All 48 programs ran
once per round, five interleaved rounds, alternating order, under
`perf stat`. Cycles and instructions are per step, medians over 40 runs; the
bracket is the range of the eight per-layout medians. Load average 2.8 to
3.3, no build running.

| Program | Level | Cycles | Layout range | Instructions | Time |
|---|---|---|---|---|---|
| scalar (`nbody.plenty`) | `x86-64` | 220.5 | 219.6-224.0 | 652 | 262 ms |
| scalar | `x86-64-v3` | 216.6 | 216.4-217.1 | 552 | 259 ms |
| `f64x2`, exact (`nbody_exact.plenty`) | `x86-64` | 214.7 | 214.3-214.8 | 621 | 255 ms |
| `f64x2`, exact | `x86-64-v3` | 199.9 | 199.0-200.4 | 540 | 238 ms |
| `f64x2`, estimate | `x86-64` | 211.7 | 211.3-215.6 | 723 | 251 ms |
| `f64x2`, estimate | `x86-64-v3` | 203.2 | 201.9-205.3 | 600 | 242 ms |

The layout ranges of estimate and exact do not overlap at either level. For
reference on the same machine: Rust `reference/nbody.rs` 158 cycles per step
and GCC `-O3` 197. In `nbody.c`, measured earlier with `perf stat -r 5`, the
estimate saves 10% (GCC 16) and 23% (Clang 22) over exact `sqrtpd` and
`divpd`.

Every program printed `-0.169075164`, `-0.169087605` at 1,000 steps and
`-0.169059907` at 50,000,000, at both levels. `ulp.c` measures the refined
magnitude `dt / d2 * y` against `dt / (d2 * sqrt(d2))` over 2*10^7 values of
`d2`: at most 125 ulp (88 ulp for `y` against `1 / sqrt(d2)`), the same from
`1e-30` to `1e30`.

## Why it did not help

The magnitude instructions matched GCC's: `cvtpd2ps`, `rsqrtps` (`vrsqrtps`
at `x86-64-v3`), `cvtps2pd`, then five multiplies and a subtract per
refinement step, and the `dt / d2` division the C entries keep. The cost is
around them. Guarded numerical state holds the list cells of `advance` as
scalar values: 16 vector registers cannot hold them, so most are spilled and
reloaded (`movdqu` to and from the stack), and each `f64x2` is assembled from
two scalars (`pshufd`, `movlhps`) and split again after the step. A step is
one dependent chain through those reloads. On Zen 4 the exact path's latency
is about `sqrtpd` plus a multiply plus `divpd`; the estimate path is the
conversions, `rsqrtps` and two refinement steps of three dependent operations
each, about as long. The estimate frees the divider, which helps only a loop
limited by throughput, and adds 12 instructions per vector. At `x86-64-v3`
the VEX forms drop register copies, the exact loop gets 7% faster, and the
added instructions outweigh what the estimate saves.

Intel was not measured. Its 128-bit divider and square root unit are slower
relative to multiplies than Zen 4's, so the estimate should gain more there;
that is an expectation, not a figure.

## A deterministic alternative

An integer first guess removes the processor dependence: for an `f64` lane
with bits `b`, `0x5FE6EB50C7B537A9 - (b >> 1)` read as `f64` approximates
`1 / sqrt(x)` within about 3.4%, and three Newton-Raphson steps reach full
`f64` precision. Every CPU, target and profile computes the same bits. It
needs integer lane shifts and float/integer lane bitcasts in `std.simd`, and
costs one refinement step more than `rsqrtps`. It was not implemented.
