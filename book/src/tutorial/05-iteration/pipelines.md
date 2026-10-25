# Chain iteration steps with `iter`

`iter(source)` starts a pipeline: a chain of iteration steps that runs only
when a loop or a final method such as `count()` consumes it. No intermediate
collection is built.

```plenty
def main() -> Result[(), AllocError | IoError]:
    rolls = [3, 6, 2, 6, 6, 1]?
    print(iter(&rolls).filter_value(6).count())?
    for turn, roll in iter(&rolls).enumerate(1).skip(2).take(3):
        print(turn, *roll)?
    print(len(rolls))?
    Ok(())
```

```output
3
3 2
4 6
5 6
6
```

`filter_value(6)` keeps the items equal to `6`, and `count()` counts them
without building a list. `enumerate(1)` numbers the items from one, `skip(2)`
discards the first two, and `take(3)` stops after three more. Steps apply in
the order written: `enumerate` comes first, so the turn numbers count every
roll, including skipped ones.

The source follows the ordinary iteration rules. Borrowing `rolls` keeps the
list, and each item is a reference, so the loop reads `*roll`.
`filter_value` still takes a plain `i64`: a borrowed item is compared by value.
It also accepts a borrowed value, such as `filter_value(&wanted)`, so a string
can be matched without copying it; `wanted` then stays borrowed until the
pipeline ends.

A pipeline is single-use. Consuming it again, or using an owned source after
the pipeline has taken it, is an ownership error.

## Pair, join, and collect

`zip` walks two sources together, and `chain` continues with a second source of
the same item type. `collect()` builds the collection named by the binding's
annotation and returns a `Result` for allocation failure:

```plenty
def main() -> Result[(), AllocError | IoError]:
    names = ["Ada", "Bob", "Cal"]?
    present = [True, False, True]?
    for name, here in iter(&names).zip(&present):
        print(name, *here)?
    print(iter(&present).filter_value().count())?
    morning = [3, 1]?
    evening = [4, 3]?
    readings: list[i64] = iter(morning).chain(evening).collect()?
    print(readings)?
    Ok(())
```

```output
Ada True
Bob False
Cal True
2
[3, 1, 4, 3]
```

Without an argument, `filter_value()` keeps `True` items. The two owned lists
move into the chain, so their numbers move into `readings` without copying.

The value must have the item's type. There is no implicit conversion, and the
no-argument form works only for `bool` items:

```plenty-error
def main() -> Result[(), AllocError | IoError]:
    rolls = [3, 6]?
    print(iter(&rolls).filter_value("6").count())?
    Ok(())
```

```error
filter_value compares &i64 items with a str value; it requires i64 or &i64
```

Generators have the same methods without `iter`:

```plenty
def countdown(start: i64) -> Generator[i64]:
    mut remaining = start
    while remaining > 0:
        yield remaining
        remaining -= 1

def main() -> Result[(), AllocError | IoError]:
    last: list[i64] = countdown(10).skip(7).collect()?
    print(last)?
    Ok(())
```

```output
[3, 2, 1]
```

`skip` still advances the generator past the first seven values; it only
discards them.

Prefer a pipeline when a chain of these steps states the task directly, and a
plain loop or comprehension when the work needs its own expression or several
statements. Most other pipeline steps, such as `map`, `filter`, and `fold`,
take a function. They are taught after
[functions as values](../10-functions/index.md), starting with
[Reduce a borrowed collection with a pipeline](../10-functions/pipelines.md).
The [pipeline reference](../../design/language/09-iteration/06-pipelines.md)
lists every step and its cost.
