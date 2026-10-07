# Syntax and values

Spaces delimit indented suites; tabs are rejected. Indentation must return to
an existing indentation level. Blank/comment lines do not affect indentation.
Parentheses permit multiline expressions. Comments begin with `#`. Identifiers
are ASCII letters, digits, and underscores, and cannot start with a digit.
Names beginning `__plenty_` are reserved for compiler-generated functions.
Parser diagnostics carry one-based line and column positions.

Use Python spellings `def`, `True`, `False`, `and`, `or`, `not`, and `pass`.
Strings may use single, double, or triple quotes; triple quotes permit physical
newlines. The first standalone string in a function is its documentation.
To return a string directly without a docstring, use `return "text"`.

The current primitive types are `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`,
`u64`, `bool`, and `str`. Numeric built-ins use explicit-width names; there is
no built-in `int`. Floating-point types are `f32` and `f64`. Unsuffixed integer
literals default to `i64`; decimal/exponent literals default to `f64`. Suffixes select
widths, including `1f32`, `1.5f32`, and `1e-3f64`. Decimal forms such as `.5`
and `1.` are also accepted.
Numeric annotations, parameters, returns, and typed arithmetic operands guide
unsuffixed literals within the expression. Already typed values and suffixed
literals must match exactly; there is no implicit numeric widening.

The [typed-range and inference proposal](../../proposals/typed-ranges-and-expression-inference.md)
recommends square-bracket function type arguments, deferred literal defaults,
and context flowing through one initializer or return expression. It also records
integer-family constraints, unsigned range boundary choices, and requirements
for future multiline closures. Broader inference remains proposed; the numeric rules
in this section continue to describe implemented behavior.

Integer casts use truncation/sign-extension like the historical backend.
Integer overflow and division by zero are runtime errors. `//` floors signed
integer quotients, including negative operands; `%` is also integer-only.
`/` accepts same-width floats. Floating-point arithmetic uses IEEE semantics:
signed zero, infinities, NaN, and ordinary rounding, without fast-math rewrites.
Float literals must fit their width; runtime arithmetic can overflow to infinity.
Underflow follows the target's IEEE behavior. Float-to-integer casts truncate
toward zero and saturate to the target range; NaN becomes zero. Integer-to-float
casts and narrowing floats may round; widening `f32` to `f64` is exact.
Comparisons require equal types; ordering accepts integers and floats. NaN
compares unequal to every value, including itself, and all ordered comparisons
with NaN are false. These rules also apply to structural equality and membership.
Float printing uses shortest round-trip Rust debug formatting, including a decimal
point for whole finite values and `inf`, `-inf`, and `NaN`.
Floats cannot be dictionary keys or set elements. Chained comparisons remain rejected.

There is **no `None` type or value**. The unit type `()` means a computation
completed without producing data, and lowers to no result register. It is
supported for expressions, function returns, and enum payloads, including
`Result[(), E]` and `Option[()]`. Standalone unit bindings, parameters, collection
elements, and class fields remain unsupported. Absence and recoverable failure
use explicit sum types: `Some(value)` / `Nothing` and `Ok(value)` / `Err(error)`.
`Ok(())` means success without data, not absence.
