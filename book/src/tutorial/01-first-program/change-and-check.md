# Change and check a program

Change the greeting or calculation, then run the file again. Each run starts
a fresh process.

Use `cargo run -- --check hello.plenty` to check syntax and types without
running the program. A check needs no linker. A run or native compilation needs
a `cc`-compatible linker driver installed.

To keep an executable:

```sh
cargo run -- --compile hello.plenty -o /tmp/hello-plenty
/tmp/hello-plenty
```

The executable can run without Plenty installed.

Parse and type errors prevent execution. A runtime error can happen after
earlier effects, such as printing. Use `--check` when you want feedback
without those effects, or `print(value)?` inside `main` to inspect a value.
