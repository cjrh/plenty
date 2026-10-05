# Plenty

Plenty is becoming a statically typed language with Python-shaped syntax,
expression-valued blocks, explicit mutability, and native compilation through
Cranelift. Fast compilation and a small, understandable language are primary
design goals.

This branch contains the first working slice of the new language. Structs,
sum types, ownership/borrowing, and generators are designed next steps, **not
implemented features**. The full contract and roadmap are in [DESIGN.md](DESIGN.md).

Start with [TUTORIAL.md](TUTORIAL.md) to learn the language through runnable
examples. Its code and expected diagnostics are tested as the compiler evolves.

```python
def sum_to(n: i64, total: i64) -> i64:
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
```

Plenty uses Cranelift AOT exclusively. Running a file compiles it into a
temporary executable, runs it, and removes it when execution finishes. Both
running and compiling require a C compiler named `cc` on PATH; `--check`
does not. Running with no arguments prints help.

There is no interpreter, REPL, or planned JIT backend.

Supported today:

- `def name(parameter: type, ...) -> type:` with required types, optional
  docstrings, forward references, and direct/mutual tail-call optimization.
- `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `bool`, `str`, and `()`
  for no return value. Sized literals such as `42u8` and explicit casts such
  as `i64(value)`. Arithmetic is checked; there are no implicit conversions.
- Transparent type aliases, such as `type Count = u32`. There is no built-in
  `int`; users may explicitly choose `type int = i32` or `type int = i64`.
  Floating-point types such as `f32` and `f64` are not implemented yet.
- Infix arithmetic and comparisons, `True`/`False`, short-circuit `and`/`or`,
  `not`, parentheses, and Python's `a if condition else b` expression.
- Indented `if`/`elif`/`else` blocks. A final expression supplies a block's
  result; continuing branches must agree. Conditions require `bool`.
- Early `return value` and unit `return`, including nested guard branches.
  Each return must match the declared result type. Returning branches do not
  participate in later joins; code after a guaranteed exit is rejected.
- Inferred bindings (`answer = 42`), annotated bindings (`answer: i64 = 42`),
  explicit mutable bindings (`mut answer = 42`), and reassignment.
- `print(value)` and `contains(haystack, needle)`. Strings support single,
  double, and triple quotes, UTF-8, and `\n`, `\r`, `\t`, quote/backslash escapes.
- Typed `list[T]`, `dict[K, V]`, and `set[T]`, including nested values, literals,
  indexing, membership, and independent-value updates through `mut` bindings.
- `for` loops over collections, strings, and lazy `range` values; list, dict,
  and set comprehensions with multiple iteration and filter clauses.
- `//` rounds toward negative infinity; `%` follows the divisor's sign. `/`
  is reserved for future floating-point support.

Collections and comprehensions use familiar syntax with fixed element types:

```python
squares: list[i64] = [n * n for n in range(10) if n % 2 == 0]
by_value: dict[i64, i64] = {n: n * n for n in squares}
unique: set[i64] = set(squares)
for n in squares:
    print(n)
```

Collections have independent values: changing a `mut` binding leaves copies
unchanged. Current updates copy outer storage and allocations last until process
exit; use comprehensions for bulk construction. General iterator protocols,
tuple unpacking, `items()`, and `break`/`continue` are not implemented yet.

Guard clauses can return early while the main path uses an implicit result:

```python
def clamp_low(value: i64, minimum: i64) -> i64:
    if value < minimum:
        return minimum
    value
```

There is no `None` or implicit nullable type. `()` describes successful
completion without a value; it is not a marker for a missing value.

Run `cargo test` for frontend diagnostics, native execution, deep
tail recursion, executable tutorial lessons, and historical backend regressions.
`cargo test --test test_tutorial` checks the learning guide specifically.
`cargo clippy --all-targets -- -D warnings` checks the Rust implementation.

The old stack syntax is available only through `--legacy` (before a filename
or `--compile`) and `compile_legacy_source_to_executable` while backend
regressions remain useful. It also uses AOT. Its [tutorial](docs/legacy-tutorial.md)
and [design](docs/legacy-design.md) are historical archives; they do not define
the new language or current execution modes.
The modern library entry points are `compile_source_to_executable` and
`check_source`; `check_source` and `--check` validate without executing code.

To measure compiler latency without adding a benchmark dependency:

```sh
cargo run --release --example compile_bench -- 1000 10
cargo run --release --example compile_bench -- 1000 10 --aot
```

The first number is the generated function count, the second is repetitions.
The harness warms up once and reports median check time; `--aot` additionally
measures the complete native compilation and link pipeline. Rust's own build
time and generated program execution are excluded.
