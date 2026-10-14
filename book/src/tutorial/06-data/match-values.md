# Bind the value a match selects

A `match` at the end of a function supplies the function's result. To use the
selected value in the middle of a function, write the `match` as the value of a
binding. The final expression of the selected case becomes the value:

```plenty
enum Shape:
    Dot
    Square(i64)
    Rect(i64, i64)

def paint_cost(shape: Shape) -> i64:
    area = match shape:
        case Shape.Dot:
            return 1
        case Shape.Square(side):
            side * side
        case Shape.Rect(width, height):
            width * height
    area * 10

def main() -> Result[(), Failure]:
    print(paint_cost(Shape.Dot))?
    print(paint_cost(Shape.Square(3)))?
    print(paint_cost(Shape.Rect(2, 5)))?
    Ok(())
```

```output
1
90
100
```

The cases that continue must produce one type. A case may instead leave with
`return`, `break`, `continue`, or `?`, as `Shape.Dot` does; it then supplies no
value and nothing is bound.

The same block is the value of an assignment to a `mut` local, a field, or a
collection element, and of `return`. An annotation gives every case its
expected type, which `Nothing` needs:

```plenty
def half(number: i64) -> Option[i64]:
    return match number % 2:
        case 0:
            Some(number // 2)
        case _:
            Nothing

def main() -> Result[(), Failure]:
    mut halves = [0, 0]?
    for index in range(2):
        found: Option[i64] = match index:
            case 0:
                half(8)
            case _:
                Nothing
        halves[index] = match found:
            case Some(value):
                value
            case Nothing:
                -1
    print(halves)?
    print(half(3))?
    Ok(())
```

```output
[4, -1]
Option[i64].Nothing
```

`return match` returns the final expression of the selected case, so nothing
after the block runs.

The block must be the whole right-hand side. Inside a larger expression it is
rejected; bind it first:

```plenty-error
def main() -> Result[(), Failure]:
    number = 3
    doubled = 2 * match number:
        case 3:
            30
        case _:
            0
    print(doubled)?
    Ok(())
```

```error
a `match` block is a value only as the whole right side of a binding, a single-target assignment, or `return`; bind its value to a name first
```

Cases that disagree are rejected at the case that differs:

```plenty-error
def main() -> Result[(), Failure]:
    number = 3
    label = match number:
        case 3:
            "three"
        case _:
            0
    print(label)?
    Ok(())
```

```error
expected str, got i64
```

The [reference](../../design/language/04-expressions/matching.md#a-match-as-a-statements-value)
lists the remaining rules, including why the value cannot be a reference.
