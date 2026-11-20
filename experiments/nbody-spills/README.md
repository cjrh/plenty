# n-body data movement (issue #930)

Tools and results for attributing and reducing the data movement in the
expanded n-body step. All counts are per step on x86-64.

| File | Use |
|---|---|
| `ledger.py` | Classify a loop's instructions from `objdump -d --no-show-raw-insn` or `plenty --dump asm`; count reloads a memory operand or a retargeted load could replace |
| `measure.py` | Interleaved `perf stat -r` rounds on one pinned core; records load average |
| `layouts.sh` | Build eight binaries with `advance` at different addresses with two compilers and measure them |
| `ir.py` | Generate hand-written CLIF kernels: scalar, and magnitudes packed by tree, statement or operation |
| `peephole.py` | Rewrite an allocated kernel listing: `--fold` reloads into the next instruction's memory operand, `--copies` retargets a reload that feeds a copy, `--scalar` uses 8-byte spill moves |
| `nbody_f64x4.plenty` | `experiments/nbody-rsqrt/nbody_exact.plenty` with an `f64x4` magnitude loop before the `f64x2` one; measured through `NBODY_SOURCE` for the narrow-spill prototype (issue #969, not adopted) |
| `results.json` | Final session: compilers before and after, `std.simd` and list-of-class sources, gcc, clang, Rust, hand-written kernels |
| `results-fold.json` | Fork prototypes and peephole variants |
| `cranelift-any-use-fold.patch`, `regalloc2-register-first.patch` | The fork prototypes measured in `results-fold.json`; they are slower and are not proposed |

## Results

`results.json` (load average 2.5 to 2.8):

| Build | Instructions | Cycles | ms per 5M steps |
|---|---:|---:|---:|
| Plenty `02f2e245` | 647 | 236.4 | 256 |
| Plenty, paired statements | 652 | 220.0 | 236 |
| `std.simd` magnitudes, before and after | 621 | 214 | 230 |
| List of class, before and after | 1,573 | 377 | 408 |
| gcc 16.2 `-O3`, vectorization off | 549 | 221.5 | 239 |
| clang 22.1 `-O3`, vectorization off | 495 | 215.0 | 233 |
| Rust | 451 | 158.0 | 171 |
| CLIF kernel, scalar | 645 | 230.1 | 249 |
| CLIF kernel, magnitudes packed by statement | 634 | 221.5 | 239 |
| CLIF kernel, `x`/`y` pairs kept as vectors | 488 | 187.2 | 203 |

`results-fold.json` (load average 2.0 to 2.2; the two Plenty binaries ran
4% to 6% slower in this session than in the one above, the kernels did not):

| Build | Instructions | Cycles |
|---|---:|---:|
| Plenty `02f2e245` | 647 | 246.5 |
| Plenty, paired statements | 652 | 233.8 |
| `02f2e245` with both fork prototypes | 592 | 287.9 |
| Paired statements with both fork prototypes | 648 | 266.1 |
| Allocated scalar kernel, unchanged | 645 | 231.0 |
| `peephole.py --fold` | 612 | 220.1 |
| `peephole.py --copies` | 602 | 226.4 |
| `peephole.py --fold --copies` | 569 | 213.3 |
| `peephole.py --fold --scalar` | 612 | 218.8 |

Eight layouts (`layouts.sh`, `advance` at 0x213e00 to 0x213f90): 236.0 to 238.5
cycles before, 220.5 to 225.1 after.

## Fork prototypes

The Cranelift patch gives read-only scalar `xmm_m32`/`xmm_m64` operands an
`Any` early-use constraint and rewrites an operand allocated to a spill slot
into an `rsp`-relative memory operand at emission. regalloc2 sends a bundle
whose uses are all `Any` straight to the stack; the regalloc2 patch tries a
register first. With both, values read only through foldable operands still
spend more time in memory, and the step is 17% slower with 55 fewer
instructions.

`peephole.py` changes no allocation. It removes a reload only when the next
instruction can read the slot and the register is dead afterwards. That is the
shape to implement in Cranelift's emission: fold a reload edit into the
following instruction's operand. Packed operands cannot fold without AVX or
16-byte-aligned slots, since SSE packed memory operands must be aligned and
spill slots sit 8 bytes off a 16-byte boundary.

## Current compiler and deferred work

Rechecked at `4465a43e` with Plenty 0.8.32, pinned Cranelift
`2986d9c583`, GCC 16.2.1, and Zen 4. The isolated
[velocity kernel](../nbody-codegen/README.md) has 58 instructions per
pair with proved body bounds and 69 on the fallback. GCC `-O2` uses
34. The guarded, expanded production step is a different path. It
contains no collection checks or per-pair collection address work.

| Unchanged source | CPU | Neither rewrite | Memory operands | Forwarding | Both |
|---|---|---:|---:|---:|---:|
| `list[Body]` | x86-64 | 500 | 469 | 500 | 471 |
| `clist[Body]` | x86-64 | 509 | 480 | 509 | 480 |
| `list[Body]` | x86-64-v3 | 420 | 392 | 420 | 393 |
| `clist[Body]` | x86-64-v3 | 424 | 392 | 424 | 393 |

The table counts instructions per guarded step. Static disassembly and
three pinned measurements of the difference between 200,000 and 2,200,000
steps agree to the nearest instruction. All 16 Plenty binaries print
the same energies as both staged C builds at 0, 1, 2, 1,000 and
1,000,000 steps. The C controls retire 734 instructions per step at
x86-64 and 615 at v3 with `-O2 -ffp-contract=off`. They retain the staged
algorithm and their own loop structure. A comparison of isolated
velocity loops answers a different question from those process counts.

With both rewrites enabled, the list step at x86-64 has 180 FP arithmetic
instructions (24 with stack operands), 93 explicit stack loads, 54
stores, 94 register copies, 40 lane rearrangements, five zeroing
instructions and five integer/control instructions. At v3 it has 180
FP arithmetic (23 with stack operands), 92 stack loads, 50 stores, 16
regular register copies, five three-operand scalar moves, 40 lane
rearrangements, five zeroing instructions and five integer/control
instructions. Forwarding can substitute a register copy for a stack
load at the same instruction count and can preempt a memory-operand
fold. These counts alone leave the earlier cycle-based choice of
default intact.

The retained two-operand ledger finds 37 load/copy pairs in the
x86-64 list step. Earlier retargeting of such loads saved instructions
without saving cycles. Its one remaining arithmetic candidate in this
step is a spill reload feeding the source of `subsd`. The legacy
read/write destination of the same instruction must remain a register.
A general fold across block boundaries would also have to prove the
register and spill-slot state on successor edges. The separate
[paired-vector CLIF probe](../declared-vectors/paired.md) can keep all
three coordinates in vector state. The current harness reproduces its
instruction counts and the earlier cycle result.

| Kernel / production control | Instructions per step | Median cycles per 1M steps |
|---|---:|---:|
| Default Plenty list | 471 | 177.97M |
| Baseline native halves, paired magnitudes | 550 | 207.67M |
| v3 Plenty list | 393 | 178.98M |
| AVX2 native wide | 299 | 183.39M |
| AVX2 native wide, paired magnitudes | 293 | 169.06M |

Cycles are nine interleaved process measurements on core 3. The ten
native kernels each pass 1,536 full-state comparisons against C,
and the paired runner checks 126 complete outputs across its native,
Plenty and C programs. The AVX2 wide paired kernel's 5.5% cycle gain
uses a private fixed-size native boundary. The source-level Vec3
attempt in local PR #1274 cut v3 instructions, tied v3 cycles and took
14.82% more cycles on baseline x86-64. The residual reload and packing
work is deferred.

To reproduce, build an optimized compiler with `cargo build --locked
--release --bin plenty`. Build the n-body sources under
`benchmarks/game/n-body/plenty/` using `--cpu x86-64` and
`--cpu x86-64-v3`, `--no-strip --compile`, and
`PLENTY_RELOAD_FOLD=off`, `operands`, `forwarding` or `both`.
Disable the cache with `PLENTY_CACHE=off`. Feed
`objdump -d --no-show-raw-insn` output to
`python3 -B experiments/nbody-spills/ledger.py --function __plenty_fn_advance`
and use `perf stat -e instructions:u` for the process counts. The staged
C build uses `-DKERNEL=staged` with `driver.c` and `kernels.c` under
`experiments/nbody-structural/`. For the general-size velocity kernel,
use the dump commands in the [original report](../nbody-codegen/README.md).
