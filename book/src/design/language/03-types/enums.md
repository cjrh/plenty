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
generators, including through nested standard wrappers. A function result may
carry a reference in the `Some` or `Ok` payload; such a value is held only in a
local and never stored, as described in
[optional and fallible references](../05-ownership/05-optional-references.md).

Constructors can qualify variants with an enum name, transparent alias, or
explicit generic instance; patterns use the enum name or an alias and never
type arguments. Public enums expose every variant.
Enums with permitted element types can be list elements or dictionary values;
they are not dictionary keys or set elements.

Equality compares nominal identity, variant, and payload contents, including IEEE
float comparisons. Printing uses qualified variant names. Automatic equality and
formatting reject recursive data types.

[Matching](../04-expressions/matching.md) selects variants and binds payloads.
Standard [Option and Result](../08-errors/index.md) add prelude constructors and
contextual inference. Generic enums follow [generic data](../07-generics/data.md).

## Methods

An enum can declare ordinary methods alongside its variants:

```plenty
enum Reading:
    Missing
    Value(i64)

    def number(self) -> i64:
        match self:
            case Reading.Missing:
                0
            case Reading.Value(value):
                *value
```

Bare `self` borrows the enum; `self: &mut Reading` takes an exclusive borrow.
Matching that receiver borrows the active payload under the ordinary matching
rules. Methods may have their own generic parameters and use named arguments
and defaults under the ordinary method rules. Generic enum parameters come from
the receiver, as described in [generic data](../07-generics/data.md).

Methods are private to their defining module unless declared `pub def`.
Variant and method names must be distinct. Class lifecycle hooks are not enum
methods: variant construction and automatic payload destruction retain their
existing behavior. Anonymous sums cannot declare methods. They forward only the
[`std.report.Formattable`](../../stdlib/report.md) contract when every member
satisfies it; arbitrary member methods and protocols are not forwarded.

Enums can satisfy [structural protocols](../07-generics/protocols.md) with
accessible matching methods. They can also supply `display` and `debug` methods
for [`std.fmt`](../../stdlib/fmt.md); builtin printing and entrypoint reports
continue to use structural rendering.
