# A small program

```python
def choose(flag: bool, first: i64, second: i64) -> i64:
    """Choose one of two integers."""
    first if flag else second

def countdown(n: i64) -> Result[(), IoError]:
    if n == 0:
        Ok(())
    else:
        print(n)?
        countdown(n - 1)

def main() -> Result[(), IoError]:
    mut answer: i64 = choose(True, 40, 0)
    answer = answer + 2
    print(answer)?
    Ok(())
```
