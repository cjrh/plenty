# Named and anonymous function values

A named function can be assigned, stored, passed, and returned. To accept a
callback, take a type parameter bound by `Callable` and a parameter of type
`&F`, or `&mut F` when the callback changes a capture:
`def apply[F: Callable[[i64], i64]](f: &F, value: i64) -> i64`. This form
accepts named functions, capture-free anonymous functions, and
[closures](03-closures.md); see [generic functions](../07-generics/index.md).
Parameter names do not affect a signature. `Callable[[], T]` takes no
arguments, and `Callable[[T], ()]` returns unit. Aliases may name these
signatures.

`Callable[[parameter types], result]` is also a value type: a code address with
no environment, the type of a named function or capture-free anonymous
function. A capturing closure also needs an inline environment, so a concrete
`Callable` parameter or field cannot hold one; use the bound form above, or a
[boxed callable](05-boxed-callables.md) for storage. Function values are
copyable, display as `<function>`, and compare native code addresses rather
than behavior. They cannot be dictionary keys or set elements. A payload enum
variant written without arguments, such as `AppError.Io`, and a class
constructor, such as `Point`, are also function values; see
[enums](../03-types/enums.md) and [classes](../03-types/classes.md).
Generic constructor values specialize from an explicit data instance or an
expected `Callable`, including prelude `Some`, `Ok`, and `Err`. Like named
functions, constructor values can be borrowed as `&Point` or `&Reading.Value`;
`&mut` borrows a fresh copy of the code address.

An arbitrary callee expression is allowed: `choose()(n)`, `(f)(n)`,
`handlers[index](n)`, and `record.callback(n)`. The callee evaluates before its
arguments. Brackets following a local binding or a
[module constant](../02-source/modules.md#constants) index it; brackets following a
function declaration or module path provide type arguments. Following a field
or method on a value, brackets index when their contents start at a local
binding and otherwise provide method type arguments.

Take a generic function value with explicit specialization, such as `identity[u8]`,
or let a concrete expected `Callable` from an annotation, parameter, or return
specialize the bare name. Both input and result positions provide evidence;
unconstrained type parameters need explicit arguments. Signatures do not
implicitly convert, and this does not introduce higher-rank polymorphism.
[Generic inference](../07-generics/inference.md) also works inside callable
inputs and results.

An anonymous function is `def(parameters) -> T:` followed by a body. Its
position fixes the form of the body: outside brackets the body is an indented
block; inside `()`, `[]`, or `{}` it is one expression, whose value is returned.

```plenty
def apply[F: Callable[[i64], i64]](f: &F, value: i64) -> i64:
    f(value)

def main() -> Result[(), IoError]:
    twice = def(value: i64) -> i64:
        value * 2
    print(twice(3))?
    print(apply(&(def(value: i64) -> i64: value + 1), 3))?
    Ok(())
```
```output
6
4
```

The one-expression body is the final expression of a block, so both forms
create the same value and follow the same type, capture, and ownership rules.
Only the layout differs:

- The expression extends as far as it can and ends at the `,` or closing
  bracket of the enclosing element. Line breaks inside brackets do not end it.
- Statements have no one-expression form: `return`, a binding, an assignment,
  or a second expression is rejected. Give such a function an indented body and
  bind it to a name first.
- A string body is the returned value, not documentation.
- Outside brackets a body on the `def` line is rejected. Parentheses are
  brackets: `twice = (def(value: i64) -> i64: value * 2)`.

In both forms an anonymous function must be a complete expression: a bound or
returned value, an argument, an element, or the contents of parentheses. As the
operand of an operator, including `&` and `&mut`, it needs parentheses:
`&(def [offset](value: i64) -> i64: value + offset)`. Parameter and result types
are always written. `lambda` is reserved and rejected with the `def` spelling.

Capture-free anonymous functions have ordinary `Callable` types, can nest, and
can be returned. Their parameters and locals form a separate scope; enclosing
generic type parameters are substituted. Implicit capture of an enclosing
local, including one shadowing a module function, is rejected. Use an explicit
[capture list](03-closures.md) for environments.

`Callable` signatures exclude generator frames and closure environment values.
A named function can be borrowed directly: `apply(&double, value)` or
`callback = &double`. Each borrowed declaration or explicit specialization,
such as `&identity[u8]`, has one immutable code-address slot in the borrowing
function, initialized at entry without allocation. Its references follow the
ordinary local borrow and escape checks. A local binding shadowing a function
name takes precedence. A generator can borrow one for a call, but a reference
to that local slot cannot remain live across `yield`.
Generic reference bindings require explicit specialization;
a concrete callable parameter can instead supply the expected signature.
`&mut double` borrows a fresh copy of the code address, so a named function also
passes to a `&mut F` parameter; distinct `&mut` borrows of one declaration do
not conflict because a function has no state.
`Callable` constraints, as distinct from storage types, accept reusable named
functions and concrete captured closures.
