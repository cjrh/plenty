# Change and check a program

Change the greeting or the calculation in your file, then run it again. Each
run compiles the complete file and starts a fresh process. Plenty has no
interactive REPL.

Use `cargo run -- --check hello.plenty` to check syntax and types without
running the program. A check needs no linker. A run or native compilation needs
a `cc`-compatible linker driver installed.

To keep an executable:

```sh
cargo run -- --compile hello.plenty -o /tmp/hello-plenty
/tmp/hello-plenty
```

The executable can run without Plenty installed. The run command preserves the
program's input, output, and working directory and reports a failing exit
status when the program fails.

Parse and type errors prevent execution. A runtime error can happen after
earlier effects, such as printing. Check a change first when you want feedback
without those effects. Use `print(value)?` inside `main` to inspect a value.
A function's final expression supplies its return value, so it must have the
declared type.
