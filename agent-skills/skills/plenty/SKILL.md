---
name: plenty
description: Write, fix, and explain programs in the Plenty programming language (`.plenty` files, `plenty.toml` packages, the `plenty` compiler). Use whenever a task touches Plenty source, Plenty diagnostics, or the Plenty standard library (`std.*`). Plenty looks like Python but rejects many Python habits; read this before writing any Plenty code.
---

# Writing Plenty programs

Plenty is statically typed, with Python-shaped syntax, single ownership with
checked borrows, no exceptions, and no hidden allocation. It compiles ahead of
time to native code. Code that looks like correct Python is usually wrong
Plenty.

This skill describes Plenty @VERSION@. Run `plenty --version`; if the installed
compiler differs, install the matching skill release, and trust the compiler's
diagnostics over this text.

## Workflow

1. Before writing code, read [the cheat sheet](references/cheatsheet.md) and, for
   any `std` call, find its signature in
   [the signature index](references/std/signatures.md). Never guess an API: search
   the index for the name, then read the module page it links for ownership,
   allocation, and error contracts.
2. Write the program. Executable statements belong in a function; a bare
   top-level statement is an error.
3. Type-check: `plenty --check FILE` for one file, or `plenty check` inside a
   package. Fix every diagnostic, then check again. Diagnostics are precise;
   read the whole message, including its notes.
4. Run: `plenty FILE`, or `plenty run` / `plenty test` in a package.
   `plenty FILE ARGS` is rejected; to pass arguments, build with
   `plenty --compile FILE -o OUT` and run `./OUT ARGS`, or use
   `plenty run -- ARGS` in a package.

Use a package (`plenty new NAME`) for anything with dependencies, several
executables, or tests. See [workflow and packages](references/workflow.md).

## Rules that prevent most errors

- `main` returns `()`, `i32`, `Result[(), E]`, or `Result[i32, E]`. Use
  `def main() -> Result[(), AllocError | IoError]:` and end with `Ok(())`.
- Every `Result` must be used. `print(...)` returns one: write `print(x)?`.
- Collection literals, comprehensions, `append`, `insert`, `add`, string `+`,
  `split`, `copy`, and f-strings allocate, so they return `Result`: write
  `xs = [1, 2]?`, `(a + b)?`, `f"{x}"?`, `items.append(v)?`.
- `?` propagates `Err` or `Nothing`. The function's error type must equal the
  source's error or contain it in a union (`AllocError | IoError | ParseError`).
  `?` never turns an `Option` into a `Result`; use `.ok_or(error)?`.
- No exceptions, `None`, `try`, `raise`, or `lambda`. Use `Result[T, E]`,
  `Option[T]` (`Some(x)` / `Nothing`), and `def(x: i64) -> i64: x + 1`.
- Types are explicit on every parameter and result. `i64` and `f64`, not `int`
  and `float`. No implicit numeric widening; cast with `f64(n)`.
- Bindings are immutable. Declare `mut x = 0` once to reassign or mutate.
- Scalars, `str`, and `@copy` classes copy. Assigning or passing a
  collection, box, or other class instance moves it; the old name is then
  unusable. Borrow with `&x` (shared) or `&mut x`
  (exclusive), or copy explicitly with `copy(x)?`. Iterating `&list[T]`
  yields `&T`; dereference with `*value`. `for x in owner` moves `owner`;
  write `for x in &self.items` to borrow.
- Methods that mutate declare `self: &mut ClassName`; bare `self` is a shared
  borrow.
- `d[k] = v` only replaces an existing key; add one with `d.insert(k, v)?`.
- No `**`, slices, truthiness, chained comparisons, or inheritance. Use
  `x.pow(n)`, `xs.slice(a, b)?`, `len(xs) > 0`, `a < b and b < c`, and
  composition or protocols.
- `abs`, `min`, `max`, `sum` come from `std.math`. Standard modules need an
  explicit import; prelude names (`print`, `len`, `range`, `open`,
  `read_text`, `args`, ...) do not.

## Idioms

- Propagate errors to `main`. Returning `Err` from `main` reports the error and
  exits with status 1. A `Context[E]` error prints its raw fields that way;
  for a readable message, call `error.report()` in `main -> i32`. Printing an error and returning `Ok(())` reports
  success; do that only when the program genuinely recovers.
- Name error groups with an alias: `type AppError = AllocError | IoError | ParseError`.
  Define an enum only to add meaning, and map into it with `.map_error(...)`.
- Prefer borrowed parameters (`&str`, `&list[T]`) unless the function keeps the
  value. Avoid `copy` unless two owners are needed.
- Reserve capacity when the size is known: `list[i64].with_capacity(n)?`.

See [errors](references/errors.md), [ownership](references/ownership.md), and
[common diagnostics](references/diagnostics.md).

## References

| File | Use |
|---|---|
| [references/cheatsheet.md](references/cheatsheet.md) | Every everyday form, with the Python habits Plenty rejects |
| [references/std/signatures.md](references/std/signatures.md) | Every public `std` declaration, one line each |
| [references/std/index.md](references/std/index.md) | Module table; one page per module under `references/std/` |
| [references/builtins/index.md](references/builtins/index.md) | Prelude text, numbers, and collections |
| [references/io/index.md](references/io/index.md) | Prelude files, streams, arguments |
| [references/packages.md](references/packages.md) | `plenty.toml`, dependencies, project commands, tests |
| [references/workflow.md](references/workflow.md) | Commands and a package walkthrough |
| [references/errors.md](references/errors.md) | Error types, propagation, context, exit status |
| [references/ownership.md](references/ownership.md) | Moves, borrows, copies, allocation |
| [references/diagnostics.md](references/diagnostics.md) | Frequent compiler errors and their fixes |
