# Introduction

Plenty is a programming language that answers the question,
"Is it possible to build a language with Python syntax but Rust
performance and guarantees?".

Plenty is statically-typed and compiles ahead of time to native code.

## Why the name "Plenty"?

I wanted a language without trade-offs: simple syntax that is easy
to learn, easy to read and write, but also produces fast code, and
is safe and reliable, and with a huge standard library to reduce the
need for third-party dependencies. And more! I wanted everything that
is good about each programming language. The programmer's version of
"a land of plenty" where you can have it all.

## Goals

There are multiple goals for Plenty:

- **Convenience and safety**: Could we merge the best parts of Python and Rust? This is Plenty.
- **Explicit ownership**: Ownership and borrowing are explicit, with compile-time checks.
- **Sum types**: Enums are tagged unions and are used to represent sum types. Very similar to Rust.
- **Fast compilation**: The compiler is designed to compile quickly, even for large codebases.
- **Efficient**: An optimizing compiler generating machine code (Cranelift). SIMD support. Parallel tools like Rust's `rayon`. Exploit hardware features.
- **No garbage collector**: Deterministic memory usage patterns.
- **No dynamic behaviour**: Figure out as much as possible at compile time.
- **Fallible memory allocation**: All operations that allocate memory can report allocation failure without exceptions.
- **Simple FFI interop with C**: In both directions. There is an interface file format to specify lifetime information for resources from a C library so that Plenty can manage them safely.
- **No inheritance**: Classes and enums have fixed declarations rather than dynamic attributes or inheritance.
- **Generics and Protocols**: A native version of Python's `typing.Protocol`. Something like Go interfaces, or a simpler version of Rust's traits.
- **Batteries included**: A standard library so stuffed to the gills with awesome that many programs can be written using the standard library alone.
- **Simple package management**: No central package repo. Use golang-inspired git links for dependencies.

The primary influences on Plenty are:
- Python, for syntax
- Rust, for ownership and borrowing, and getting the details right
- Zig, for emphasizing fallible memory allocation

Plenty has features similar to Rust, but does not compete with Rust.
The Plenty compiler itself is written in Rust, and self-hosting the
compiler is not a goal. The Plentiful view is that Rust is appropriate
for some tasks, but a simpler language with different trade-offs can
be appropriate for others. Rust is a great language for writing a
compiler.

## Getting Started

This book provides:

- **[Learn Plenty](tutorial/index.md)** teaches the language with runnable
  examples. Start here.
- **[Reference](design/index.md)** defines language rules, built-in facilities,
  and compiler/runtime architecture in separate areas.

## Tested examples

`cargo test --test integration test_tutorial::` compiles and runs every
example in *Learn Plenty*. It compares the output with the expected output on
the page. An example that must fail must give the expected diagnostic.

## Build this book

```sh
mdbook serve book --open
```

