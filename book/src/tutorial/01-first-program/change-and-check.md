# Change and check a program

Change the greeting or calculation, then run the file again. Each run starts
a fresh process.

Use `plenty --check hello.plenty` to check syntax and types without
running the program. A check needs no linker. A run or native compilation needs
a `cc`-compatible linker driver installed.

To keep an executable:

```sh
plenty --compile hello.plenty -o /tmp/hello-plenty
/tmp/hello-plenty
```

The executable can run without Plenty installed, on a compatible host system.
To ship an application directory instead:

```sh
plenty --compile hello.plenty --dist dist/hello
./dist/hello/hello
```

Choose a destination that does not already exist. This greeting needs only the
executable. Applications reaching optional native engines also get the exact
libraries and packaged notices they need, without development-machine lookup
paths. This is not a static or cross-platform build; see
[application distributions](../../design/toolchain/01-building/distributions.md)
for engine setup and platform requirements.

Parse and type errors prevent execution. A runtime error can happen after
earlier effects, such as printing. Use `--check` when you want feedback
without those effects, or `print(value)?` inside `main` to inspect a value.
