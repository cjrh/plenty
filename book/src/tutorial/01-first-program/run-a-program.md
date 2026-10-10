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

From the repository, run `cargo run -- hello.plenty`. If you have an installed
compiler, use `plenty hello.plenty`. A run compiles your file to a native
executable and then runs it.

Execution begins at `main`. Indentation groups its statements; use four spaces.
`print` prints one value followed by a newline. Put executable statements inside
functions.

Printing can fail. The `?` after each call continues when printing succeeds and
returns an output error when it fails. `Ok(())` reports that the program
finished successfully. For now, keep this pattern around your own printing
experiments. [Absence and failure](../03-absence/index.md) explains each piece.

Every example in this guide is a separate complete program. Copy an example
into a new file to run it; earlier examples do not supply hidden definitions.
Examples with labelled companion modules need those files too, on the same
page as the program.
