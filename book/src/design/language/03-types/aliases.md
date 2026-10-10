# Type aliases

```python
type Count = i32

def increment(value: Count) -> Count:
    value + Count(1)
```

`type Name = Type` declares a transparent, non-generic module-level alias to a
supported type or another alias. Aliases are visible throughout the module and
may refer forward. Unknown targets, cycles, duplicate aliases, and declaration
or built-in name collisions are rejected even when the alias is unused.
Local aliases are unsupported.

An alias adds no nominal identity, validation, or runtime wrapper. Two aliases
for `i32` are interchangeable with each other and `i32`. Existing storage and
visibility restrictions apply through aliases.

Aliases work in signatures and local annotations. Numeric aliases support their
target's cast syntax and conversion semantics. Boolean, string, and unit aliases
add no constructors or conversions.

Aliases do not change literal defaults or introduce suffixes: `x: Count = 42`
uses the annotation as context, while `42Count` is invalid. A local expression
name may shadow `Count` without changing its meaning in a type annotation.
