# Borrowing and unpacking while iterating

Borrow a list when a helper needs to inspect or mutate it without taking ownership:

```plenty
def total(values: &list[i64]) -> i64:
    mut result = 0
    for value in values:
        result = result + value
    result

def add(values: &mut list[i64], value: i64) -> Result[(), AllocError]:
    values.append(value)?
    Ok(())

def main() -> Result[(), Failure]:
    mut numbers = [1, 2]?
    print(total(numbers))?
    add(&mut numbers, 3)?
    print(numbers)?
    Ok(())
```

```output
3
[1, 2, 3]
```


## Unpack a tuple in a loop

```plenty
def measurement() -> (i64, str):
    (42, "cm")

def main() -> Result[(), Failure]:
    value, unit = measurement()
    print(value)?
    print(unit)?
    for number, word in [(1, "one"), (2, "two")]?:
        print((number, word))?
    print((3, "three"))?
    Ok(())
```
```output
42
cm
(1, "one")
(2, "two")
(3, "three")
```
