# Errors

Plenty has no exceptions. A fallible operation returns `Result[T, E]`; absence
is `Option[T]`. Every `Result` must be consumed with `?`, `match`, a method
such as `unwrap_or`, or an explicit `drop(result)`.

## Propagate with `?` and a union

`?` returns early with the error. The enclosing function's error type must be
the same type or a union containing it. Name the union once with an alias:

```plenty
type AppError = AllocError | IoError | ParseError

def total(path: &str) -> Result[i64, AppError]:
    text = read_text(path)?
    mut sum = 0
    for line in text.split("\n")?:
        cleaned = line.strip()?
        if cleaned != "":
            sum += i64.parse(cleaned)?
    Ok(sum)

def main() -> Result[(), AppError]:
    write_text("numbers.txt", "10\n\n20\n")?
    print(total("numbers.txt")?)?
    Ok(())
```
```output
30
```

List only the errors the function can actually produce. Unions are flat and
allocation-free: `AppError | UsageError` is one larger union.

## Exit status

Returning `Err` from `main` writes the error to standard error and exits with
status 1. Do not print an error and then return `Ok(())`: the caller sees
success. Recover only when the program can genuinely continue.

To choose the status, return `i32` or `Result[i32, E]` from `main`. Keep the
work in a function that takes its inputs, so it is easy to test:

```plenty
enum UsageError:
    Arguments

type AppError = AllocError | IoError | UsageError

def run(arguments: &list[str]) -> Result[(), AppError]:
    if len(arguments) != 2:
        write_stderr("usage: greet NAME\n")?
        return Err(UsageError.Arguments)
    print("hello,", arguments[1])?
    Ok(())

def main() -> i32:
    # A real program passes `args()?`; its first element is the program name.
    match ["greet", "Ada"]:
        case Ok(arguments):
            match run(&arguments):
                case Ok(_):
                    0i32
                case Err(error):
                    match error:
                        case usage: UsageError:
                            2i32
                        case _:
                            1i32
        case Err(_):
            1i32
```
```output
hello, Ada
```

<!-- issue #575 -->
Match a union member by type with `case name: Type:`; `_` covers the rest.
A variant pattern such as `Err(UsageError.Arguments)` does not select a union
member.

## Add meaning with an enum and `map_error`

Wrap a cause when the caller needs to know what was being done. A variant
constructor written without arguments is a function from its payload:

```plenty
enum ConfigError:
    InvalidPort(ParseError)
    Missing

def port(text: &str) -> Result[u16, ConfigError | RangeError]:
    number = i64.parse(text).map_error(ConfigError.InvalidPort)?
    u16.try_from(number)

def main() -> Result[(), IoError]:
    match port("http"):
        case Ok(value):
            print(value)?
        case Err(error):
            print("bad port:", error)?
    Ok(())
```
```output
bad port: ConfigError.InvalidPort(ParseError.Invalid)
```

An application enum that holds lower-level causes stores them as payloads,
for example `Io(IoError)` and `Alloc(AllocError)`, and converts with
`.map_error(AppError.Io)?`. A plain union is usually simpler.

## Say which operation failed

`std.error.context` and `path_context` wrap an error with a label (and path),
keeping the typed cause in `.cause`:

```plenty
from std.error import Context, context

def parse_limit(text: &str) -> Result[i64, Context[ParseError]]:
    context(i64.parse(text), "parse limit")

def main() -> Result[(), AllocError | IoError]:
    match parse_limit("ten"):
        case Ok(value):
            print(value)?
        case Err(error):
            print(error.describe()?)?
    Ok(())
```
```output
parse limit: ParseError.Invalid
```

Returning a `Context[E]` from `main` prints its fields raw, including path
bytes. For a readable message, call `error.report()`, which writes the
description to standard error, and return an `i32` status as in
[exit status](#exit-status).

## Option

```plenty
def first_negative(values: &list[i64]) -> Option[i64]:
    for value in values:
        if *value < 0:
            return Some(*value)
    Nothing

def main() -> Result[(), AllocError | IoError | ParseError]:
    values = [3, -1, 4]?
    print(first_negative(&values).unwrap_or(0))?
    found = first_negative(&values).ok_or(ParseError.Invalid)?
    print(found)?
    Ok(())
```
```output
-1
-1
```

`unwrap()` terminates the program on `Err` or `Nothing`, skipping cleanup.
Use it only when termination is the intended policy.

## Avoid

- `Failure`: propagating into `Result[T, Failure]` discards the cause. It is a
  last resort, not a way to combine errors.
- `str` errors when a typed error exists: callers cannot match on text.
