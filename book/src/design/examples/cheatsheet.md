# Syntax cheat sheet

A compact summary of everyday Plenty, for readers who already know Python. Every
program on this page runs as a test, so the forms shown are current. Each
section links to the reference, which remains authoritative. The
[tutorial](../../tutorial/index.md) teaches the same material in order, and the
[signature index](../stdlib/signatures.md) lists the standard library.

Builds optimize by default. Use `plenty --fast-compile FILE`,
`plenty script --fast-compile FILE`, or `plenty run --fast-compile` for shorter
edit-run cycles in large projects; runtime checks stay enabled. Package output
is in `target/optimized/` by default or `target/fast-compile/` with that flag.

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
imports, `const` values, and compile-time `assert` checks. `main` returns `()`, `i32`,
`Result[(), E]`, or `Result[i32, E]`. A `Result` cannot be silently discarded:
use `?`, `match`, or `drop(result)`. See [entrypoint](../language/02-source/entrypoint.md),
[source files](../language/02-source/index.md), and [branches](../language/04-expressions/branches.md).

## Bindings and numbers

```plenty
import std.math as math

def main() -> Result[(), IoError | ParseError | OverflowError[i64]]:
    mut count = 0
    count += 1
    small = 7u8
    ratio: f64 = 1.5
    total = f64(count) + ratio
    print(count, small, total)?
    print(7 // 2, -7 // 2, 7 % 2, 7.0 / 2.0)?
    base = 2
    print(base.pow(10), (3).pow(2), math.abs(-4)?, math.max(3, 9))?
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
  `7u8`, `1.5f32`. Integers may be hexadecimal, octal, or binary (`0xFFu8`,
  `0o755`, `0b1010`) and use `_` separators (`1_000_000`).
- Types never widen implicitly; cast with `f64(n)`, `u8(n)` (traps if out of
  range), `T.try_from(n)` (also for floats), or `T.wrapping_from(n)`. `T.parse(text)` returns
  `Result[T, ParseError]`.
- `//` floors and `%` is integer-only. Integer arithmetic returns errors from
  the function: include `OverflowError[T]` and, for divisors other than
  nonzero literals, `ZeroDivisionError` in its `Result` signature. `x // 2`
  cannot fail at all. A shift by a count other than a literal adds
  `ShiftCountError[T]`. Still write `Ok(...)` on
  success; operators themselves return ordinary numbers. No `**`: use `x.pow(n)`.
- `abs`, `min`, `max`, `sum` come from [`std.math`](../stdlib/math.md).

Use `checked:` for local recovery instead of returning arithmetic errors from
the function. Its last line is the `Ok` value:

```plenty
def scaled(n: i64, d: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    checked:
        n * 1000 // d

def main() -> Result[(), IoError]:
    print(scaled(7, 2).unwrap(), scaled(7, 0).is_err(), scaled(i64.MAX, 1).is_err())?
    Ok(())
```
```output
3500 True True
```

- `Positive[T]` (> 0) and `NonNegative[T]` (>= 0) accept signed integer `T`.
  Literals convert where expected; computed values use `Positive.try_from(n)?`
  or `NonNegative.try_from(n)?`. Read with `.get()`; widen a Positive with
  `NonZero.from(p)` or `NonNegative.from(p)`. See [integer domains](../language/03-types/numbers.md#positive-and-non-negative-integers).

- [`Bounded[T, lo, hi]`](../language/03-types/bounded.md) accepts the eight integer
  types and inclusive compile-time bounds. Literals convert where expected;
  an alias such as `type Percent = Bounded[u8, 0, 100]` provides
  `Percent.try_from(n)?` for runtime inputs. Read `.get()` for arithmetic.
  [Value parameters](../language/07-generics/values.md) use `const N: i64` or
  `[T: IntType, const N: T]`.

- Each function and closure has its own declared arithmetic contract,
  independent of its definition or call site. There is no private callback
  channel. Handle a callee's `Result` normally.
- `unchecked:` asserts that its arithmetic cannot fail and yields an ordinary
  suite value, not a `Result`. A violation traps by default and is undefined
  behavior with `--unchecked assume`; `unchecked trap:` always traps. Use
  wrapping operations for modular arithmetic. Keep opt-outs small and document
  their bounds.
- `?`, `return`, `break`, and `continue` inside it act on the function or loop.
- Float arithmetic stays IEEE: overflow may produce infinity and division by
  zero may produce infinity or NaN. For recoverable zero division,
  `x.div_checked(y)` is `Result[F, ZeroDivisionError]`.

See [numbers](../language/03-types/numbers.md#checked-blocks) and
[assignment](../language/04-expressions/assignment.md).

## Text

```plenty
def main() -> Result[(), AllocError | IoError]:
    name = "Ada"
    greeting = ("Hello, " + name)?
    print(greeting)?
    print(f"{name} has {len(name)} letters; pi is {f64.PI:.2f}"?)?
    words = "a,b,,c".split(",")?
    print(words, "-".join(&words)?, "  x ".strip()?)?
    print("hello".find("l"), "hello".startswith("he"), "ll" in "hello")?
    print(name[0], name[-1], "hello"[1:3]?, str.from(42)?)?
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
An f-string has type `Result[str, AllocError]`, or
`Result[str, AllocError | FormatError]` when a field runs a `display` or
`debug` method or a spec is computed. Escapes are `\n`, `\t`,
`\r`, `\0`, `\\`, quotes, and `\u{H...}` (`"e\u{301}"`); there is no `\x` or
`\N{...}`. `len` counts Unicode scalars;
`text.byte_len()` counts bytes. A [slice](../language/04-expressions/slices.md)
such as `name[1:]?` or `name[::-1]?` copies, so it needs `?`. See
[text](../facilities/01-builtins/text.md),
[building text](../facilities/01-builtins/text-building.md), and
[format strings](../language/04-expressions/format-strings.md).

`&substr` remains a compatible alias for `&strview`. A plain `&s` still
produces an ordinary `&str`. A `&strview` function parameter accepts a whole
string as `f(s)`, implicitly borrowed without copying, or a substring as `f(&s[a:b])`. Explicit whole-string
borrows, existing `&str` bindings, and literals work too. Views support scalar
text operations and byte access; see [substring views](../language/04-expressions/slices.md#substring-views)
for operations and ownership limits. Scalar `len(view)` scans; `view.byte_len()`
and `view.as_bytes()` are O(1).

## Collections

```plenty
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut items = [3, 1, 2]?
    items.append(4)?
    items.sort()
    print(items, items[0], items[-1], items.get(10))?
    print(items[1:3]?, items[::-1]?, items.pop(), 2 in items)?

    mut counts: dict[str, i64] = {}?
    for word in "a b a".split()?:
        *counts.get_or_insert(word, 0)? += 1
    counts.insert("b", 5)?
    match counts.get_mut("a"):
        case Some(count):
            *count += 10
        case Nothing:
            pass
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
[2, 3] [4, 3, 2, 1] Option[i64].Some(4) True
{"a": 12, "b": 5} Option[i64].Nothing
a 12
b 5
2 True
2 [0, 4, 16]
```

Displays, comprehensions, `append`, `insert`, and `add` can fail to allocate,
so they return `Result`. Empty displays need a type: `xs: list[i64] = []?`.
Indexing a missing element or key traps; `get` returns an `Option`.
Dictionaries have no indexed assignment: `insert` adds or replaces,
`get_mut` returns `Option[&mut V]` to change a value in place, and
`get_or_insert(k, default)?` returns `&mut V` after inserting a missing key.
`for v in (&mut d).values():` and `for k, v in (&mut d).items():` change
every value in place. See
[collections](../facilities/01-builtins/collections.md),
[lists](../facilities/01-builtins/lists.md), and
[dictionaries](../facilities/01-builtins/dictionaries.md).

## Tuples

```plenty
def div_mod(a: i64, b: i64) -> Result[(i64, i64), OverflowError[i64] | ZeroDivisionError]:
    Ok((a // b, a % b))

def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:
    pair = (42, "cm")
    single: (i64,) = (7,)
    print(pair, pair[0], pair[-1], single[0])?
    quotient, remainder = div_mod(7, 2)?
    print(quotient, remainder, (1, "b") < (1, "c"))?
    mut point = (1, 2)
    x = &mut point[0]
    *x += 10
    print(point)?
    for n, unit in [(1, "kg"), (2, "m")]?:
        print(n, unit)?
    Ok(())
```
```output
(42, "cm") 42 cm 7
3 1 True
(11, 2)
1 kg
2 m
```

Tuples allocate nothing. Indices are integer literals; negative ones count from
the end, so `pair[-1]` is `pair[1]`. There is no `pair[0] = value`, so assign
through `&mut pair[0]`. Moving an owned component out needs whole-tuple
unpacking, including nested targets such as `(x, y), label = value`.
Starred targets are unsupported. `match` accepts tuple patterns such as
`case (Some(a), Some(b)):`. See
[tuples](../language/03-types/tuples.md) and
[assignment to several targets](../language/04-expressions/assignment.md#several-targets).

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

def main() -> Result[(), IoError | OverflowError[i64]]:
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

Block `if` and `match` can also be the whole value of a binding, a single-place
assignment, or `return`; see [conditional values](../language/04-expressions/branches.md#an-if-as-a-statements-value).
A suite's final expression is its value, so `return` is only needed for early
exits. Conditions must be `bool`; there is no truthiness. Chained comparisons
and loop `else` are rejected. `match` works on enums, classes, tuples, integers, `str`,
and `bool`, must be exhaustive, and has no guards. Tuple patterns nest within
enum/class payloads and each other. See
[matching](../language/04-expressions/matching.md) and
[loops](../language/04-expressions/loops.md).

## Functions and closures

```plenty
def scale(value: i64, factor: i64 = 2) -> Result[i64, OverflowError[i64]]:
    Ok(value * factor)

def increment(value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value + 1)

def apply[F: Callable[[i64], Result[i64, OverflowError[i64]]]](f: &F, value: i64) -> Result[i64, OverflowError[i64]]:
    f(value)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(scale(5)?, scale(5, factor=10)?)?
    offset = 100
    shift = def [offset](value: i64) -> Result[i64, OverflowError[i64]]:
        Ok(value + offset)
    print(apply(&increment, 1)?, apply(&shift, 1)?)?
    print(apply(&(def(value: i64) -> Result[i64, OverflowError[i64]]: Ok(value * 3)), 2)?)?
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
with `&F`, which takes named functions and closures; use `&mut F` for a
closure that changes a `mut` capture. See
[signatures](../language/06-functions/01-signatures-and-calls.md),
[function values](../language/06-functions/02-function-values.md), and
[closures](../language/06-functions/03-closures.md).

## Classes and enums

Use [`@serial`](../stdlib/serial-derive.md) above a class or enum to generate
missing typed JSON/TOML hooks. Decoding preserves normal construction and
constructor defaults; handwritten hooks customize a representation. The
[typed configuration lesson](../../tutorial/07-modules/30-typed-configuration.md)
shows one schema used with both formats.

For primitive-field classes, `clist[T]()` creates allocation-free empty
[columnar storage](../facilities/01-builtins/clists.md).
`items.append(row)` returns `Result[(), AppendError[T]]`; `items[i].field`
reads a scalar, `items.copy_row(i)` returns `Option[T]`, and
`items.column(T.field)` is a borrowed local pipeline yielding scalar values.
Whole-row and packed-field references are unsupported.

```plenty
@copy
class Point:
    x: f64
    y: f64

class Counter:
    name: str
    hits: i64

    def bump(self: &mut Counter) -> Result[(), OverflowError[i64]]:
        self.hits += 1
        Ok(())

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

def unused(counter: &Counter) -> bool:
    match counter:
        case Counter(_, 0):
            True
        case Counter(name=_, hits=_):
            False

def main() -> Result[(), IoError | OverflowError[i64]]:
    mut counter = Counter("clicks", 0)
    counter.bump()?
    counter.bump()?
    print(counter.report(), counter)?
    print(Shape.Rect(2.0, 3.0).area(), Shape.Dot)?
    origin = Point(0.0, 0.0)
    mut shifted = origin
    shifted.x = 1.5
    print(origin, shifted)?
    print(unused(&counter))?
    Ok(())
```
```output
2 Counter(name="clicks", hits=2)
6.0 Shape.Dot
Point(x=0.0, y=0.0) Point(x=1.5, y=0.0)
False
```

Classes have typed fields and a generated positional constructor; there is no
inheritance. An explicit `__init__` returns `()` for a plain class result or
`Result[(), E]` for precisely `Result[Class, E]`, with explicit `Ok(())` on
success. Construction adds no allocation error; initializer operations and
parameter defaults must declare or map their own errors. Bare `self` is a shared borrow; mutating methods declare
`self: &mut Class`. Explicit `self: Class` transfers an owned receiver into the
method, just like a by-value function parameter; the caller cannot reuse an
affine receiver. `@copy` before `class` makes instances copy like integers; every
field must copy, and the class cannot define `__del__`. A generic `@copy` class
copies when its type arguments copy, and `T: CopyType` requires an argument
that copies ([copy classes](../language/03-types/classes.md#copy-classes)). `@newtype` on a one-field class forwards the field's operators between two values of that class ([newtype forwarding](../language/04-expressions/operators.md#newtype-forwarding)). A field can move out of an owned binding or receiver, after
which the object cannot be used as a whole until that field is assigned again;
see [moving fields](../language/05-ownership/01-bindings-moves-and-copies.md#moving-fields).
Variants are always qualified (`Shape.Dot`), and matching a
borrowed enum binds references, so payloads are dereferenced with `*`.
`case Counter(name, hits=0)` takes a class apart, written like its constructor, with
every field named once
([class patterns](../language/04-expressions/matching.md#class-patterns)). See
[classes](../language/03-types/classes.md), [enums](../language/03-types/enums.md),
and [borrowed matching](../language/05-ownership/06-borrowed-matching.md).

Classes and enums can define [arithmetic hooks](../language/04-expressions/operators.md)
such as `__add__(self, other: &T) -> T`, `__rmul__`, and `__neg__`.
Operators borrow their operands and dispatch statically. Fallible hooks return
`Result`: write `(a + b)?`. In-place hooks such as
`__iadd__(self: &mut T, other: &T) -> ()` mutate an existing destination;
there is no automatic `+=` fallback. Equality remains structural. Other
`__name__` methods, such as `__eq__`, `__str__` and `__len__`, are
[rejected](../language/04-expressions/operators.md#reserved-method-names):
write `display`/`debug` for formatting and named methods for the rest.

## Compile-time dimensions

Use `dimension` for a base or a derived dimension, and `D: Dimension` for an
argument that carries no runtime value:

```plenty
dimension Length("m")
dimension Time("s")
dimension Velocity = Length / Time
const metre: Quantity[f64, Length] = Quantity[f64, Length](1.0)

def main() -> Result[(), IoError]:
    distance = 12.0 * metre
    elapsed = Quantity[f64, Time](4.0)
    speed: Quantity[f64, Velocity] = distance / elapsed
    print(speed)?
    Ok(())
```
```output
3.0 m/s
```

Products and quotients normalize; `Pow[A, n]` and `Root[A, n]` require integer
exponents and exact roots. `One` is dimensionless. A dimension cannot be a value
type. `Quantity[T, D]` checks dimensions in arithmetic and `.get()` reads its
stored number. For floats, `.to(unit)` divides by a quantity of the same type.
[`std.units`](../stdlib/units.md) provides SI dimensions, units such as `u.km`
and constants; declare your own only for other bases.
See [dimensions](../language/03-types/dimensions.md) and
[quantities](../language/03-types/quantities.md).

## Option, Result, and errors

```plenty
def parse_pair(text: str) -> Result[(i64, i64), AllocError | ParseError]:
    parts = text.split(",")?
    if len(parts) != 2:
        return Err(ParseError.Invalid)
    Ok((i64.parse(parts[0])?, i64.parse(parts[1])?))

def first_even(values: &list[i64]) -> Option[i64]:
    two: NonZero[i64] = 2
    for value in values:
        if *value % two == 0:
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
a `Result`: use `ok_or(error)?`. To wrap an error, map it explicitly:
`print(line).map_error(AppError.Io)?`. `unwrap()` traps. `print` shows the full
enum name, as in `Option[i64].Some(6)`. See
[propagation](../language/08-errors/01-sums-and-propagation.md) and
[anonymous sums](../language/03-types/anonymous-sums.md).

## Ownership and borrowing

```plenty
def total(values: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut sum = 0
    for value in values:
        sum += *value
    Ok(sum)

def double_all(values: &mut list[i64]) -> Result[(), OverflowError[i64]]:
    for value in values:
        *value *= 2
    Ok(())

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut values = [1, 2, 3]?
    print(total(&values)?, total(values)?)?
    double_all(&mut values)?
    backup = copy(values)?
    moved = values
    print(moved, backup)?
    label = "team"
    other = label
    print(label, other)?
    Ok(())
```
```output
6 6
[2, 4, 6] [2, 4, 6]
team team
```

Numbers, `bool`, `str`, `bytes`, ranges, and named function values copy. A
tuple, `Option`, `Result`, enum, or anonymous sum (`A | B`) copies when all of
its parts copy. A class copies only when it is declared [`@copy`](../language/03-types/classes.md#copy-classes), and a generic one only when its type arguments
also copy.
Every other value moves: collections, boxes, closures that capture,
generators, and other class instances.
After `moved = values`, `values` is unusable.
`str` assignment keeps both names usable without `copy`; immutable storage may
be shared. `copy(x)?` explicitly duplicates eligible owned mutable contents.
An eligible ordinary class does not need `@copy` for this explicit operation;
nested [Shared](../facilities/01-builtins/shared.md) handles retain the same content.
Custom destruction, recursive data, generators and captured closures restrict
copying. `&x` lends shared access and
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
        # The loop proves 1 <= n <= i64.MAX, so decrement cannot overflow.
        unchecked:
            n -= 1

def main() -> Result[(), AllocError | IoError]:
    for n in countdown(3):
        print(n)?
    print(list(countdown(4))?, countdown(5).any(def(n: &i64) -> bool: *n == 2))?
    Ok(())
```
```output
3
2
1
[4, 3, 2, 1] True
```

A function containing `yield` returns `Generator[T]`; its body cannot use `?`.
Generators have the [pipeline](#local-pipelines) methods, such as
`any(predicate)`. See [generators](../language/09-iteration/03-generators.md).

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
(`True` by default) without a callback; borrowed items accept `v` or `&v`.

```plenty
def add_square(total: i64, value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(total + value * value)

def add_result(total: i64, value: Result[i64, OverflowError[i64]]) -> Result[i64, OverflowError[i64]]:
    Ok(total + value?)

def squares(stop: i64) -> Generator[Result[i64, OverflowError[i64]]]:
    iter(range(stop)).map(def(n: i64) -> Result[i64, OverflowError[i64]]: Ok(n * n)).generator()

def main() -> Result[(), IoError | OverflowError[i64]]:
    values = iter(range(5))
    print(values.try_fold(0, add_square)?)?
    print(squares(4).try_fold(0, add_result)?)?
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
| `range(a, b, step)` with a computed step | `range(a, b, NonZero.try_from(step)?)`; a literal step converts |
| `abs(x)`, `min`, `max`, `sum(xs)` | Integer `std.math.abs(x)?`, `min`, `max`, `sum(&xs, 0)?`; float `abs_float` / `sum_float` |
| `if items:` | `if len(items) > 0:` |
| `a < b < c` | `a < b and b < c` |
| `xs[1:3]`, `s[1:3]` | `xs[1:3]?`, `s[1:3]?`: a slice allocates a copy; `&xs[1:3]` borrows a list view, and `&s[1:3]` a `&strview`, in place |
| `lambda x: x + 1` | `def(x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + 1)` inside brackets |
| `map(f, xs)`, `filter(p, xs)`, `reduce(f, xs, init)` | `iter(&xs).filter(p).map(f)`, `.fold(init, f)`; no import |
| `any(flags)`, `all(p(x) for x in xs)` | `iter(&flags).any()`, `iter(&xs).all(p)` |
| `d[k] = v` | `d.insert(k, v)?`, which adds or replaces |
| `d[k] += 1` | `*d.get_or_insert(k, 0)? += 1`, or `*v += 1` on `Some(v)` from `d.get_mut(k)` |
| `case x if x > 0:` | Test inside the arm |
| `class B(A):` | Composition or a [protocol](../language/07-generics/protocols.md) |
| Global variables | Pass values as parameters; `const` holds a number, quantity, `bool`, `str`, or `bytes` computed at compile time |
| `y = xs`, then using `xs` | `y = copy(xs)?`, or borrow with `&xs` |
| A name first bound inside an `if`, loop, or `match` arm, used after it | Bind `name = if condition:` (or `name = match value:`), or use `a if cond else b`; the binding ends with its block. Only [`if const`](../language/02-source/target-conditionals.md) arms open no scope |
| `x is None` | `x.is_none()`, or `match` on `Some`/`Nothing` |
| `"-" * 3` | `"-".repeat(3)?` |
| `del xs[i]` | `xs.pop(i)`, which returns an `Option` |
| `assert x` in a function | Return an error: `if not x: return Err(...)` |
| `xs.sort(reverse=True)` | `xs.sort(True)` |
| `open(path, "rb")` | `std.io.open_file(path, "r")?` for bytes; [modes](../facilities/02-io/files.md) take no `b` or `t` letter |

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
