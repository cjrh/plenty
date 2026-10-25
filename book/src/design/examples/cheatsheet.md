# Syntax cheat sheet

A compact summary of everyday Plenty, for readers who already know Python. Every
program on this page runs as a test, so the forms shown are current. Each
section links to the reference, which remains authoritative. The
[tutorial](../../tutorial/index.md) teaches the same material in order, and the
[signature index](../stdlib/signatures.md) lists the standard library.

## Programs and printing

```plenty
def main() -> Result[(), AllocError | IoError]:
    # print returns Result[(), IoError]; `?` propagates the error.
    print("hello", 42)?
    # Collection literals allocate, so they return Result[_, AllocError].
    names = ["Ada", "Grace"]?
    print(len(names), names)?
    Ok(())
```
```output
hello 42
2 ["Ada", "Grace"]
```

Executable statements live in functions; module scope holds only declarations,
imports, and numeric `const` values. `main` returns `()`, `i32`,
`Result[(), E]`, or `Result[i32, E]`. A `Result` cannot be silently discarded:
use `?`, `match`, or `drop(result)`. See [entrypoint](../language/02-source/entrypoint.md),
[source files](../language/02-source/index.md), and [branches](../language/04-expressions/branches.md).

## Bindings and numbers

```plenty
import std.math as math

def main() -> Result[(), IoError | ParseError]:
    mut count = 0
    count += 1
    small = 7u8
    ratio: f64 = 1.5
    total = f64(count) + ratio
    print(count, small, total)?
    print(7 // 2, -7 // 2, 7 % 2, 7.0 / 2.0)?
    base = 2
    print(base.pow(10), (3).pow(2), math.abs(-4), math.max(3, 9))?
    print(i64.MAX, u8(200), i64.parse(" 12 ")?)?
    Ok(())
```
```output
1 7 2.5
3 -4 1 3.5
1024 9 4 9
9223372036854775807 200 12
```

- Bindings are immutable; `mut` declares a mutable one, once.
- There is no `int`: unsuffixed integers are `i64`, floats `f64`. Suffixes:
  `7u8`, `1.5f32`.
- Types never widen implicitly; cast with `f64(n)`, `u8(n)` (traps if out of
  range), `T.try_from(n)`, or `T.wrapping_from(n)`. `T.parse(text)` returns
  `Result[T, ParseError]`.
- `//` floors, `%` is integer-only, overflow traps. No `**`: use `x.pow(n)`.
- `abs`, `min`, `max`, `sum` come from [`std.math`](../stdlib/math.md).

See [numbers](../language/03-types/numbers.md) and
[assignment](../language/04-expressions/assignment.md).

## Text

```plenty
from std.fmt import FormatError

def main() -> Result[(), AllocError | IoError | FormatError]:
    name = "Ada"
    greeting = ("Hello, " + name)?
    print(greeting)?
    print(f"{name} has {len(name)} letters; pi is {f64.PI:.2f}"?)?
    words = "a,b,,c".split(",")?
    print(words, "-".join(&words)?, "  x ".strip()?)?
    print("hello".find("l"), "hello".startswith("he"), "ll" in "hello")?
    print(name[0], name[-1], "hello".slice(1, 3)?, str.from(42)?)?
    Ok(())
```
```output
Hello, Ada
Ada has 3 letters; pi is 3.14
["a", "b", "", "c"] a-b--c x
Option[i64].Some(2) True True
A a el 42
```

`str` is immutable. Operations that build text return
`Result[str, AllocError]`; `?` binds tighter than `+`, so write `(a + b)?`.
An f-string has type `Result[str, FormatError]`. `len` counts Unicode scalars;
`text.byte_len()` counts bytes. There is no slice syntax. See
[text](../facilities/01-builtins/text.md),
[building text](../facilities/01-builtins/text-building.md), and
[format strings](../language/04-expressions/format-strings.md).

## Collections

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut items = [3, 1, 2]?
    items.append(4)?
    items.sort()
    print(items, items[0], items[-1], items.get(10))?
    print(items.slice(1, 3)?, items.pop(), 2 in items)?

    mut counts: dict[str, i64] = {}?
    for word in "a b a".split()?:
        counts.insert(word, counts.get(word, 0) + 1)?
    counts["b"] = 5
    print(counts, counts.get("z"))?
    for word, count in counts.items():
        print(word, *count)?
    print(len(counts.keys()), "a" in counts.keys())?

    mut seen = {1, 2}?
    seen.add(2)?
    squares = [n * n for n in range(5) if n % 2 == 0]?
    print(len(seen), squares)?
    Ok(())
```
```output
[1, 2, 3, 4] 1 4 Option[i64].Nothing
[2, 3] Option[i64].Some(4) True
{"a": 2, "b": 5} Option[i64].Nothing
a 2
b 5
2 True
2 [0, 4, 16]
```

Displays, comprehensions, `append`, `insert`, and `add` can fail to allocate,
so they return `Result`. Empty displays need a type: `xs: list[i64] = []?`.
Indexing a missing element or key traps; `get` returns an `Option`.
`dict[key] = value` only replaces an existing key; `insert` adds one. See
[collections](../facilities/01-builtins/collections.md),
[lists](../facilities/01-builtins/lists.md), and
[dictionaries](../facilities/01-builtins/dictionaries.md).

## Control flow and matching

```plenty
def classify(n: i64) -> str:
    match n:
        case 0:
            "zero"
        case 1 | 2 | 3:
            "small"
        case _:
            "many"

def main() -> Result[(), IoError]:
    for i in range(0, 10, 4):
        print(i, classify(i))?
    mut n = 4
    while n > 0:
        n -= 1
        if n == 2:
            continue
        elif n == 0:
            break
        print("n", n)?
    parity = "even" if n % 2 == 0 else "odd"
    print(parity)?
    for index, letter in enumerate("hi"):
        print(index, letter)?
    Ok(())
```
```output
0 zero
4 many
8 many
n 3
n 1
even
0 h
1 i
```

A suite's final expression is its value, so `return` is only needed for early
exits. Conditions must be `bool`; there is no truthiness. Chained comparisons
and loop `else` are rejected. `match` works on enums, integers, `str`, and
`bool`, must be exhaustive, and has no guards or tuple patterns. See
[matching](../language/04-expressions/matching.md) and
[loops](../language/04-expressions/loops.md).

## Functions and closures

```plenty
def scale(value: i64, factor: i64 = 2) -> i64:
    value * factor

def increment(value: i64) -> i64:
    value + 1

def apply[F: Callable[[i64], i64]](f: &F, value: i64) -> i64:
    f(value)

def main() -> Result[(), IoError]:
    print(scale(5), scale(5, factor=10))?
    offset = 100
    shift = def [offset](value: i64) -> i64:
        value + offset
    print(apply(&increment, 1), apply(&shift, 1))?
    print(apply(&(def(value: i64) -> i64: value * 3), 2))?
    Ok(())
```
```output
10 50
2 101
6
```

Every parameter and result has a type. Anonymous functions are
`def(params) -> T:`; `lambda` is rejected. Captures are explicit:
`def [x, mut y, &z, &mut w](...)`. Accept callbacks as `F: Callable[...]`
with `&F`, which takes both named functions and closures. See
[signatures](../language/06-functions/01-signatures-and-calls.md),
[function values](../language/06-functions/02-function-values.md), and
[closures](../language/06-functions/03-closures.md).

## Classes and enums

For primitive-field classes, `clist[T]()` creates allocation-free empty
[columnar storage](../facilities/01-builtins/clists.md).
`items.append(row)` returns `Result[(), AppendError[T]]`; `items[i].field`
reads a scalar, `items.copy_row(i)` returns `Option[T]`, and
`items.column(T.field)` is a borrowed local pipeline yielding scalar values.
Whole-row and packed-field references are unsupported.

```plenty
class Counter:
    name: str
    hits: i64

    def bump(self: &mut Counter) -> ():
        self.hits += 1

    def report(self) -> i64:
        self.hits

enum Shape:
    Dot
    Circle(f64)
    Rect(f64, f64)

    def area(self) -> f64:
        match self:
            case Shape.Dot:
                0.0
            case Shape.Circle(radius):
                f64.PI * *radius * *radius
            case Shape.Rect(width, height):
                *width * *height

def main() -> Result[(), IoError]:
    mut counter = Counter("clicks", 0)
    counter.bump()
    counter.bump()
    print(counter.report(), counter)?
    print(Shape.Rect(2.0, 3.0).area(), Shape.Dot)?
    Ok(())
```
```output
2 Counter(name="clicks", hits=2)
6.0 Shape.Dot
```

Classes have typed fields and a generated positional constructor; there is no
inheritance. Bare `self` is a shared borrow; mutating methods declare
`self: &mut Class`. Variants are always qualified (`Shape.Dot`), and matching a
borrowed enum binds references, so payloads are dereferenced with `*`. See
[classes](../language/03-types/classes.md), [enums](../language/03-types/enums.md),
and [borrowed matching](../language/05-ownership/05-borrowed-matching.md).

## Option, Result, and errors

```plenty
def parse_pair(text: str) -> Result[(i64, i64), AllocError | ParseError]:
    parts = text.split(",")?
    if len(parts) != 2:
        return Err(ParseError.Invalid)
    Ok((i64.parse(parts[0])?, i64.parse(parts[1])?))

def first_even(values: &list[i64]) -> Option[i64]:
    for value in values:
        if *value % 2 == 0:
            return Some(*value)
    Nothing

def main() -> Result[(), AllocError | IoError | ParseError]:
    print(parse_pair("3,4")?)?
    match parse_pair("3"):
        case Ok(pair):
            print(pair)?
        case Err(error):
            print("bad input:", error)?
    values = [1, 3, 6]?
    print(first_even(&values), first_even(&values).unwrap_or(0))?
    print(first_even(&values).ok_or(ParseError.Invalid)?)?
    Ok(())
```
```output
(3, 4)
bad input: ParseError.Invalid
Option[i64].Some(6) 6
6
```

There is no `None` and no exceptions: absence is `Option[T]`
(`Some(x)`/`Nothing`), failure is `Result[T, E]` (`Ok(x)`/`Err(e)`). `?`
returns early on `Nothing` or `Err`; the function's error type must equal the
source's or contain it in an `A | B` union. `?` never converts an `Option` into
a `Result`: use `ok_or(error)?`. `unwrap()` traps. `print` shows the full
enum name, as in `Option[i64].Some(6)`. See
[propagation](../language/08-errors/01-sums-and-propagation.md) and
[anonymous sums](../language/03-types/anonymous-sums.md).

## Ownership and borrowing

```plenty
def total(values: &list[i64]) -> i64:
    mut sum = 0
    for value in values:
        sum += *value
    sum

def double_all(values: &mut list[i64]) -> ():
    for value in values:
        *value *= 2

def main() -> Result[(), AllocError | IoError]:
    mut values = [1, 2, 3]?
    print(total(&values), total(values))?
    double_all(&mut values)
    backup = copy(values)?
    moved = values
    print(moved, backup)?
    Ok(())
```
```output
6 6
[2, 4, 6] [2, 4, 6]
```

Scalars, `str`, and tuples of copyable values copy. Collections, boxes, and
every class instance move: after `moved = values`, `values` is unusable.
`copy(x)?` is the explicit, fallible deep copy. `&x` lends shared access and
`&mut x` exclusive access; a `&T` parameter borrows an owner implicitly, but
`&mut` must be written. Iterating `&list` yields `&T`, so dereference with `*`.
References cannot be stored in classes, enums, tuples, or collections. See
[moves and copies](../language/05-ownership/01-bindings-moves-and-copies.md) and
[borrowing](../language/05-ownership/02-borrowing.md).

## Generators

```plenty
def countdown(start: i64) -> Generator[i64]:
    mut n = start
    while n > 0:
        yield n
        n -= 1

def main() -> Result[(), AllocError | IoError]:
    for n in countdown(3):
        print(n)?
    print(list(countdown(4))?, any(n == 2 for n in countdown(5)))?
    Ok(())
```
```output
3
2
1
[4, 3, 2, 1] True
```

A function containing `yield` returns `Generator[T]`; its body cannot use `?`.
See [generators](../language/09-iteration/03-generators.md).

## Local pipelines

Local transformations can use `iter(source).map(callback).filter(predicate)`
with a loop or a terminal such as `fold(initial, callback)` or `count()`.
Borrowed sources and callback captures stay borrowed until the pipeline is
consumed. Generators expose the same fluent methods directly. An unfinished
pipeline stays local; `.generator()` explicitly transfers owned sources and
owned captures into one resumable frame that can be passed or returned. See
[local iterator pipelines](../language/09-iteration/06-pipelines.md).
`enumerate` and `zip` produce pair items: mapping takes two parameters,
predicates borrow both, and folds take the accumulator followed by the pair's
components. Collect owned pairs into an expected `dict[K, V]` with `collect()`. `a.chain(b)` continues with a
second source of the same item type; `flat_map(f)` emits the items of the owned
source `f` returns for each item. `filter_value(v)` keeps items equal to `v`
(`True` by default) without a callback; borrowed items compare with an owned `v`.

```plenty
def add(total: i64, value: i64) -> i64:
    total + value

def squares(stop: i64) -> Generator[i64]:
    iter(range(stop)).map(def(n: i64) -> i64: n * n).generator()

def main() -> Result[(), IoError]:
    values = iter(range(5)).map(def(n: i64) -> i64: n * n)
    print(values.fold(0, add))?
    print(squares(4).fold(0, add))?
    Ok(())
```
```output
30
14
```

## Python habits that Plenty rejects

| Python | Plenty |
| --- | --- |
| Statements at module level | Put them in `main` |
| `int`, `float` | `i64`, `f64`, or another width |
| `None` | `Nothing` in an `Option[T]` |
| `try`/`except`, `raise` | Return `Result`; handle with `?` or `match` |
| `xs = [1, 2]` | `xs = [1, 2]?` |
| `print(x)` as a statement | `print(x)?` |
| `"a" + b` | `("a" + b)?`, or an f-string with `?` |
| `x ** n` | `x.pow(n)` |
| `abs(x)`, `min`, `max`, `sum(xs)` | `std.math.abs`, `min`, `max`, `sum(&xs, 0)` |
| `if items:` | `if len(items) > 0:` |
| `a < b < c` | `a < b and b < c` |
| `xs[1:3]`, `s[1:3]` | `xs.slice(1, 3)?`, `s.slice(1, 3)?` |
| `lambda x: x + 1` | `def(x: i64) -> i64: x + 1` inside brackets |
| `map(f, xs)`, `filter(p, xs)`, `reduce(f, xs, init)` | `iter(&xs).filter(p).map(f)`, `.fold(init, f)`; no import |
| `d[k] = v` for a new key | `d.insert(k, v)?` |
| `case x if x > 0:` | Test inside the arm |
| `class B(A):` | Composition or a [protocol](../language/07-generics/protocols.md) |
| Global variables | Pass values as parameters; `const` holds numbers only |
| `y = xs`, then using `xs` | `y = copy(xs)?`, or borrow with `&xs` |

Three of the most common rejections:

```plenty-error
def main() -> ():
    print("hello")
    return
```
```error
cannot implicitly discard a Result
```

```plenty-error
def main() -> ():
    x = 2 ** 3
```
```error
exponentiation with `**` is not supported; use `base.pow(exponent)`
```

```plenty-error
def main() -> Result[(), AllocError | IoError]:
    values = [1, 2]?
    moved = values
    print(values)?
    Ok(())
```
```error
use of moved binding `values`
```

Run `plenty --check FILE` to type-check without building.
