# Values reloaded on every pass of a loop (issues #939 and #947)

> The `--unchecked assume` build mode used in this record was removed. The
> measurements are historical; commands that pass `--unchecked` no longer run.

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

## All registers in use (issue #947)

The extension above takes free registers only. Two cut-down copies of
`random_fasta` (a checksum call in place of `write_lines`) kept stack loads
and stores in the per-symbol loop, where all 14 allocatable registers are
in use: `forwarded-count.plenty`, and `more-values.plenty` with one more
value carried through the loop.

Two candidate changes are on branch `claude/issue-947` of each fork. They
are not merged into the forks' `fork` branches and `Cargo.lock` does not pin
them; the numbers below were taken with both as path overrides. "Before"
is the pinned compiler.

* Cranelift `bc716584d5`: `remove_constant_phis` removes a block
  parameter whose arguments, other than itself, are all one value. The
  per-symbol loop held the block's length twice, once in such a parameter:
  SSA construction leaves one for a variable that a loop does not change
  when the loop body holds a join (here the `while limits[k] < r` scan).
* regalloc2 `85ec363` (`docs/loop-extension.md`, "Making room"):
  when no register is free for the whole loop, the extension moves unsplit
  values that hold a register where it is wanted to other registers free
  for them.

Per-symbol loop of `random_fasta`, static instructions and stack loads plus
stores:

| Program | Before | Parameter removal only | Both |
|---|---:|---:|---:|
| `forwarded-count.plenty` | 37, 5 | 30, 0 | 30, 0 |
| `more-values.plenty` | 40, 7 | 37, 5 | 37, 5 |
| `benchmarks/game/fasta` | 30, 0 | 33, 2 | 30, 0 |

Parameter removal alone made the benchmark worse: with two parameters
fewer the allocator's register choices shift (its probe order depends on
bundle numbers), the loop's temporaries land on different registers and
none is free for the whole loop. Making room restores it, which is why the
two changes go together.

User-mode instructions, one run each pinned to one core (`perf stat -e
instructions:u`), compilers built from the same Plenty source:

| Program | Before | Parameter removal only | Both | Change |
|---|---:|---:|---:|---:|
| `forwarded-count.plenty` 1000000 | 249,023,663 | | 243,023,657 | −2.41% |
| `more-values.plenty` 1000000 | 255,833,956 | | 246,833,850 | −3.52% |
| binary-trees 14 | 1,539,347,231 | 1,540,947,483 | 1,540,947,312 | +0.10% |
| fannkuch-redux 9 | 145,233,858 | 145,596,763 | 145,596,668 | +0.25% |
| fasta 1000000 | 417,705,803 | 441,706,029 | 417,705,450 | 0.00% |
| k-nucleotide | 2,340,029,386 | 2,329,917,930 | 2,323,735,642 | −0.70% |
| mandelbrot 1000 | 517,448,775 | 517,448,858 | 517,448,754 | 0.00% |
| n-body 500000 | 233,378,150 | 235,378,165 | 235,378,146 | +0.86% |
| n-body clist 500000 | 241,361,109 | 239,361,146 | 239,361,120 | −0.83% |
| regex-redux | 2,519,233,168 | 2,521,132,940 | 2,517,896,404 | −0.05% |
| reverse-complement | 49,138,920 | 49,138,913 | 49,138,913 | 0.00% |
| spectral-norm 500 | 171,279,121 | 171,279,056 | 171,279,033 | 0.00% |

k-nucleotide varies by a few tenths of a percent between runs of one
binary.

### n-body

Plain n-body executes 4 instructions more per step (about 467 before).
They are `xmm` moves and reloads in the straight-line body of `advance`.
Its CLIF differs only in removed `i64` block parameters, and making room
changes nothing in it. The cause is regalloc2's probe order: the register
it tries first for a bundle is offset by the bundle's number, and removing
values renumbers every later bundle, floating-point ones included. With
the offset shifted by each of 32 constants in a test build (100,000 steps):

| Compiler | Mean | Minimum |
|---|---:|---:|
| Before | 47.885 M | 46.97 M |
| Parameter removal only | 47.720 M | 46.97 M |
| Both | 47.720 M | 46.97 M |

The change is neutral for n-body's code on average; the unshifted order
that ships happens to fall 0.86% on the wrong side. fannkuch-redux and
binary-trees were not sampled this way.

### Compile cost

Compiler user-mode instructions for `--dump asm --dump-std`, pinned:

| Program | Before | Parameter removal only | Both |
|---|---:|---:|---:|
| fasta | 371,513,786 | 371,519,803 (0.00%) | 371,762,866 (+0.07%) |
| k-nucleotide | 437,401,181 | 437,619,885 (+0.05%) | 434,373,040 (−0.69%) |
| n-body | 493,958,407 | 494,232,846 (+0.06%) | 499,724,356 (+1.17%) |
| regex-redux | 1,300,319,446 | 1,300,147,535 (−0.01%) | 1,302,912,860 (+0.20%) |

n-body is the worst case found: many floating-point values in loops with
every `xmm` register in use, each of which now sweeps the registers twice.

## Not fixed by the candidates

* `more-values.plenty` still loads and stores two values per symbol. More
  values are used in the loop than there are registers, and a value live
  through the loop but not used in it keeps its register; splitting it
  around the loop is not attempted.
* A value reloaded in a loop and not changed there is still stored back.
* The probe-order sensitivity described under n-body.
