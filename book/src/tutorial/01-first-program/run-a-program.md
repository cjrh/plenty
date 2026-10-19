# Run a program

Save this as `hello.plenty`:

```plenty
def main() -> Result[(), IoError]:
    print("Hello, Plenty!")?
    print(6 * 7)?
    Ok(())
```

```output
Hello, Plenty!
42
```

In the directory where you saved the file, run:

```sh
plenty hello.plenty
```

A run compiles your file to a native executable and then runs it. If your shell
cannot find `plenty`, follow [Setting up](../00-setting-up.md) first.

Execution begins at `main`. Indentation groups its statements; use four spaces.
`print` prints one value followed by a newline. Put executable statements inside
functions.

Printing can fail. The `?` after each call continues when printing succeeds and
returns an output error when it fails. `Ok(())` reports that the program
finished successfully. Keep this pattern for printing;
[Absence and failure](../03-absence/index.md) explains how to handle errors.

Each example is a complete program. Run it in a new file, along with any
labelled companion modules shown on its page.
