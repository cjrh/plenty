# Match borrowed values

Use `match &value` to inspect an enum while retaining its owner. Payload
bindings are shared references. With `match &mut value`, they are exclusive
references and can change the original payload.

An existing enum reference can be matched directly. A function can also return
a payload reference when it comes from the function's single reference parameter:

```plenty
def number(value: &Result[i64, i64]) -> &i64:
    match value:
        case Ok(payload):
            payload
        case Err(payload):
            payload

def main() -> Result[(), Failure]:
    mut outcome: Result[i64, i64] = Err(7)
    selected = number(&outcome)
    print(*selected)?
    outcome = Ok(9)
    print(number(&outcome))?
    Ok(())
```

```output
7
9
```

Replacing `outcome` is allowed after `selected`'s last use; doing it before that
use is rejected. References cannot outlive their owner.

Assign through a mutable payload reference to change the original slot, keeping
its enclosing variants:

```plenty
def main() -> Result[(), Failure]:
    mut nested: Option[Result[i64, str]] = Some(Ok(4))
    match &mut nested:
        case Some(result):
            match result:
                case Ok(number):
                    *number = *number + 1
                case Err(_):
                    pass
        case Nothing:
            pass
    print(nested)?
    Ok(())
```

```output
Option[Result[i64, str]].Some(Result[i64, str].Ok(5))
```

`_` ignores a payload without moving it. Borrowed matching uses the same
exhaustiveness checks and qualified variant names as owned matching.
