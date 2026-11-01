# Plenty

Plenty is a programming language and compiler.

```python
type int = i64

def sum_to(n: int, total: int) -> int:
    """Sum the integers from 1 through n, using constant call-stack space."""
    if n == 0:
        total
    else:
        sum_to(n - 1, total + n)

def main() -> ():
    mut answer = sum_to(100, 0)
    answer = answer + 1
    print(answer).unwrap()
```

## Documentation

- [Learn Plenty](book/src/tutorial/index.md) and [setup](book/src/tutorial/00-setting-up.md)
- [Language and standard library reference](book/src/design/index.md)
- [Development workflow](book/src/development/workflow.md) and [backlog](book/src/backlog.md)
- [Online documentation](https://cjrh.info/plenty/) and [current development documentation](https://cjrh.info/plenty/dev/)

## License

Plenty is licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT), at your option.
