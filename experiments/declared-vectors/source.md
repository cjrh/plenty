# Source-level Vec3 prototype (issue #967)

This continuation implements `std.vector.Vec3` in ordinary Plenty source and
compiles the source n-body program through the normal compiler. No CLIF kernel,
machine-code replacement or foreign vector implementation is injected. The
scalar method bodies define behavior; eligible optimized x86 functions keep
three logical components in private native registers. The existing benchmark
is unchanged. This is a prototype, with the costs below exposed for review.

## Scope and representation

The initial type is a copy class with public `x`, `y`, `z: f64`, arithmetic,
ordered `dot`, `length`, checked `lane` and `to_tuple`. The
[reference](../../book/src/design/stdlib/vector.md) specifies its API and limits.
There is no Vec4, generic lane type, packed collection redesign or new C ABI.

Ordinary class layout is retained: three 16-byte fields occupy 48 payload
bytes; an owning slot adds 16 bytes where required. A body with position,
velocity and mass occupies 112 payload bytes, or 128 with its owning slot.
`array[Body, 5]` has 560 logical payload bytes. This is not the proposed compact
32-byte vector layout. Lists and arrays use their existing class machinery;
`clist[Vec3]` retains its existing scalar columns; nested vector-field
column flattening is outside this implementation.

The private register form is two 128-bit parts at the default CPU level, or
one 256-bit part at `x86-64-v3`. Loads read exactly three ordinary fields.
Padding is positive zero; scaling uses an inactive multiplier of one, avoiding
`0 * Inf` before masking. Subtraction restores positive-zero padding even under
round-down; negation flips only real component sign bits. Dot computes
`(x*x + y*y) + z*z`, without FMA or reassociation. Unsupported native operations
use the existing narrow fallback; fast compilation uses scalar bodies.

## Attribution and controls

`source.py` generates matched complete programs. The scalar array control
retains the old floating operation order. The ordinary-class control uses the
same Vec3 source/API and paired-magnitude algorithm, but a local class identity
that deliberately does not trigger the std-only lowering. The unpaired Vec3
control differs only in using two scalar square roots/divisions. The current
production benchmark is also compiled unchanged except for optional state
output. C uses the existing strict scalar-locals driver, with vectorization and
FP contraction disabled.

Each of five adjacent pairs supplies two real squared distances to existing
`std.simd.f64x2` sqrt/div operations. Velocity updates retain their original
pair order. This pairing is explicit source code, rather than an automatic
compiler transformation or arithmetic relaxation.

The initial source-native loop had 366 instructions and was slower than the
production benchmark despite fewer instructions. Emitted-code attribution
identified padding construction, unsupported wide negation triggering a narrow
rebuild, and dead canonical snapshots retaining frame reservations. The final
lowering constructs the neutral scale operand in two halves, uses sign-bit
negation, and removes only unreferenced snapshot reservations after the
existing instruction-group liveness pass. Live addressable values retain
ordinary storage. The final frame is about 2.8 KiB, versus about 21 KiB in the
initial lowering; the unnecessary stack-probe loop disappears.

All comparisons use the same branch-base compiler configuration (`58e5a09b`,
Cranelift `98c0373`) plus this prototype. They do not silently adopt the
separate scheduling fork/pin work from issue #972. Static final hot-loop
instructions on the measured x86-64 host:

| Source/profile | Instructions |
|---|---:|
| Current benchmark, v3 | 395 |
| Scalar array, v3 | 468 |
| Ordinary local Vec3 class, paired, v3 | 1122 plus callees |
| Native Vec3, paired, v3 | 307 |
| Native Vec3, unpaired, v3 | 311 |
| Current benchmark, default CPU | 473 |
| Native Vec3, default CPU | 538 |
| Scalar array, default CPU | 633 |

The v3 native loop contains five packed square roots and five packed divisions,
with no FMA and no whole-function narrow rebuild. These counts do not establish
a runtime improvement, and the default CPU profile has more instructions than
the current benchmark.

## Runtime result and adoption gate

Nine rotating samples on the same pinned Zen 4 core, one million steps each,
produce the following medians. Instructions and cycles are millions; all raw
samples and exact identities are retained in `source-results.json`.

| Source/profile | Instructions | Cycles | Task-clock ms |
|---|---:|---:|---:|
| Current benchmark, v3 | 395.723 | 177.945 | 38.39 |
| Native Vec3, paired, v3 | 307.658 | 178.377 | 38.21 |
| Native Vec3, unpaired, v3 | 311.658 | 189.670 | 40.79 |
| Scalar array, v3 | 468.656 | 211.090 | 45.15 |
| Ordinary local Vec3 class, paired, v3 | 5397.659 | 3992.263 | 834.92 |
| Strict C scalar locals, v3 | 359.493 | 208.301 | 44.69 |
| Current benchmark, default CPU | 473.723 | 179.760 | 38.81 |
| Native Vec3, default CPU | 538.657 | 206.409 | 44.42 |

The source-native AVX2 candidate executes 22.25% fewer instructions, but its
median cycles are 0.24% higher and task-clock 0.47% lower: effectively tied.
**The strict source-level speedup/adoption gate is not passed.** Pairing reduces
cycles 5.95% against the unpaired Vec3 version, so it remains essential to this
candidate. The default-CPU candidate is 14.82% slower by cycles. This does not
reproduce the native-only experiment's robust whole-program gain, and does not
justify replacing the benchmark or claiming a general Vec3 performance win.

The ordinary-class API control exposes the cost avoided by native promotion,
while the smaller boundary controls quantify costs still paid when values
escape local registers. For one million outer iterations their medians are:

| Boundary control | Instructions M | Cycles M | Task-clock ms |
|---|---:|---:|---:|
| Eight local updates | 19.625 | 48.841 | 11.46 |
| Two owned calls | 257.625 | 223.409 | 47.70 |
| Two shared calls | 243.625 | 206.173 | 44.04 |
| Array materialization between groups | 161.625 | 183.969 | 39.46 |

These are complete-program costs, including retained callees, and explain why
register-local instruction improvements cannot be applied to arbitrary call
or collection-heavy code. List-specific throughput and nested clist layout
remain unqualified. Measurements cover one host and compiler configuration;
raw sample variation is retained rather than removed.

## Correctness and public-boundary costs

Preparation compares all 35 state fields plus both energies bit-for-bit through
printed round-trip `f64` values, at -1, 0, 1, 2, 3, 10, 1000, 10000 and one
million steps, across ten implementations/profiles: 90 complete-state cases.
Masses and nonpositive trip behavior participate in those comparisons. Four
boundary controls also agree exactly at eight trip counts (32 cases).

Boundary controls perform the same eight updates locally, through two retained
owned calls, through two retained shared calls, or with an array read/write
between each four updates. Their loop counts are respectively 19, 59, 65 and
175 instructions; call bodies add work beyond the loop count. These probes
expose the cost of ordinary ABI/storage boundaries rather than claiming that
all vector values remain packed. Native promotion does not change their
ownership contract.

The regression tests compare scalar, default optimized and AVX2 execution for
arithmetic, wrappers, collections, mutable fields, callable arguments, signed
zero, subnormals, infinity and ordered reduction. A diagnostic test preserves
owner/component loan exclusion. A dump regression ensures wide negation does
not silently rebuild the function narrow. The existing SIMD regression suite
also runs.

`source-fenv.plenty` is compiled as a real baseline-CPU Plenty shared library;
`source-fenv.c` checks positive/negative infinite scaling and cancellation in
four rounding modes (12 cases), including absence of a padding-induced invalid
exception. This does not qualify AVX2 floating environment behavior, trapping
exception order, NaN payload bits, arbitrary pointer-width boundaries or other
architectures. Existing paired native experiment checks remain separate
evidence, not substitutes for source-level tests.

## Reproduction and retained evidence

Build the release compiler, then separate preparation from controlled timing:

```sh
RUSTC_WRAPPER= cargo build --locked --release --bin plenty
python3 -B experiments/declared-vectors/source_measure.py
python3 -B experiments/declared-vectors/source_measure.py --measure --core 3
```

Preparation disables compilation caches and saves generated Plenty/C source,
assembly/optimized CLIF, compiler/source/executable hashes and correctness cases
under `target/vec3-source/final/`. Measurement verifies executable hashes, rotates
program order through nine pinned-core samples, and retains every perf counter
sample plus medians. It measures instructions, cycles and task-clock; no build
or other suite should run concurrently. `preparation.json` and
`measurements.json` link the exact source recipes and executable identities.
The source commit field names the branch base when preparation precedes the
final commit; individual source and compiler hashes identify the actual code.

The floating-environment check can be repeated with:

```sh
PLENTY_CACHE=off target/release/plenty --shared-library experiments/declared-vectors/source-fenv.plenty --library-name vec -o target/vec3-source/libvec.so
gcc -O2 -frounding-math -ffp-contract=off experiments/declared-vectors/source-fenv.c -ldl -lm -o target/vec3-source/verify-fenv
target/vec3-source/verify-fenv target/vec3-source/libvec.so
```

The source-level prototype does not adopt a benchmark rewrite, compact layout,
a runtime ISA dispatcher or a blanket speed guarantee. Broader workload,
call/collection and cross-target qualification remain in issue #967.
