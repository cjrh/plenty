# Plenty

Plenty is becoming a statically typed language with Python-shaped syntax,
expression-valued blocks, explicit mutability, and native compilation through
Cranelift. Fast compilation and a small, understandable language are primary
design goals.

This branch contains the first working slice of the new language. Structs,
sum types, ownership/borrowing, and generators are designed next steps, **not
implemented features**. The full contract and roadmap are in [DESIGN.md](DESIGN.md).

```python
def sum_to(n: int, total: int) -> int:
    """Sum the integers from 1 through n, using constant call-stack space."""
    if n == 0:
        total
    else:
        sum_to(n - 1, total + n)

mut answer = sum_to(100, 0)
answer = answer + 1
print(answer)
```

Run and compile the example:

```sh
cargo run -- examples/sum.plenty
cargo run -- --check examples/sum.plenty
cargo run -- --compile examples/sum.plenty -o /tmp/plenty-sum
/tmp/plenty-sum
cargo run
```

Native linking requires `cc` on PATH. File execution and the REPL currently
use the interpreter; AOT compilation uses Cranelift. This checkout does not
contain a JIT backend.

The REPL accepts expressions with Enter and indented definitions with a blank
line. Ctrl-J (or Shift/Alt-Enter) forces submission; Ctrl-G opens `$EDITOR`;
Tab completes names. Functions persist across submissions. Local bindings
currently last for one submission, and function redefinition is rejected.

Supported today:

- `def name(parameter: type, ...) -> type:` with required types, optional
  docstrings, forward references, and direct/mutual tail-call optimization.
- `int` (an alias for `i64`), `i8`–`i64`, `u8`–`u64`, `bool`, `str`, and `()`
  for no return value. Sized literals such as `42u8` and explicit casts such
  as `i64(value)`. Arithmetic is checked; there are no implicit conversions.
- Infix arithmetic and comparisons, `True`/`False`, short-circuit `and`/`or`,
  `not`, parentheses, and Python's `a if condition else b` expression.
- Indented `if`/`elif`/`else` blocks. A final expression supplies a block's
  result; both branches must agree. Conditions require `bool`.
- Inferred bindings (`answer = 42`), annotated bindings (`answer: int = 42`),
  explicit mutable bindings (`mut answer = 42`), and reassignment.
- `print(value)` and `contains(haystack, needle)`. Strings support single,
  double, and triple quotes, UTF-8, and `\n`, `\r`, `\t`, quote/backslash escapes.
- `//` rounds toward negative infinity. `/` is reserved for future floating
  point support. A final `return value` is accepted; early returns and loops
  are not implemented yet. Use tail recursion for iteration in this slice.

There is no `None` or implicit nullable type. `()` describes successful
completion without a value; it is not a marker for a missing value.

Run `cargo test` for frontend diagnostics, interpreter/native parity, deep
tail recursion, and historical backend regressions. `cargo clippy --all-targets
-- -D warnings` checks the Rust implementation.

The old stack syntax is available only through `--legacy` (before a filename
or `--compile`) and the explicit `Vm::run_legacy` library API while backend
regressions remain useful. Its [tutorial](docs/legacy-tutorial.md) and
[design](docs/legacy-design.md) are archived; they do not define the new language.
The modern library entry points are `Vm::run` and
`compile_source_to_executable`; `check_source` and `--check` validate without
executing code.

To measure compiler latency without adding a benchmark dependency:

```sh
cargo run --release --example compile_bench -- 1000 10
cargo run --release --example compile_bench -- 1000 10 --aot
```

The first number is the generated function count, the second is repetitions.
The harness warms up once and reports median check time; `--aot` additionally
measures the complete native compilation and link pipeline. Rust's own build
time and generated program execution are excluded.
