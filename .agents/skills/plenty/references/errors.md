# Errors

Plenty has no exceptions. A fallible operation returns `Result[T, E]`; absence
is `Option[T]`. Every `Result` must be consumed with `?`, `match`, a method
such as `unwrap_or` (when deliberately recovering), or an explicit `drop(result)`.

## Propagate with `?` and a union

`?` returns early with the error. The enclosing function's error type must be
the same type or a union containing it. Name the union once with an alias:

```plenty
type AppError = AllocError | IoError | ParseError | OverflowError[i64]

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
allocation-free: `AppError | UsageError` is one larger union. The addition in
`total` produces an ordinary `i64`; overflow returns `Err` from `total` through
its declared `Result`. The successful path still needs `Ok(sum)`.

## Arithmetic and callbacks

Each function and closure declares its own errors, independent of its caller.
Use a fallible callback consumer such as `try_fold` when accumulating integers:

```plenty
def add_square(total: i64, value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(total + value * value)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(iter(range(5)).try_fold(0, add_square)?)?
    print(add_square(i64.MAX, 1).is_err())?
    Ok(())
```
```output
30
True
```

An integer pipeline `.sum()` also returns `Result`; handle it with `?` or
`match`. Float arithmetic retains IEEE behavior. Use `checked:` to recover
locally from integer arithmetic rather than returning its error; its final
expression becomes the success value. Neither `checked:` nor `unchecked:`
changes a called function's declared contract. See
[checked arithmetic](../../../../book/src/design/language/03-types/numbers.md#checked-blocks)
and [local pipelines](../../../../book/src/design/language/09-iteration/06-pipelines.md).

## Unchecked arithmetic

Prefer declared arithmetic errors or local `checked:` recovery. When the
intended policy is termination, use `unchecked trap:`. Plain `unchecked:`
asserts that integer arithmetic preconditions hold; `--unchecked trap` checks
that assertion (the default), while `--unchecked assume` omits those checks
and makes violations undefined behavior. Neither mode means wrapping.
In assume mode, list, slice-view and `Bytes` indexing inside a plain
`unchecked:` suite also requires a nonnegative index below the collection
length: neither bounds checks nor negative-index normalization remain.
`unchecked trap:` retains normal negative indexing and bounds traps.
Optimization profiles do not select the mode. Other indexing, allocation,
floating-point arithmetic and called functions keep their own contracts;
`std` continues to check its unchecked suites. See
[unchecked blocks](../../../../book/src/design/language/03-types/numbers.md#unchecked-blocks)
for the full preconditions and lexical nesting rules.

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
                case Err(UsageError.Arguments):
                    2i32
                case Err(_):
                    1i32
        case Err(_):
            1i32
```
```output
hello, Ada
```

Match an enum member's variant directly with `case Err(UsageError.Arguments):`.
To bind a whole union member, match the error with `case name: Type:`; `_`
covers the rest.

To recover from some members and forward the others, end with
`case rest: B | C:`, naming exactly the members earlier cases leave. `rest`
has the smaller sum type, so no `case e: T: Err(e)` per member is needed:

```plenty
enum SettingError:
    Missing

def setting(text: &str) -> Result[u8, SettingError | ParseError | RangeError]:
    if text == "":
        return Err(SettingError.Missing)
    u8.try_from(i64.parse(text)?)

def setting_or_default(text: &str) -> Result[u8, ParseError | RangeError]:
    match setting(text):
        case Ok(value):
            Ok(value)
        case Err(error):
            match error:
                case SettingError.Missing:
                    Ok(8)
                case rest: ParseError | RangeError:
                    Err(rest)

def main() -> Result[(), IoError | ParseError | RangeError]:
    print(setting_or_default("")?)?
    Ok(())
```
```output
8
```

The compiler names the correct set when the written one differs. A member
covered by only some of its variants stays in the residual; one remaining
member is the ordinary `case rest: B:`. It works only at the case position of
an owned match: `match &error` cannot narrow, so bind `_ as rest` there.

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

## One application error with labels

`Context[E]` changes type at each layer and does not widen
(`Context[ParseError]` is not `Context[Base]`). For one application error
type with labels added at many layers, add a labelled class to the sum and a
`context` helper that wraps a plain member (`case rest: Base:`) or extends an
existing label list. Never flatten the cause into a message string:

```plenty
type Base = AllocError | IoError | ParseError

# Caps how many labels one error keeps, so a retry loop cannot grow it.
# Labels are normally literals; a formatted label adds its own length.
const FRAMES: i64 = 8

# A typed cause with the innermost operation and later labels, innermost
# first. Wrapping allocates nothing; the label list is made on first use. A
# label past FRAMES, or one without memory to store it, is only counted.
class Trace:
    pub cause: Base
    pub operation: str
    pub outer: Option[list[str]]
    pub omitted: i64

    def add(self: &mut Trace, operation: str) -> ():
        if self.kept() < FRAMES:
            match &mut self.outer:
                case Some(labels):
                    if labels.append(operation).is_ok():
                        return
                case Nothing:
                    match [operation]:
                        case Ok(labels):
                            self.outer = Some(labels)
                            return
                        case Err(_):
                            pass
        if self.omitted < i64.MAX:
            unchecked:
                self.omitted += 1

    def kept(self) -> i64:
        match &self.outer:
            case Some(labels):
                len(labels)
            case Nothing:
                0

    # Outermost first, as nested `Context` descriptions read.
    def describe(self) -> Result[str, AllocError]:
        mut text = self.operation.concat(": ")?.concat(&str.repr(self.cause)?)?
        match &self.outer:
            case Some(labels):
                for label in labels:
                    text = label.concat(": ")?.concat(&text)?
            case Nothing:
                pass
        if self.omitted > 0:
            text = f"[{self.omitted} omitted] {text}"?
        Ok(text)

type Error = Base | Trace

def context[T](result: Result[T, Error], operation: str) -> Result[T, Error]:
    match result:
        case Ok(value):
            Ok(value)
        case Err(error):
            match error:
                case trace: Trace:
                    mut traced = trace
                    traced.add(operation)
                    Err(traced)
                case rest: Base:
                    Err(Trace(rest, operation, Nothing, 0))

def load(text: &str) -> Result[i64, Error]:
    context(context(i64.parse(text), "parse limit"), "load settings")

def main() -> Result[(), Error]:
    for text in ["12", "ten"]?:
        match load(text):
            case Ok(limit):
                print(limit)?
            case Err(error):
                match error:
                    case trace: Trace:
                        match &trace.cause:
                            case ParseError.Invalid:
                                print(trace.describe()?)?
                                print("using 10")?
                            case _:
                                return Err(trace)
                    case rest: Base:
                        return Err(rest)
    Ok(())
```
```output
12
load settings: parse limit: ParseError.Invalid
using 10
```

Recover only from the causes you expect and forward the rest. Wrapping
allocates nothing; a label that cannot be stored (no memory, or past `FRAMES`,
a cap on the label count, not length) is counted in `omitted` and the typed
cause is kept. See
[the reference](../../../../book/src/design/stdlib/error.md#one-application-error-with-context).

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

## Eager and lazy defaults

Both `Option` and `Result` have `unwrap_or(default)` and
`unwrap_or_else(callback)`. `unwrap_or` evaluates the default even on success:
`option.unwrap_or(compute()?)` can propagate `compute`'s error when the option
is `Some`. The lazy callback takes no arguments for Option, or the error for
Result. Option's `ok_or_else` constructs an absence error lazily.

For a fallible default, use `option.map_or_else(fallible_default, Ok)?`:
both branches return a Result, and `?` runs outside the callbacks. `map_or`
has an eager default; `map_or_else` calls only one branch. Callback creation
and capture evaluation are still eager. Owned one-shot callbacks are accepted;
ordinary restrictions on borrowed environments still apply. Use `match` when
it is clearer or a callback cannot be passed by value. Do not mechanically
rewrite a lazy match into eager `unwrap_or` or `ok_or`.

`expect(message)` on either sum deliberately traps with that eagerly evaluated
message and the call location; it does not print the error payload. Prefer
propagation or recovery for ordinary failures. See the executable examples and
contracts in [standard sums](../../../../book/src/design/language/08-errors/01-sums-and-propagation.md#defaults-and-recovery).
