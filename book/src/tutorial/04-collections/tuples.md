# Returning and unpacking tuples

A tuple groups values with different types. Write `(value,)` for one component;
parentheses without a comma simply group an expression. Function signatures may
spell a tuple as `(i64, str)` or `tuple[i64, str]`. Unpacking transfers owned
components, and `_` discards a component. Indices must be integer literals.

```plenty
def measurement() -> (i64, str):
    (42, "cm")

def main() -> Result[(), Failure]:
    value, unit = measurement()
    print(value)?
    print(unit)?
    print((3, "three"))?
    Ok(())
```
```output
42
cm
(3, "three")
```

Creating `(a, b)` does not allocate a tuple, so it needs no `?`. Operations that
produce its components, such as the list construction below, can still fail.

Distinct tuple components can be borrowed and read independently. The borrow
below protects component 1 while component 0 remains readable:

```plenty
def main() -> Result[(), Failure]:
    mut pair = (1, [2]?)
    items = &mut pair[1]
    first = pair[0]
    print(first)?
    print(pair[0])?
    items.append(3)?
    print(pair)?
    Ok(())
```
```output
1
1
(1, [2, 3])
```

Reading an exclusively borrowed component is rejected:

```plenty-error
def main() -> Result[(), Failure]:
    mut pair = (1, [2]?)
    items = &mut pair[1]
    print(pair[1])?
    items.append(3)?
    Ok(())
```
```error
conflicting borrow: cannot read or borrow `pair[1]` while it is exclusively borrowed
```
