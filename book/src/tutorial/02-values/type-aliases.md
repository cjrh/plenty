# Give types names of your own

There is no built-in `int`. If you want that name, choose its meaning explicitly:

```plenty
type int = i32

def increment(value: int) -> Result[int, OverflowError[int]]:
    Ok(value + int(1))

def main() -> Result[(), IoError | OverflowError[i32]]:
    answer: int = increment(int(41))?
    print(answer)?
    Ok(())
```

```output
42
```

`type int = i32` makes `int` another name for `i32`, including in casts.
Aliases do not change unconstrained literal defaults, but an annotation such
as `answer: int = 41` selects the literal's width. Aliases are not literal
suffixes: write `41i32`, not `41int`.

Aliases can describe a value's purpose and refer to other aliases:

```plenty
type Count = u32
type ItemCount = Count

def add_one(count: ItemCount) -> Result[ItemCount, OverflowError[ItemCount]]:
    Ok(count + 1u32)

def main() -> Result[(), IoError | OverflowError[u32]]:
    print(add_one(41u32)?)?
    Ok(())
```

```output
42
```

`Count`, `ItemCount`, and `u32` are interchangeable. An alias does not create
a distinct type or enforce units of measurement.

Declare aliases at module scope; they are visible throughout the file. Alias
chains must reach a concrete type: cycles and unknown targets are errors.
An alias cannot redefine a built-in name, another alias, or a function name.
