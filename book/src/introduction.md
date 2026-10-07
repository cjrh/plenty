# Introduction

Plenty is a statically typed language with Python-shaped syntax. It compiles
ahead of time to native code through Cranelift. Its main goal is low memory use.

This book has three parts:

- **[Learn Plenty](tutorial/index.md)** teaches the language with runnable
  examples. Start here.
- **[Reference](design/index.md)** is the language contract and the compiler
  design.
- **[Proposals](proposals/index.md)** record the reasons for past and future
  design decisions. They are not the current contract.

## Build this book

```sh
mdbook serve book --open
```

## Tested examples

`cargo test --test test_tutorial` compiles and runs every example in
*Learn Plenty*. It compares the output with the expected output on the page.
An example that must fail must give the expected diagnostic.
