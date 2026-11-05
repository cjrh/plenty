# Plenty

Plenty is a statically typed language with Python-inspired syntax that compiles to native code.
It draws inspiration from:

- Python (syntax and standard library)
- Rust (safety)
- Zig (allocation)
- Erlang (concurrency)
- Go (interfaces)
- D (fast compilation)

## Documentation

- [Install Plenty](book/src/design/toolchain/01-building/installing.md) and [Learn Plenty](book/src/tutorial/index.md)
- [Language and standard library reference](book/src/design/index.md)
- [Development workflow](book/src/development/workflow.md) and [backlog](book/src/backlog.md)
- [Online documentation](https://cjrh.info/plenty/) and [current development documentation](https://cjrh.info/plenty/dev/)

## Demo

Here's a small word counter:

```python
def main() -> Result[(), AllocError | IoError]:
    text = "small language big ideas small programs"
    mut counts: dict[str, i64] = {}?
    for word in text.split()?:
        counts.insert(word, counts.get(word, 0) + 1)?
    print(counts)?
    Ok(())
```

- Bindings are immutable unless declared `mut`.
- Allocation and I/O return errors explicitly; `?` propagates them.
- `AllocError | IoError` declares the possible error types.

Save it as `main.plenty` and compile a native executable:

```sh
plenty --compile main.plenty --release -o main
./main
```

Output:

```text
{"small": 2, "language": 1, "big": 1, "ideas": 1, "programs": 1}
```

Or run it directly in script mode:

```sh
plenty script main.plenty
```

Script mode caches the compiled executable for fast subsequent runs.
For an executable script, add `#!/usr/bin/env -S plenty script` as the first
line of `main.plenty`, then run:

```sh
chmod +x main.plenty
./main.plenty
```

Edit and run again; Plenty recompiles when needed.

## License

Plenty is licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT), at your option.
