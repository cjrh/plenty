# Enums

```plenty fragment
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
unit, and resolved owned closures. An enum copies when every payload of every
variant copies, and moves otherwise; `@copy` does not apply to enums. See
[the copy rule](../05-ownership/01-bindings-moves-and-copies.md). References and generator-containing values
cannot be user enum payloads. Standard `Option`/`Result` wrappers can contain
generators, including through nested standard wrappers. A function result may
carry a reference in the `Some` or `Ok` payload; such a value is held only in a
local and never stored, as described in
[optional and fallible references](../05-ownership/05-optional-references.md).

Constructors can qualify variants with an enum name, transparent alias, or
explicit generic instance; patterns use the enum name or an alias and never
type arguments. Public enums expose every variant.

A payload variant named without arguments is a
[function value](../06-functions/02-function-values.md): `Reading.Value` has
type `Callable[[i64], Reading]`, and a variant with two payloads takes two
arguments. Pass it where a callable is expected, for example
`i64.parse(text).map_error(ConfigError.InvalidCount)` or
`iter(range(3)).map(Reading.Value)`. A generic enum needs an explicit instance,
as in `Wrapped[i64].One`, or a concrete expected `Callable` whose result names
that instance. This includes `Option[i64].Some`, `Result[i64, str].Ok`, and
`Result[i64, str].Err`. The prelude `Some`, `Ok`, and `Err` specialize from an
expected callable signature, for example
`wrap: Callable[[i64], Option[i64]] = Some`. Nullary variants remain values of
the enum, not functions. A constructor value cannot have unit or generator
payload parameters; use a closure for those signatures.
Enums with permitted element types can be list elements or dictionary values;
they are not dictionary keys or set elements.

Equality compares nominal identity, variant, and payload contents, including IEEE
float comparisons; it never calls user methods, and comparison and hashing
hooks are [rejected](../04-expressions/operators.md#equality-ordering-and-hashing).
Printing uses qualified variant names unless the enum defines
[`display` or `debug`](../../facilities/01-builtins/formatting.md#user-formatting-methods). Automatic equality and formatting reject
recursive data types.

[Matching](../04-expressions/matching.md) selects variants and binds payloads.
Standard [Option and Result](../08-errors/index.md) add prelude constructors and
contextual inference. Generic enums follow [generic data](../07-generics/data.md).

## Methods

An enum can declare ordinary methods alongside its variants:

```plenty fragment
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
Enum methods follow the class rule for
[reserved names](../04-expressions/operators.md#reserved-method-names): a
`__name__` method must be an operator hook, since enums have no
lifecycle or context hooks.
Variant and method names must be distinct. Class lifecycle hooks are not enum
methods: variant construction and automatic payload destruction retain their
existing behavior. Anonymous sums cannot declare methods. They forward only the
[`std.fmt.Formattable`](../../stdlib/report.md) contract when every member
satisfies it; arbitrary member methods and protocols are not forwarded.

Enums can satisfy [structural protocols](../07-generics/protocols.md) with
accessible matching methods. They can also supply `display` and `debug` methods
for [`std.fmt`](../../stdlib/fmt.md); builtin printing and entrypoint reports
continue to use structural rendering.
