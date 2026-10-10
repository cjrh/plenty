# Enums

```python
enum Reading:
    Missing
    Value(i64)
    Invalid(str)
```

Enums are nominal module-level types. Variants have fixed positional payloads;
nullary declarations omit parentheses. `Reading.Value(7)` and
`Reading.Value.new(7)` return `Reading` directly. Nullary variants use
`Reading.Missing` or `Reading.Missing.new()`.

Construction allocates nothing. Payload expressions evaluate first and transfer
their values into the enum; their own allocation and error contracts still apply.
Payloads can include primitive values, classes, collections, tuples, other enums,
unit, and resolved owned closures. References and generator-containing values
cannot be user enum payloads. Standard `Option`/`Result` wrappers can contain
generators, including through nested standard wrappers.

Constructors and patterns can qualify variants with an enum name, transparent
alias, or explicit generic instance. Public enums expose every variant.
Enums with permitted element types can be list elements or dictionary values;
they are not dictionary keys or set elements.

Equality compares nominal identity, variant, and payload contents, including IEEE
float comparisons. Printing uses qualified variant names. Automatic equality and
formatting reject recursive data types.

[Matching](../04-expressions/matching.md) selects variants and binds payloads.
Standard [Option and Result](../08-errors/index.md) add prelude constructors and
contextual inference. Generic enums follow [generic data](../07-generics/data.md).
