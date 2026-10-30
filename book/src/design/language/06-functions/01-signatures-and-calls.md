# Signatures, methods, and calls

Named functions are declared at module scope or as class or enum methods. Every
parameter and result has an explicit type, except the receiver shorthand
`self`. All signatures are available before bodies are checked, permitting
forward calls and mutual recursion. Nested named functions, overloads, and
redefinitions are unsupported.

```plenty
def clamp_low(value: i64, minimum: i64) -> i64:
    if value < minimum:
        return minimum
    value

def main() -> Result[(), IoError]:
    print(clamp_low(2, 5))?
    Ok(())
```
```output
5
```

The final expression supplies the function's result. `return value` exits from
any suite; bare `return` supplies unit. Every return must match the declared
type, and every continuing path must produce that result. The
[branch and return rules](../04-expressions/branches.md) define how continuing
paths join and unreachable statements are rejected.

A string literal that begins a body is the function's documentation, not its
final expression. A `str` function whose body is only that string is rejected
with this explanation; write `return "text"` to return the string.

Calls evaluate the callee once, then arguments once from left to right.
Parameter types supply expected types, including numeric literal context.
Owned arguments copy or move; reference arguments follow
[borrowing](../05-ownership/02-borrowing.md). A later argument's `?` cleans up
already evaluated arguments without entering the callee.

Named functions, class and enum methods, and class constructors accept
`name=value` arguments.
Positional arguments must come first; named arguments can follow in any order.
Unknown names, duplicate bindings (including a positional argument plus its
name), and missing required arguments are errors. Names bind statically; no
runtime dictionary is constructed. Supplied expressions still run in written
order, with the expected type of the parameter they name. `self` is supplied
by the method receiver and cannot also be named. Parameter names are part of
the source API.

Parameters may have defaults: `def f(x: i64, scale: i64 = 2) -> i64:`.
Required parameters must precede defaulted parameters. Each omitted expression
is evaluated afresh, after all supplied arguments, in declaration order. An
explicit value, including `Nothing`, skips that default. Names in defaults
resolve in the defining module; defaults cannot depend on parameters or `self`.
Concrete declarations are checked even when their defaults are never omitted;
generic defaults are checked on specialization.

Owned defaults produce fresh owners. For a borrowed parameter, the default
produces an owner of the referenced type and lends it for the call. For example,
`values: &mut list[i64] = []?` creates a new list for each omission. It requires
a compatible `Result` return type: allocation failure is explicit. `?` inside
a default produces an error from the called function, not its caller. Evaluated
owners are cleaned up and the body is skipped on failure. Supplying every
argument does not change the return type. A returned reference cannot borrow
an omitted temporary; pass caller-owned storage instead.

Calls through callable values retain full positional arity. Builtins and enum
payloads remain positional. Protocol requirements cannot declare defaults;
named method calls in protocol-constrained generic bodies are currently rejected
conservatively, including calls whose receivers happen to have a concrete type.
Those bodies must also supply every method argument: an implementation's
defaults are not part of its protocol contract. Generic parameters must be
inferable from supplied arguments or passed explicitly; omitted defaults do
not drive type inference.
Anonymous functions and C import/export declarations do not accept parameter
defaults. No splat unpacking or variadic parameters are
supported. Field defaults are separate from parameter defaults.

A method's first parameter is `self`. Bare `self` borrows the enclosing class
or enum immutably in ordinary methods. Class lifecycle methods `__init__` and
`__del__` instead borrow `&mut Class`; enums have no lifecycle methods.
Mutating ordinary methods declare `self: &mut Class` (or the enum type);
their receiver must be a mutable owner or exclusive
reference. Methods resolve statically. Temporary
owned receivers are supported when no reference escapes. Attempting mutation
through an ordinary method's bare `self` reports the explicit
`self: &mut Class` annotation needed for an exclusive receiver. For a generic
class, use its declared type parameters in that annotation, such as
`self: &mut Cell[T]`; the diagnostic points to these parameters rather than a
particular specialization. See
[classes](../03-types/classes.md) for initialization and field rules and
[returned references](../05-ownership/04-returned-references.md) for receiver
lifetime restrictions.

An explicit `self: Class` (or `self: Enum`) is a by-value receiver. Calling
`value.finish(args)` transfers an affine receiver just like passing it as the
first owned argument to a function; the old binding is unusable even when the
method returns an error. Copyable enums retain ordinary copy semantics. Generic
owners can use `self: Cell[T]` or the current-owner shorthand `self: Cell`.
There is no dynamic dispatch or allocation introduced by this call syntax.

The receiver is evaluated before the remaining arguments, including named
arguments. Once moved, it cannot be used in a later argument; save needed values
before the call. If a later argument propagates an error, the pending owned
receiver is cleaned up. Two-phase mutable-receiver reservation does not apply.
The callee owns cleanup unless it returns or otherwise transfers the receiver.
To support retry, return the owner in the error payload explicitly.

Owned `self` is an immutable binding, like any value parameter; use `mut owned =
self` to modify it. Consuming methods do not permit moving individual class
fields, consuming through an affine reference or indexed element, or returning
a borrow of the consumed owner's storage. Constructor/destructor receivers and
operator-hook contracts are unchanged. See the
[consuming-method lesson](../../../tutorial/06-data/consuming-methods.md).
