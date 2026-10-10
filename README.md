# Plenty

Plenty is a statically typed language with Python-shaped syntax, explicit
ownership, checked borrowing, and ahead-of-time native compilation through
Cranelift. Low memory use, fast compilation, and a small, understandable language
guide the design.

Start with [Learn Plenty](book/src/tutorial/index.md). The
[reference](book/src/design/index.md) separates language rules, built-in facilities,
and compiler/runtime architecture. Its [support index](book/src/design/toolchain/02-architecture/support.md)
links to implemented features and restrictions. Future work lives in the
[backlog](book/src/backlog.md); proposals explain design alternatives.

The book is an mdBook:

```sh
mdbook serve book --open
```

## Run a program

```python
type int = i64

def sum_to(n: int, total: int) -> int:
    """Sum the integers from 1 through n, using constant call-stack space."""
    if n == 0:
        total
    else:
        sum_to(n - 1, total + n)

def main() -> ():
    mut answer = sum_to(100, 0)
    answer = answer + 1
    print(answer).unwrap()
```

```sh
cargo run -- examples/sum.plenty
cargo run -- --check examples/sum.plenty
cargo run -- --compile examples/sum.plenty -o /tmp/plenty-sum
/tmp/plenty-sum
```

Running compiles into a private temporary directory, executes, and cleans up.
Executable builds need a `cc`-compatible linker driver; checking does not.
The [Rust runtime](plenty-runtime/README.md) is compiled and embedded when Plenty
is built, so installed compilers need no runtime source or Rust toolchain to
compile programs. Native emission currently supports `x86_64-unknown-linux-gnu`.
See [commands and linking](book/src/design/toolchain/01-building/index.md) for
objects, libraries, native driver options, and external build systems.

## Ownership and recoverable errors

```python
def main() -> Result[(), Failure]:
    squares: list[i64] = [n * n for n in range(10) if n % 2 == 0]?
    by_value: dict[i64, i64] = {n: n * n for n in &squares}?
    unique: set[i64] = set(copy(squares)?)?
    for n in &squares:
        print(n)?
    Ok(())
```

Owned collections move on assignment and owned argument passing. `copy` explicitly
duplicates contents; `&` and `&mut` borrow without transferring ownership.
Cleanup runs at scope exit or explicit `drop`. Allocating operations return
`Result`; `?` propagates errors with cleanup, while `unwrap` deliberately traps.
Classes, tuples, enums, ranges, generators, and closure environments use inline
construction; their component operations keep their own allocation contracts.

The compiler checks move availability, reference lifetimes, and conflicting
access. Bounds and integer arithmetic are checked at runtime. Concurrent workers
have restricted types and effects; the [concurrency reference](book/src/design/facilities/03-concurrency/index.md)
defines ownership transfer and shutdown. Trusted [C interfaces](book/src/design/facilities/04-interop/index.md)
place ABI, lifetime, and aliasing obligations on foreign code.

## Development

```sh
cargo test --workspace --all-features
cargo test --test test_tutorial
cargo clippy --workspace --all-targets -- -D warnings
```

The documentation test runs tutorial and supplementary examples and checks their
outputs and expected diagnostics. See [Development workflow](book/src/development/workflow.md)
for keeping implementation, reference, and lessons aligned, and
[compilation measurements](book/src/design/toolchain/02-architecture/emission.md)
for performance evidence. `AGENTS.md` and `CLAUDE.md` describe local issue claims,
worktrees, and review records.

Plenty has no interpreter, REPL, or JIT. Historical stack syntax remains behind
`--legacy` for backend regression coverage; it does not define the current language.
