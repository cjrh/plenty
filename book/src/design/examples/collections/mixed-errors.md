# Preserve different error causes

Declare a sum of the errors an operation can return. Each `?` injects its
original error into that sum, preserving the variant and its payload:

```plenty
type BuildError = ParseError | AllocError | IoError

def work(text: str) -> Result[list[u8], BuildError]:
    n = u8.parse(text)?               # ParseError
    values = [n, n + 1u8]?           # AllocError
    print("built values")?          # IoError
    Ok(values)

def main() -> Result[(), AllocError | IoError]:
    print(str.repr(work("41"))?)?
    match work("invalid"):
        case Ok(values):
            print(values)?
        case Err(error):
            match error:
                case cause: ParseError:
                    print(cause == ParseError.Invalid)?
                case cause: AllocError:
                    print(cause)?
                case cause: IoError:
                    print(cause)?
    Ok(())

```
```output
built values
Result[list[u8], AllocError | IoError | ParseError].Ok([41, 42])
True
```

The alias names the combination; it does not wrap the errors or allocate.
Matching a member gives the original concrete value. `?` also widens an error
sum into a larger declared sum without dropping its cause. Normal scope cleanup
runs on the error path.

A `main` returning `Err` reports the error on standard error and exits with
status one. Use `?` for propagation and `match` for recovery. `.unwrap()` is an
explicit choice to terminate immediately on `Err` or `Nothing`, without normal
scope cleanup. Neither sums nor `?` catch runtime traps or turn `Option.Nothing`
into an error.

Builtin `Failure` discards the original cause. It is a last resort, for when
no precise error can be written; these errors combine without it. The
[error tutorial](../../../tutorial/03-absence/error-details.md) describes its
behavior.
