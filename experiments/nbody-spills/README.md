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
