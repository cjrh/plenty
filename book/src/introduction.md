# Introduction

Plenty is a statically typed language with Python-shaped syntax and
Rust-influenced behaviors. It compiles ahead of time to native code.

There are multiple goals for Plenty:

- **Convenience and safety**: Is it possible to merge the best parts of Python and Rust? This is Plenty.
- **Explicit ownership**: Ownership and borrowing are explicit, with compile-time checks.
- **Sum types**: Enums are tagged unions, and can be used to represent sum types. Very similar to Rust.
- **Fast compilation**: The compiler is designed to compile quickly, even for large codebases.
- **Efficient**: An optimizing compiler generating machine code (Cranelift). SIMD support. Parallel tools like Rust's `rayon`.
- **No garbage collector**: Deterministic memory usage patterns.
- **No dynamic behaviour**: Figure out as much as possible at compile time.
- **Fallible memory allocation**: All operations that allocate memory can report allocation failure without exceptions.
- **No inheritance***: Classes and enums have fixed declarations rather than dynamic attributes or inheritance.
- **Protocol-based interfaces**: A native version of Python's `typing.Protocol`. Something like a simpler version of Rust's traits.
- **Batteries included**: A standard library so stuffed to the gills with awesome that many programs can be written using the standard library alone.
- **Simple package management**: No central package repo. Use golang-inspired git links for dependencies

This book provides:

- **[Learn Plenty](tutorial/index.md)** teaches the language with runnable
  examples. Start here.
- **[Reference](design/index.md)** defines language rules, built-in facilities,
  and compiler/runtime architecture in separate areas.
- **[Proposals](proposals/index.md)** record the reasons for past and future
  design decisions. They are not the current contract.
- **[Backlog](backlog.md)** is the single maintained list of future work and
  proposed priorities. Start here when choosing the next task.

## Build this book

```sh
mdbook serve book --open
```

## Tested examples

`cargo test --test integration test_tutorial::` compiles and runs every
example in *Learn Plenty*. It compares the output with the expected output on
the page. An example that must fail must give the expected diagnostic.
