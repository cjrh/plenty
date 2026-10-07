# Choosing numeric types in expressions

Type arguments go before the call: `range[u8](8)`. A collection annotation or
function return type can also guide a directly written range comprehension.

```plenty
def squares() -> Result[list[u8], AllocError]:
    [n * n for n in range(8)? if n % 2 == 0]

def main() -> Result[(), Failure]:
    print(squares()?)?
    print([n * n for n in range[u8](8)? if n % 2 == 0]?)?
    small: list[u16] = [n + 1 for n in range(3)?]?
    print(small)?
    print(list(range[u8](5, 0, -2)?)?)?
    fraction: f32 = 3.5
    print(fraction * 2)?
    Ok(())
```
```output
[0, 4, 16, 36]
[0, 4, 16, 36]
[1, 2, 3]
[5, 3, 1]
7.0
```

Context guides unsuffixed literals, never changes an existing value's type.
`x: u8 = 256` fails, as does assigning an i64 binding to a u8 binding. Explicit
suffixes remain authoritative. A range's bounds must fit its element type;
`range[u8](256)` therefore fails even though its stop is exclusive.

Inference stays within straightforward expressions. It does not infer a range
type backward through filters, arbitrary calls, or a previously stored range.
Use explicit `range[T]` in those cases. There is no silent numeric widening or
narrowing.
