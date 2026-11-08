# Template delimiter scan (issue #881)

`scan.plenty` isolates the repeated `view[0]` comparison formerly used by
`std.template.quoted` and `include_expression`. Both functions use the same
compiler/runtime; only delimiter placement differs. Input is nonempty valid
UTF-8. This is a library fix, with no compiler or representation change.

The migration from `&str` to `&strview` exposed this cost on ASCII: owned strings
have a cached scalar count and an ASCII indexing shortcut, whereas view indexing
currently counts all scalars before locating index zero. Indexing inside the
loop is consequently quadratic. The fixed parser still validates short inputs
before reading the delimiter and keeps scalar iteration and all error checks.

Measured on x86-64 using the release compiler from this branch, Plenty 0.8.19,
Cranelift 0.137.0, and gcc 16.2.1 `-O2`:

| Kernel | Instructions per loop pass |
|---|---:|
| C cached delimiter | 15 |
| Plenty repeated index | 69 |
| Plenty cached delimiter | 39 |

These static counts use `../loops.py` and exclude callees. The repeated version
calls `plenty_op_598` (text-view indexing) on every pass; the cached version
calls it once before the loop. The 30-instruction reduction includes its call,
argument-frame setup, temporary string ownership, and the associated register
traffic. The full runtime scalar-count scan is additional work hidden by the
static call instruction. Remaining Plenty overhead includes runtime scalar
decoding, string equality/ownership, byte-length lookup, and loop bookkeeping;
the C comparison works directly on UTF-8 bytes. This comparison does not claim
equivalent runtime interfaces or C parity.

| Whole program, 32768 ASCII scalars | `instructions:u`, three-run mean |
|---|---:|
| Plenty repeated index | 740539240 |
| Plenty cached delimiter | 4723401 |
| C cached delimiter | 1047247 |

All outputs are `32768`; instruction spread was below 0.01%. The kernel shows
the eliminated repeated scan, not an end-to-end template-rendering speed ratio.
No compiler IR variants or compiler-throughput claim applies to this source
hoist. The operation-dump regression test additionally checks both actual
standard-library loops, and behavior tests cover Unicode, escapes and errors.

Reproduce from the repository root (use the compiler built in this worktree):

```sh
cargo build --locked --release --bin plenty
PLENTY_CACHE=off target/release/plenty --release --dump clif-opt,asm \
  experiments/kernels/template-quote/scan.plenty -o /tmp/template-scan
python3 experiments/kernels/loops.py < /tmp/template-scan/asm.txt
gcc -O2 -fno-tree-loop-distribute-patterns \
  experiments/kernels/template-quote/scan.c -o /tmp/template-scan-c
objdump -d --no-show-raw-insn /tmp/template-scan-c \
  | python3 experiments/kernels/loops.py
python3 - <<'PY'
from pathlib import Path
source = Path('experiments/kernels/template-quote/scan.plenty').read_text()
for name in ['before', 'after']:
    Path(f'/tmp/template-scan-{name}.plenty').write_text(source.replace(
        'print(before(text), after(text))?', f'print({name}(text))?'))
PY
PLENTY_CACHE=off target/release/plenty --release --compile \
  /tmp/template-scan-before.plenty -o /tmp/template-scan-before
PLENTY_CACHE=off target/release/plenty --release --compile \
  /tmp/template-scan-after.plenty -o /tmp/template-scan-after
perf stat -r 3 -e instructions:u /tmp/template-scan-before
perf stat -r 3 -e instructions:u /tmp/template-scan-after
perf stat -r 3 -e instructions:u /tmp/template-scan-c
```
