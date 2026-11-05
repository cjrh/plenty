# Match combinations with tuple patterns

Match a tuple when the answer depends on several values together. Each
component takes a pattern, and the cases cover combinations of components:

```plenty
def combine(left: Option[i64], right: Option[i64]) -> Result[i64, OverflowError[i64]]:
    match (left, right):
        case (Some(a), Some(b)):
            Ok(a + b)
        case (Some(a), Nothing):
            Ok(a)
        case (Nothing, Some(b)):
            Ok(b)
        case (Nothing, Nothing):
            Ok(0)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(combine(Some(3), Some(4))?)?
    print(combine(Nothing, Some(5))?)?
    Ok(())
```
```output
7
5
```

Leaving out `(Nothing, Nothing)` is an error: both inputs can be absent.
Tuple patterns also nest inside enum payloads, such as `Some((value, offset))`.
Write `(pattern,)` for a singleton tuple; `(pattern)` groups a pattern.

Matching an owned tuple transfers owned components into the case's bindings
and destroys ignored components. Match `&value` or `&mut value` to borrow its
components, as in [borrowed matching](08-borrowed-matching.md). The
[matching reference](../../design/language/04-expressions/matching.md#tuple-patterns)
describes coverage and pattern limits.
