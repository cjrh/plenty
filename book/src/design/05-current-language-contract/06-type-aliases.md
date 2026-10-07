# Type aliases

```python
type int = i32
type Count = int

def increment(value: Count) -> Count:
    value + Count(1)
```

`type Name = Type` declares a transparent, non-generic alias at module scope.
Aliases may target any supported primitive, collection type, `range`, `()`, or another alias. They are
visible throughout the module and may refer to later declarations. Cycles,
unknown targets, duplicate aliases, and collisions with built-ins or function
names are rejected, including unused aliases. Local aliases are not supported.
An alias creates no new nominal identity, validation rule, layout, or runtime
wrapper; two aliases for `i32` are interchangeable with each other and `i32`.

Use aliases in parameter/return types and local annotations. Numeric aliases
also support the same explicit cast syntax as their target: with `type int =
i32`, `int(42)` is exactly `i32(42)`, including truncation semantics. Boolean,
string, and unit aliases do not introduce constructors or conversions. Existing
restrictions on storing or passing unit still apply through aliases.

Aliases do not change unconstrained literal defaults or add literal suffixes.
With the alias above, `x: int = 42` uses its `i32` annotation as literal context.
`x: int = 42i32` and `x: int = int(42)` also work. `42int` is not a valid suffix. Current
examples use explicit numeric widths unless they are teaching aliases.

Aliases belong to their source module. Type lookup uses a separate namespace
from local bindings; a local may shadow a callable cast name without changing
the meaning of type annotations.
