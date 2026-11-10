# Float-vector square root (issue #929)

The `f32x4.sqrt()` and `f64x2.sqrt()` methods use scalar lane bodies on the
fallback path and a native Cranelift `sqrt` on x86-64 optimized builds. A chain
of multiply, sqrt and divide stays in vector registers; no scalar helper call
or intermediate stack load/store remains. Negative lanes produce NaN, signed
zero and infinity follow scalar rules, and no allocation is introduced.

The pre-existing [CLIF experiment](magnitude.clif) already establishes the
arithmetic target: 17 instructions per scalar pair, 16 per two packed pairs.
No inline assembly or dependency patch is necessary. The change deliberately
uses that existing arithmetic lowering rather than changing numeric semantics.

## Complete public path

[magnitude-simd.plenty](magnitude-simd.plenty) runs the public load/compute/store
path and checks lengths 0 through 17 against scalar arithmetic, including odd
tails and an untouched output sentinel. Its hot loop has **237 instructions per
two pairs, excluding callees**, versus 18 per scalar pair in the versioned
scalar kernel. The packed arithmetic includes `sqrtpd`, but three
`load_f64x2` calls, their caller-site tracking, Result/Option unwrapping, enum
storage, stores and checking dominate it. Explicit lane constructors and tuple
extraction were also probed and still took 220 instructions per two pairs.
These are API/representation boundaries, not a failure to lower packed sqrt.

Run `python3 -B experiments/nbody-codegen/sqrt.py` after building the release
compiler. It generates a SIMD magnitude replacement in `target/simd-sqrt`,
compares full n-body output, and counts three runs of 100,000 steps. An initial
measurement was 205,281,728 scalar instructions versus 984,381,971 for the public
SIMD variant. Both produced identical output. This is a regression, so the
benchmark source remains scalar. No timing speedup is claimed.

Run the isolated probe and inspect its assembly:

```sh
PLENTY_CACHE=off target/release/plenty experiments/nbody-codegen/magnitude-simd.plenty
PLENTY_CACHE=off target/release/plenty --dump clif-opt,asm --dump-filter magnitude_simd \
  experiments/nbody-codegen/magnitude-simd.plenty -o target/magnitude-simd
python3 experiments/kernels/loops.py < target/magnitude-simd/asm.txt
```

The capability table checks the trusted module, exact shape, borrowed receiver
and result before selecting a unary intrinsic. The scalar bodies remain the
semantic fallback. Tests cover both profiles, distinct lanes, negative values,
NaNs, infinities, subnormals, signed zero, borrowed parameters and copies.
The emitted-code test requires packed sqrt and absence of scalar calls or
intermediate stack loads/stores in a multiply/sqrt/divide chain.

Issue #929 remains open for the measured transfer/representation gap. This PR
delivers the missing numerical operation, not the complete n-body speedup.

Cache-disabled n-body compilation (including linking), three runs: before [456350190, 451270345, 452566543]; after [450210964, 453851580, 450967077] instructions. No compile-time speedup is claimed.
