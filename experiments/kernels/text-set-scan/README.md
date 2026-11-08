# Text-view ASCII set-scan call overhead

Evidence for local issue #880. Widening `std.text.find_in` and `find_not_in`
to `&strview` changed their lowering from a narrow native helper to opcode 604.
The generic entry required a 96-byte scratch block. The fix passes both borrowed
ranges directly to `plenty_text_view_find_in`; its scalar scan is the existing
`aggregates::byte_scan::text_find_in` implementation. No Unicode policy, scan
algorithm, allocation, loan, effect or trap rule changes.

`scan.plenty` scans a selected UTF-8 range one million times and prints
`1000000`. `scan.c` models the same range arguments and Option result; its optional
main calls the same Rust scalar-scan helper. `scan.clif` isolates the old scratch
entry and proposed direct entry before implementing the compiler change.

## Native instruction evidence

x86-64 Linux, GCC 16.2.1, optimized release compiler and optimized generated
programs, `PLENTY_CACHE=off`. Counts are instructions rather than elapsed time.
The successful path of the standalone std wrapper is:

| Wrapper | Native instructions | Scratch bytes |
| --- | ---: | ---: |
| Before, main `962de018` | 26 | 96 |
| Handwritten direct CLIF variant | 10 | 0 |
| Implemented direct entry | 11 | 0 |
| C direct caller | 9 | 0 |

The fixed caller removes six argument stores, two result loads, scratch-address
and dispatcher-argument setup, and stack reservation/restoration. The remaining
C comparison differs in frame conventions and Option control-flow joins; the
handwritten CLIF variant returns from each branch whereas the Plenty source has
a shared return block. No index checks or useful scanning work are removed.

The existing `loops.py` reports 31 instructions in the Plenty main loop both
before and after; the C main loop has 12. That loop/body difference and Plenty's
existing caller-context tracking are outside this entry-point regression.

| Whole probe, 1,000,000 calls | User instructions, mean of 3 runs | Reported spread |
| --- | ---: | ---: |
| Plenty before | 272,605,631 | +/- 0.00% |
| Plenty after | 247,605,913 | +/- 0.00% |
| C using the same Rust scan | 168,701,022 | +/- 0.00% |

The Plenty reduction is 9.17%. The counted kernel compilation itself increased
from 186,330,734 instructions (+/- 0.71%) to 196,969,705 (+/- 0.63%). This is a
single small program; it establishes no broad compiler-speed improvement.
No `compile_bench` or larger-application instruction measurement was performed.

## Reproduce

Use an isolated target directory and build the release compiler. Set
`PLENTY_TARGET` to that directory and `PLENTY_COMPILER` to its `release/plenty`.
The compiler build also writes runtime archives below `release/build/plenty-*/out`.
Set `PLENTY_RUNTIME_OUT` to the current build's directory containing
`libplenty_library_runtime.a` and `library-runtime-link-args.txt`.

```sh
cargo build --locked --release --bin plenty --target-dir "$PLENTY_TARGET"
PLENTY_CACHE=off "$PLENTY_COMPILER" --release --dump asm,clif-opt --dump-std \
  --dump-filter std.text.find_in experiments/kernels/text-set-scan/scan.plenty \
  -o /tmp/text-scan-dump
PLENTY_CACHE=off "$PLENTY_COMPILER" --release --compile \
  experiments/kernels/text-set-scan/scan.plenty -o /tmp/text-scan
perf stat -r 3 -e instructions:u taskset -c 2 /tmp/text-scan
perf stat -r 3 -e instructions:u env PLENTY_CACHE=off "$PLENTY_COMPILER" \
  --release --compile experiments/kernels/text-set-scan/scan.plenty \
  -o /tmp/text-scan-counted

cargo build --locked --release --manifest-path experiments/kernels/harness/Cargo.toml \
  --target-dir "$PLENTY_TARGET"
"$PLENTY_TARGET/release/plenty-kernel-harness" \
  experiments/kernels/text-set-scan/scan.clif

gcc -O2 -fno-tree-loop-distribute-patterns -c \
  experiments/kernels/text-set-scan/scan.c -o /tmp/text-scan-c.o
objdump -d --no-show-raw-insn /tmp/text-scan-c.o
```

Link the optional C benchmark against the **library** runtime, which supplies
no application `main`. Keep its exact recorded native link arguments:

```sh
python3 - <<'PY'
import os
import pathlib
import shlex
import subprocess
out = pathlib.Path(os.environ['PLENTY_RUNTIME_OUT'])
subprocess.run([
    'gcc', '-O2', '-DSCAN_BENCHMARK',
    'experiments/kernels/text-set-scan/scan.c',
    str(out / 'libplenty_library_runtime.a'),
    *shlex.split((out / 'library-runtime-link-args.txt').read_text()),
    '-o', '/tmp/text-scan-c',
], check=True)
PY
perf stat -r 3 -e instructions:u taskset -c 2 /tmp/text-scan-c
```

For loop counts, dump only `main` from Plenty and pipe that asm file to
`experiments/kernels/loops.py`. For C use `objdump -d --disassemble=main
--no-show-raw-insn` before piping to the same script. The helper wrappers have
no loops, so their executed path counts are read directly from disassembly.
