# Named and anonymous function values

A named function can be assigned, stored, passed, and returned. Its type is
`Callable[[parameter types], result]`; parameter names do not affect identity.
`Callable[[], T]` takes no arguments, and `Callable[[T], ()]` returns unit.
Aliases may name these signatures. Function values are copyable, display as
`<function>`, and compare native code addresses rather than behavior. They
cannot be dictionary keys or set elements.

An arbitrary callee expression is allowed: `choose()(n)`, `(f)(n)`,
`handlers[index](n)`, and `record.callback(n)`. The callee evaluates before its
arguments. Brackets following a local binding index it; brackets following a
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

Multiline anonymous functions use `def(parameters) -> T:` with an indented body:

```plenty
def main() -> Result[(), Failure]:
    twice = def(value: i64) -> i64:
        value * 2
    print(twice(3))?
    Ok(())
```

Capture-free anonymous functions have ordinary `Callable` types, can nest, and
can be returned. Their parameters and locals form a separate scope; enclosing
generic type parameters are substituted. Implicit capture of an enclosing
local, including one shadowing a module function, is rejected. Indented anonymous
bodies inside delimiters are unsupported. Use an explicit
[capture list](03-closures.md) for environments.

`Callable` signatures exclude generator frames and closure environment values.
A named function value can itself be borrowed through a reference parameter.
`Callable` constraints, as distinct from storage types, accept reusable named
functions and concrete captured closures.
