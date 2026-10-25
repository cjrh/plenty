# Plenty

Plenty is a programming language and compiler.

Features:
- Syntax influenced by Python and Rust (and others)
- AOT native compilation through Cranelift
- Static typing with type inference
- Fallible memory allocation
- Explicit ownership and borrowing with compile-time checks
- No garbage collector

The goal of Plenty is to merge the best features of Python
and Rust, and to shamelessly copy good ideas from other languages.

We are not trying to invent anything groundbreaking but
what we have is plenty.

Read the [online documentation](https://cjrh.info/plenty/), or the
[development documentation](https://cjrh.info/plenty/dev/) for current `main`.
The default stays on development until the first stable release is admitted.

Start with [Learn Plenty](book/src/tutorial/index.md). The
[reference](book/src/design/index.md) separates language rules, built-in facilities,
the [standard library](book/src/design/stdlib/index.md), and compiler/runtime architecture. Its [support index](book/src/design/toolchain/02-architecture/support.md)
links to implemented features and restrictions. Future work lives in the
[backlog](book/src/backlog.md); proposals explain design alternatives.

The book is an mdBook; for a local preview:

```sh
mdbook serve book --open
```

## Install a release

Release archives for x86-64 Linux (glibc 2.28 or later) need no Rust toolchain:
extract one anywhere, add its `bin/` directory to `PATH`, and install a C
compiler for linking. See [installing a release](book/src/design/toolchain/01-building/installing.md);
maintainers, see [releasing Plenty](book/src/development/releasing.md).

## Run a program

Follow [Setting up](book/src/tutorial/00-setting-up.md) to install Plenty and add
it to your PATH. Save this program as `sum.plenty`:

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
plenty sum.plenty
plenty --check sum.plenty
plenty --compile sum.plenty -o /tmp/plenty-sum
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

## Compiler benchmarks

It's early days, but not too terrible. I set up and ran a benchmark from
https://github.com/nordlow/compiler-benchmark locally. The DMD compiler
is by far the fastest on this benchmark by like an order of magnitude faster
than the numbers below, but I don't have it installed so it is omitted.

 40,000 functions, three requested runs, caches disabled:

| Compiler | Build + run (µs/function) |
| --- | ---: |
| Clang 22.1.8 | 172 |
| Plenty 0.2.4 | 233 |
| Zig 0.16.0 | 269 |
| GCC 16.2.1 | 367 |
| Go 1.26.10 | 440 |

Plenty’s build took approximately 9.3 seconds, with 176 MB peak RSS.

Generated-code optimization stayed off. Only the non-generic Plenty workload is implemented.
All four adapter tests passed, including native-result verification.

## Development

```sh
cargo test --workspace --all-features
cargo test --test integration test_tutorial::
cargo clippy --workspace --all-targets -- -D warnings
```

The documentation test runs tutorial and supplementary examples and checks their
outputs and expected diagnostics. See [Development workflow](book/src/development/workflow.md)
for keeping implementation, reference, and lessons aligned, and
[compilation measurements](book/src/design/toolchain/02-architecture/emission.md)
for performance evidence. `AGENTS.md` and `CLAUDE.md` describe local issue claims,
worktrees, and review records.

Plenty has no interpreter, REPL, or JIT.

## License

Plenty is licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT), at your option.
