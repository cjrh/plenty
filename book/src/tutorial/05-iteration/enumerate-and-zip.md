# Count positions and pair inputs

Use `enumerate(iterable)` to get an `i64` index starting at zero beside each
element. Supply a second argument to choose the first index. Use
`zip(left, right)` to pair elements until the shorter input ends.

```plenty
def main() -> Result[(), IoError]:
    for index, letter in enumerate("ab", 10):
        print((index, letter))?
    for number, letter in zip(range(3), "xy"):
        print((number, letter))?
    print([index + value for index, value in enumerate(range(3))].unwrap())?
    Ok(())
```

```output
(10, "a")
(11, "b")
(0, "x")
(1, "y")
[0, 2, 4]
```

An owned list transfers its elements. Borrow a list to preserve it; its
elements retain their reference types. Borrow mutably to edit them.

```plenty
def main() -> Result[(), IoError]:
    mut values = [10, 20].unwrap()
    for index, value in enumerate(&mut values):
        *value += index
    for value, offset in zip(&values, range(2)):
        print(*value + offset)?
    print(values)?
    Ok(())
```

```output
10
22
[10, 21]
```

Both helpers work directly in loop and comprehension sources. They add no
allocation and evaluate their input expressions once, from left to right.
They do not yet produce a value you can store or pass to `next`. Borrowed
inputs require two unpacked bindings; references cannot be placed into an
item tuple. Nested helpers are supported when their inner items are owned
values. To pair a dictionary's keys, pass the dictionary itself; `items()`
remains a separate loop form.

`zip` tests the left input before the right. A left generator may advance
once more when the right input ends; that unmatched item is dropped. Early
exit drops remaining owned inputs. An enumeration index overflows only when
another element requires an index beyond `i64.MAX`.
