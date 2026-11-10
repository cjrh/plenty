# Analyzing emitted code

Use this procedure to make generated programs faster: find the loop that
dominates a benchmark, compare its machine code with C's, and account for
every extra instruction before changing the compiler. Profiling the compiler
itself is covered in [Profiling the compiler](profiling.md).

The worked example is local PR #917, whose kernels, scripts and results are in
`experiments/kernels/` (see its `README.md`). It took fannkuch-redux from
3.16x to 1.69x C's instruction count.

## Ground rules

- **Measure optimized builds.** Builds are optimized by default; never time or
  count a `--fast-compile` build, which runs Cranelift at `opt_level=none` and
  skips call-site inlining.
- **Count instructions, not time.** `perf stat -e instructions:u` and
  per-pass counts read from assembly do not depend on machine load. Wall-clock
  numbers on a shared machine are noise; take them last, alternating builds in
  one session, and compare only within that session.
- **Pin parallel programs** to physical cores (`taskset -c 0-7` on the
  reference laptop), so a busy machine does not change thread placement.
- **Check outputs.** Every build in a comparison prints identical output.
- **Prove, never assume.** A bounds, overflow or index-normalization check can
  be removed only when the compiler proves at compile time that it cannot
  fail. The safety requirement is that no out-of-bounds access or unreported
  overflow can happen, not that every access runs a check. Checks that can
  fail keep their trap kind, position and message.

## Procedure

### 1. Find the hot loop

Count the whole program, then attribute by function:

```sh
plenty --no-strip --compile app.plenty -o /tmp/app
perf stat -e instructions:u taskset -c 0-7 /tmp/app ARGS
valgrind --tool=callgrind --callgrind-out-file=/tmp/cg.out /tmp/app SMALL_ARGS
callgrind_annotate /tmp/cg.out | head -30
```

`perf record` followed by `perf annotate` shows the hot instructions inside a
function. `--no-strip` keeps the symbols.

### 2. Isolate kernels

Write each hot loop as a standalone Plenty function and a C function that
does the same work with the same types, as `experiments/kernels/kernels.plenty`
and `kernels.c` do. Keep one loop per function, pass data as views or
pointers plus lengths, and give the C version no information Plenty's lacks
(for example, no `restrict` unless Plenty's borrows prove the same thing).

### 3. Get both disassemblies

```sh
gcc -O2 -fno-tree-loop-distribute-patterns -c kernels.c -o /tmp/kc.o
objdump -d --no-show-raw-insn /tmp/kc.o > /tmp/c.asm

plenty --dump ops,clif,clif-opt,asm kernels.plenty -o /tmp/k
```

`-fno-tree-loop-distribute-patterns` stops gcc replacing copy loops with
`memmove`, which would hide the loop. Use `--dump-filter NAME` to narrow the
dump. See [Inspecting generated code](../design/toolchain/02-architecture/dump.md)
for the stages; `clif-opt` and `asm` are what runs.

A dump keeps every function body, while a compiled program drops the
standalone bodies of functions inlined at every call site. Compare the
inlined code in its caller when that matters.

### 4. Count instructions per pass

`experiments/kernels/loops.py` reads either disassembly and prints, for each
innermost loop, the instructions one pass runs on its hot path. Its README
shows how to normalize `--dump asm` output for it. Record the counts in a
table: one row per kernel, one column per build.

### 5. Attribute every extra instruction

Line up the two hot paths and give each extra Plenty instruction one cause:

| Cause | Typical shape (x86-64) | Usual fix lives in |
|---|---|---|
| Bounds check | `cmp` + `jae` to a cold block | Plenty: prove the index in range, reuse a dominating check, hoist |
| Negative-index join | `lea` of `index + len` and a second `cmp` | Plenty: prove the index nonnegative |
| Overflow check | `add` + `jo` (`jb` unsigned) | Plenty: prove no overflow (loop counters, range facts) |
| Header or length reload | `mov` from the list owner each pass | Plenty: row caching across the loop |
| Register move | `mov reg, reg` with no other effect | Usually regalloc2; check whether a value passed to a cold call causes it |
| Spill or reload | `mov` to or from the stack | regalloc2, or too many live values from Plenty's IR |
| Loop layout | unconditional `jmp` back to the header | Plenty: loop rotation |
| Call overhead | `call` of a small helper | Plenty: inlining |
| View or address construction | `isplit`, base + offset each pass | Plenty: build once outside the loop |
| Useful work | Present in C too | Nothing |

Sum the causes per kernel. A cause that accounts for many instructions across
kernels is the next target.

### 6. Test IR shapes before changing the compiler

Copy the kernel's `clif-opt` into a `.clif` file and change one shape per
variant: rotation, check placement, trap form (`trapz` against a cold call),
what a cold block receives, address arithmetic, block parameters. Compile each
with Plenty's release flags and count it:

```sh
cargo build --release --manifest-path experiments/kernels/harness/Cargo.toml \
  --target-dir target/kernel-harness
target/kernel-harness/release/plenty-kernel-harness FILE.clif \
  | python3 experiments/kernels/loops.py
```

Record a variant table (letter, change, count). The best variant shows what
Cranelift can reach; the gap between it and C is the backend's part. The
[Cranelift IR reference](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md)
and the [ISLE reference](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/isle/docs/language-reference.md)
describe the IR and the lowering rules. For the version Plenty builds, read
the mid-end rules (`opts/*.isle`) and x64 lowering (`lower.isle`) in the
[fork's](workflow.md#cranelift-and-regalloc2-forks) `cranelift/codegen` source,
checked out as `../wasmtime` on branch `fork`.

### 7. Decide where the fix lives

- **Plenty's frontend or lowering:** facts that remove checks, IR shapes that
  Cranelift lowers better. Implement these.
- **Cranelift mid-end or lowering rules, regalloc2:** write the finding up
  with the variant that demonstrates it. Prefer a Plenty-side IR shape when
  one reaches the same code. Otherwise fix it in the project's fork, following
  [Cranelift and regalloc2 forks](workflow.md#cranelift-and-regalloc2-forks):
  the fork's own tests, Plenty's suite, and before/after counts for every
  benchmark program and for compile time, since an allocator or lowering
  change affects all generated code. Keep the write-up for an upstream
  report.

### 8. Implement and verify

- Add tests that fail on the old code: CLIF or asm dump tests asserting the
  check is gone or the shape changed, plus behavior tests for the cases that
  must still trap (negative, out of range, near `i64.MAX`).
- Recount the kernel table, then the whole program, then compile time:
  `compile_bench 1000 10 --workload aot` with `PLENTY_CACHE=off` and a few
  larger programs compiled with `--compile`, counted with
  `perf stat -e instructions:u`. Report run-to-run spread.
- Run the full suite.

### 9. Record

Put the kernel table, the attribution, the variant table and the
whole-program counts in the PR description. Keep kernels and scripts in
`experiments/` so the comparison can be rerun. File an issue for each
remaining cause you did not fix.

## Report template

```text
## Kernel: NAME (source: FILE:LINE)

| Build | Instructions per pass |
|---|---|
| C (gcc VERSION -O2) | |
| Plenty before (COMMIT) | |
| Plenty after (COMMIT) | |

| Cause | Before | After | Notes |
|---|---|---|---|
| Bounds checks | | | |
| Negative-index joins | | | |
| Overflow checks | | | |
| Header reloads | | | |
| Register moves | | | |
| Spills | | | |
| Loop layout | | | |
| Calls | | | |
| Useful work (also in C) | | | |

| Variant | IR change | Count |
|---|---|---|
| a | as dumped | |

Whole program (instructions:u): before, after, C.
Compile time (instructions:u): compile_bench aot, other programs; spread.
Fixed here: ... Upstream: ... Remaining issues: #...
```

## Pitfalls

- An unoptimized build makes every comparison wrong; check the dump index's
  `optimize` line.
- Cranelift GVN merges identical pure instructions, such as `iconst` and
  `stack_addr`, and hoists them to a dominator, so a variant that duplicates
  them may compile to the same code.
- A hot value passed to a cold call's arguments can add moves on the hot
  path, because regalloc2 keeps the value in the call's fixed argument
  register. PR #917 passes list-index failure operands through a stack slot
  for this reason.
- Iterating a collection directly (`for x in xs`) has no bounds check;
  indexing in a loop has one unless the compiler proves it unnecessary.
