# 1. Run your first program

Save this in `hello.plenty`:

```plenty
def main() -> Result[(), IoError]:
    print("Hello, Plenty!")?
    print(6 * 7)?
    Ok(())
```

Output:

```output
Hello, Plenty!
42
```

From this repository, run it with:

```sh
cargo run -- hello.plenty
```

To check the program without running it, use `cargo run -- --check hello.plenty`.
To compile a native executable:

```sh
cargo run -- --compile hello.plenty -o /tmp/hello-plenty
/tmp/hello-plenty
```

Plenty always compiles ahead of time with Cranelift. Running a file compiles a
temporary native executable, runs it, and removes it afterward. Both running
and compiling require a `cc`-compatible linker driver. The default is `cc` on
PATH; add `--linker clang` or `--linker /path/to/driver` to select another.
Repeat `--link-arg ARG` for extra native link arguments. Checking needs no linker.
The Rust runtime is already packaged with Plenty; you do not need `cargo` or
`rustc` to compile Plenty programs.

`print` returns `Result[(), IoError]`: either successful completion (`Ok(())`) or
an output error. The trailing `?` continues on success, or returns the error
from `main` after cleaning up its local values. The final `Ok(())` explicitly
reports success. Lesson 19 explains results and recovery in detail.

Examples that combine different error types use `Result[(), Failure]`.
`Failure` means that the caller only needs to know whether the work succeeded:
each `?` drops the original error details and propagates a small, allocation-free
marker. Use a specific type such as `IoError` when the caller needs those details.

Execution starts by calling `main` once. Every binary application must declare
`main` with no parameters. It can return `()`, `i32`, `Result[(), E]`, or
`Result[i32, E]`. An `Err` produces exit status one after dropping its payload;
`Ok(())` produces zero and `Ok(status)` uses that status. Errors are not printed
automatically. Returning a Result lets `main` use `?` too. The `()` form
finishes successfully with exit status zero. The `i32` form returns a process
exit status: zero means success, and nonzero means failure. Use an `i32` literal
such as `0i32` or `1i32`; return annotations also guide unsuffixed literals.

Keep executable statements inside functions. Module scope contains `def`,
`class`, `enum`, and `type` declarations, plus imports. Bindings inside `main`
are local to it, so other functions receive values through typed parameters.
The modules chapter below explains imports and `pub` visibility.

Like other functions, `main` may return early, use a final expression, or call
functions declared later in the file. Its owned locals are dropped before the
program exits, including when it returns a nonzero status. Operating systems
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

Every Plenty example in this guide is a separate, complete program. You can
copy any example into a file without first running the earlier examples. In the
modules chapter, also save the companion files labelled with their filenames.

The tutorial test extracts every `plenty` and `plenty-error` block and its
preceding `plenty-file` companion modules directly from this document. It checks
successful output through both compile-and-run and an
explicitly compiled executable, and checks rejected examples for the expected
diagnostic without executing their effects. Standalone lesson sources and
generated Markdown are a proposed improvement to the authoring workflow.
