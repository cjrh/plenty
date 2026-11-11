# Arithmetic proved by checker intervals

Loops whose arithmetic the checker's
[integer intervals](../../../book/src/design/language/03-types/intervals.md)
prove infallible (issue #952) and code generation's own
[value bounds](../../../book/src/design/toolchain/02-architecture/emission.md#value-bounds)
do not: a masked operand with a negative result, a widened `i32`, and a
negated remainder.

Optimized x86-64 Linux builds, Plenty before = `584acca1`. Counts are
instructions per pass from [`loops.py`](../loops.py); each function has an
entry copy and a rotated copy of its loop.

| Kernel | Before | After |
| --- | ---: | ---: |
| `masked` | 12 and 10 | 10 and 8 |
| `widened` | 11 and 9 | 9 and 7 |
| `negated` | 22 and 20 | 20 and 18 |

Each loses two `jo` branches. The signatures still declare `OverflowError`
so that one source compiles with both compilers; with intervals the functions
could return `()`.

```sh
cargo build --locked --release --bin plenty
target/release/plenty --release --dump asm \
    experiments/kernels/intervals/kernels.plenty -o /tmp/iv
python3 experiments/kernels/loops.py < /tmp/iv/asm.txt
```
