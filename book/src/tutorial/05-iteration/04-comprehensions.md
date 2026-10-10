# Build collections with comprehensions

A comprehension produces a new collection from an iterable and optional filters.
The element expression runs only after the filters pass.

```plenty
def main() -> Result[(), Failure]:
    squares = [n * n for n in range(8) if n % 2 == 0]?
    print(squares)?
    print({n: n * n for n in [2, 3]?}?)?
    print(len({n // 2 for n in range(8)}?))?
    print([x * 10 + y for x in range(3) for y in range(x)]?)?
    Ok(())
```

```output
[0, 4, 16, 36]
{2: 4, 3: 9}
4
[10, 20, 21]
```

Multiple clauses nest from left to right. Later iterables can use earlier
variables, and you can use multiple `if` filters. Variables remain local to
the comprehension. Parenthesize a conditional expression used as an iterable
or filter. Dictionary comprehensions evaluate each key before its value.

`%` is modulo: the result follows the divisor's sign, so `-7 % 3` is `2`.
Both operands must have the same integer type.

Every element must have the same type; there is no implicit numeric widening:

```plenty-error
def main() -> ():
    values = [1, True].unwrap()
```

```error
expected list[i64]
```

Both comprehensions and repeated `append` calls use growing storage in place.
Assignment does not copy collections, and mutation does not secretly copy their
contents. Use `copy` when duplication is intended. Storage is reclaimed as owners
are replaced or leave scope; no `free` calls or reference-count management are needed.

## Collect a snapshot with unpacking

The dictionary view from the unpacking lesson can feed a comprehension directly.
The comprehension explicitly allocates a list of tuples:

```plenty
def main() -> Result[(), Failure]:
    mut counts = {"apple": 2, "pear": 3}?
    for fruit, count in (&mut counts).items():
        *count = *count + 1
    print([(fruit, count) for fruit, count in counts.items()]?)?
    print([a + b for a, b in [(1, 2), (3, 4)]?]?)?
    Ok(())
```
```output
[("apple", 3), ("pear", 4)]
[3, 7]
```

For dictionaries containing owned values, use `copy(value)?` when a snapshot
needs independent owned values. Iterating the view itself does not allocate
a snapshot or consume the dictionary.

Text iteration uses the same syntax, yielding one Unicode scalar at a time:

```plenty
def main() -> Result[(), Failure]:
    print([character for character in "hé"]?)?
    Ok(())
```
```output
["h", "é"]
```
