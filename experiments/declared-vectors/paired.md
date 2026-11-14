# Adjacent-magnitude pairing continuation (issue #967)

This continuation integrates the existing adjacent-distance magnitude pairing
with the earlier native vector-state candidate. It changes no production
compiler, public API, collection layout or benchmark. The original negative
result remains in [README.md](README.md). The new result supports this native
candidate on this host; adopting a public vector type still requires review
and source-level layout/collection qualification.

## Matched experiment

The generator retains all six original CLIF files byte for byte. New paired
forms batch two independent ordered distance calculations, use both lanes of
`sqrtpd`/`divpd`, then perform velocity updates in the original pair order.
Each dot remains `(x*x + y*y) + z*z`; no FMA, approximation or reassociation is
introduced. Inactive spatial lanes remain zero, multiplier padding is one,
and boundary loads/stores cover exactly three components. The signed positive
trip guard precedes state access. This is a five-body, 35-double diagnostic C
boundary, not a public vector ABI.

Release compiler source: `463ef429` (production Cranelift `98c03730582a07fe57c027f0e18e821040c68b8c`),
GCC strict FP flags retained in the manifests. Linux x86-64, AMD Zen 4,
physical core 3, no competing builds/tests, 1,000,000 steps. Each executable
was verified before measurement. Three instruction rounds and nine cycle
rounds used rotated/reversed order and zero-step controls; all raw samples,
including outliers, are retained under `results/`.

| Native form / current control | Instructions/step | Median cycles, millions |
|---|---:|---:|
| Baseline scalar | 639 | 222.532 |
| Baseline scalar paired | 641 | 220.130 |
| Baseline halves | 560 | 209.480 |
| Baseline halves paired | 550 | 205.940 |
| Current baseline Plenty | 473 | 179.455 |
| AVX2 scalar | 490 | 219.676 |
| AVX2 scalar paired | 486 | 200.256 |
| AVX2 halves | 425 | 195.010 |
| AVX2 halves paired | 413 | 179.976 |
| AVX2 wide | 299 | 180.988 |
| AVX2 wide paired | 293 | 167.660 |
| Current v3 Plenty | 395 | 179.161 |
| Current v3 explicit SIMD xy | 384 | 195.298 |
| v3 C staged / fused | 615 / 422 | 204.619 / 221.556 |

Instruction counts subtract the zero-step process control and divide by steps;
cycles are whole-process medians. Native wide pairing removes 30 scalar FP
and 10 zeroing instructions, adding 15 packed FP, 10 rearrangements, 5 stack
accesses and 4 copies: net **299 to 293**. Five packed square roots/divisions
replace ten scalar operations each. The ~6.42% cycle reduction against current
v3 Plenty includes native-state machinery differences; comparing the paired
and unpaired native forms isolates pairing under the same boundary. Baseline
halves remain slower than current baseline Plenty: no portable speedup claim.

## Placement sensitivity

A separate session linked the *unchanged* native byte blobs at eight function
entry phases modulo 64, confirmed with linked symbol addresses. Only candidate
placement varies; the current Plenty executable stays fixed. Simultaneous
instruction/cycle/task-clock counters retain raw perf CSV and commands.

| Entry phase | Wide cycles M | Paired cycles M | Paired task-clock ms |
|---|---:|---:|---:|
| 0 | 181.068 | 167.480 | 36.70 |
| 8 | 181.595 | 167.620 | 36.38 |
| 16 | 181.067 | 167.707 | 36.06 |
| 24 | 181.383 | 167.638 | 36.28 |
| 32 | 181.008 | 167.989 | 36.33 |
| 40 | 182.314 | 168.159 | 36.62 |
| 48 | 181.174 | 167.875 | 36.93 |
| 56 | 181.164 | 167.684 | 35.89 |

Current Plenty: **179.327M cycles**, **38.65ms task-clock**. All eight paired
medians improve cycles by **6.23–6.61%**; unpaired medians lose 0.94–1.67%.
A paired phase-8 sample reached 203.966M cycles and is retained, so the sample
ranges are not all disjoint. Task-clock is a CPU counter; the recorded duration
of invoking perf is separate and is not application CPU time. Startup differs:
the current Plenty zero-step median is 0.839M cycles / 1.64ms task-clock.
Raw zero-step controls are supplied; these are not exact isolated-loop timings.
This checks candidate placement on one machine, not universal alignment or
cross-target robustness.

## Safety and evidence limits

Ten native variants each pass 1,536 cases across four rounding modes, including
finite random states, infinities, signed zero, subnormals and NaNs. Sixteen
placed variants add 24,576 cases. All 35 fields are checked, with immutable mass,
a prefix canary and an adjacent protected page; the state is intentionally
unaligned to 16 bytes. Nonpositive trips accept NULL without access. Logical
finite results are bit-exact; NaNs compare by classification, not payload.
The verifier compares final sticky exception flags under masked exceptions;
it does **not** promise preservation of trapping exception order. Pairing moves
independent calculations earlier, so that distinction matters.

Full output checks compare all 35 state fields plus both energies for -1, 0,
1, 2, 3, 1000 and 1M steps: 126 matched checks plus 119 placement checks.
The native gate is positive for this normal-orbit workload. Subnormal-heavy
whole-program performance, other hardware, Vec4, public constructor/access
costs, call boundaries and list/clist physical layout remain unqualified.
The old source array/domain-class materialization costs remain unchanged.

## Reproduce and provenance

Build the release compiler and harness using the commands in [README.md](README.md).
Then separate preparation from quiet measurement:

```sh
python3 -B experiments/declared-vectors/paired.py
python3 -B experiments/declared-vectors/paired.py --measure-only --core 3
python3 -B experiments/declared-vectors/placement.py
python3 -B experiments/declared-vectors/placement.py --measure-only --core 3
```

An AVX2 Linux x86-64 host and usable perf counters are required. Preparation
refuses incomplete/hash-mismatched artifacts; measurement requires the requested
step count to have passed output checks. `target/declared-vectors-paired/` and
`target/declared-vectors-placement/` retain raw code, disassembly, binaries,
verifier logs and outputs. Production pins are unchanged.

`results/paired-*` and `results/placement-*` preserve the original manifests and
samples. Original manifests intentionally keep absolute artifact paths and
hashes. Supplemental provenance adds imported generator hashes without
rewriting those frozen manifests. The two `*-driver.py` files are exact archived
collector snapshots, **not standalone runners from the results directory**:
the initial session counted events separately; the placement session collected
all three simultaneously. The current runner uses the latter collector and
adds complete generator provenance. Raw measurement data are not rewritten to
pretend that all sessions ran identical driver revisions.
