# Program entrypoints and native compilation

## Running and linking

Plenty compiles ahead of time. Running a file compiles a
temporary native executable, runs it, and removes it afterward. Both running
and compiling require a `cc`-compatible linker driver. The default is `cc` on
PATH; add `--linker clang` or `--linker /path/to/driver` to select another.
Repeat `--link-arg ARG` for extra native link arguments. Checking needs no linker.
The Rust runtime is packaged with Plenty; compiling Plenty programs needs
neither `cargo` nor `rustc`.

## Entrypoints and exit status

Execution starts by calling `main` once. Every binary application must declare
`main` with no parameters. It can return `()`, `i32`, `Result[(), E]`, or
`Result[i32, E]`. `Ok(())` produces exit status zero and `Ok(status)` uses that
status. An `Err` is reported on standard error, then dropped, and the program
exits with status one. For example, a `main` that returns the error from reading
a missing file prints
`error: main returned IoError.System(2): No such file or directory`.
The error is printed once, when `main` returns it, not at each `?` on the way.
The `()` form
finishes successfully with exit status zero. The `i32` form returns a process
exit status: zero means success, and nonzero means failure. Use an `i32` literal
such as `0i32` or `1i32`; return annotations also guide unsuffixed literals.

Keep executable statements inside functions. Module scope contains `def`,
`class`, `enum`, and `type` declarations, plus imports. Bindings inside `main`
are local to it.
The [modules part](../../../tutorial/07-modules/index.md) explains imports and `pub` visibility.

Owned locals are dropped before the program exits, including when `main` returns
a nonzero status. Operating systems
limit the range of observable exit statuses; use small nonnegative codes for
portable command-line programs.

You can also choose the successful process status explicitly:

```plenty
def main() -> Result[i32, IoError]:
    print("ready")?
    Ok(0i32)
```

```output
ready
```

Calling a function at module scope is an error, even if `main` is also declared:

```plenty-error
def main() -> ():
    print("ready").unwrap()

main()
```

```error
executable statements are not allowed at module scope
```

## Keeping an executable or linking externally

Use `--compile` when you want to keep the executable and run it repeatedly
without compiling again. The executable does not need Plenty installed.

For an external build system, use `plenty --emit-object hello.plenty -o hello.o`
and `plenty --emit-runtime runtime`. Link the object with
`runtime/libplenty_runtime.a` followed by the arguments in `runtime/link-args.txt`.
These two commands do not invoke a linker. They emit a complete application,
including its imports; ordinary `pub` functions are not C exports.
Native compilation currently supports `x86_64-unknown-linux-gnu`. Use
`plenty --print-target` to inspect the packaged target. An explicit
`--target TRIPLE` must match it; choosing a linker does not enable cross-compilation.
