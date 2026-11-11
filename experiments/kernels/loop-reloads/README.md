# Values reloaded on every pass of a loop (issue #939)

A value live across a call before a loop can end up in a stack slot outside
the loop and in a register at its uses inside. regalloc2 then loaded it
before the first use and handed it back to the slot at the back edge, once
per iteration. The per-symbol loop of `random_fasta` in
`benchmarks/game/fasta/plenty/fasta.plenty` did this for two values.

The regalloc2 fork at `b2dc24b` (`docs/loop-extension.md` there) extends
such a value's register over the loop after allocation, when a register is
free for the whole loop. `Cargo.lock` pins it. The numbers below compare it
with the previous fork revision `a39b520`; they were taken with `b2dc24b`
as a path override before it was pinned.

## The rule

A value's register is extended over the innermost loop that has a use of
the value in a block dominating every backedge source of that loop; an
enclosing loop is then tried under the same test. Blocks without successors
(traps, returns) are never such a block and are left out of the extension.
A use on a conditional path inside a loop therefore buys nothing: the value
keeps its slot and is loaded at the use. The extension happens only when
one register is free for every stack-held range of the value in the loop.
The value's own register is tried first, then other registers the function
already uses.

## `random_fasta`, per symbol

| Compiler | Stack loads | Stack stores |
|---|---:|---:|
| Fork `a39b520` | 2 | 2 |
| Extension, own register only | 1 | 1 |
| Extension (`b2dc24b`) | 0 | 0 |

## benchmarks/game, user-mode instructions

`bench.py run --mode quick --repeats 1 --perf`, pinned to cores 0-7.

| Program | `a39b520` | `b2dc24b` | Change |
|---|---:|---:|---:|
| binary-trees | 7,194,693,752 | 7,194,693,435 | 0.00% |
| fannkuch-redux | 1,680,419,687 | 1,680,419,748 | 0.00% |
| fannkuch-redux parallel | 1,495,736,393 | 1,495,734,824 | 0.00% |
| fasta | 1,384,801,361 | 1,263,991,229 | −8.72% |
| k-nucleotide | 6,289,896,341 | 6,289,494,675 | −0.01% |
| k-nucleotide parallel | 6,284,512,467 | 6,291,409,378 | +0.11% |
| mandelbrot | 8,333,450,626 | 8,285,462,567 | −0.58% |
| n-body | 3,225,592,102 | 3,225,592,139 | 0.00% |
| n-body clist | 12,340,574,583 | 12,100,574,833 | −1.95% |
| n-body unchecked | 5,755,575,356 | 5,620,575,491 | −2.35% |
| regex-redux | 560,563,246 | 560,408,095 | −0.03% |
| reverse-complement | 488,928,971 | 488,928,970 | 0.00% |
| spectral-norm | 2,724,524,695 | 2,724,524,639 | 0.00% |

k-nucleotide's counts vary by ±0.1% between runs of one binary. Of its 78
functions, including `std`, two differ (`Formatter.unsigned` and
`Formatter.integer`, each one instruction shorter).

n-body at 500,000 steps: plain 322,877,520 → 322,877,453 (same machine
code), unchecked 575,860,425 → 562,360,453, clist 1,234,359,385 →
1,210,359,407.

## Other programs

* 100 programs under `examples/`, `benchmarks/` and `experiments/`: 27
  change; static instructions 363,925 → 363,814; 48 loops shorter, none
  longer. One program is statically larger
  (`experiments/regex/text-before.plenty`, +2 instructions, while its loop
  goes from 22 to 20).
* Plenty kernel programs here: `bytes-literals/kernels.plenty` 104,712,886 →
  86,281,187 instructions; the other 13 within ±400.
* CLIF kernels (20 files): 2,420 → 2,416 instructions; `read.clif`
  `%a_as_emitted` loop 10 → 7, `reverse.clif` `%reverse_h_shared_trap_block`
  17 → 14; the rest unchanged.

## Compile cost

`compile_bench 1000 10 --workload aot`, user-mode instructions:

| Build | Instructions |
|---|---:|
| `a39b520` | 125.21 bn |
| Extension | 126.31 bn (+0.9%) |
| Extension, pass switched off at run time | 125.92 bn |
| Extension, pass on, same binary | 126.29 bn (+0.3%) |

The pass itself costs 0.3%. The remaining difference between the two builds
is in Cranelift functions the change does not touch (different inlining).

## Not fixed

The extension takes free registers only. A cut-down `random_fasta` (the
same loops with a checksum call in place of `write_lines`, and the codes
read from a constant) still loads and stores the block's length once per
symbol: all 14 allocatable registers are in use in that loop, one of them
holding a second copy of the loop count.
