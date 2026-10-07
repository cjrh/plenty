# Concrete context managers

`with expression as name:` evaluates and moves an owned class instance once into
a hidden mutable local. The class must expose `__enter__(self: &mut Class) -> T`
and `__exit__(self: &mut Class) -> ()`, with no additional parameters. The `as`
binding receives the entry result and is confined to the body. Entry may return
unit if `as` is omitted; an unused owned entry result is dropped before the body.
Entry is an ordinary infallible call, not implicit Result unwrapping: perform
fallible acquisition with `?` in the manager expression. Visibility rules apply
to both methods.

Comma-separated managers (`with a() as x, b(x) as y:`) are nested scopes: each
acquisition and entry completes before evaluating the next expression, which
can use earlier entry bindings. If a later acquisition propagates failure,
only the already entered contexts exit.

Body locals drop in reverse order, then `__exit__` runs exactly once, then the
manager drops normally. Nested contexts exit in reverse order. Return values
are evaluated and preserved before cleanup; early `return`, `?`, `break`, and
`continue` use the same ordering. Inner-loop control flow does not exit an outer
context. `__del__` remains a separate fallback destructor; resource authors must
make exit plus destruction safe. Traps and process aborts do not unwind scopes.
Exit is not an implicit flush or a durability guarantee. Fallible operations
must expose their own Results.

The typed propagation operation carries its lexical cleanup operations; its
failure edge participates in ownership and loan analysis. The native backend
releases pending expression operands, runs this cleanup, then releases remaining
locals before returning the residual. Compiler exit bookkeeping adds no runtime
allocation. This does not constrain allocations inside user methods.

`with &mut existing:` reborrows a mutable named manager or class field. Its
exclusive loan lasts through `__exit__`; moving, replacing, or accessing the
original manager conflicts during the body. After exit the caller retains the
manager with its mutations, and its destructor runs at its owner's usual scope
boundary. An existing reference must also be explicitly reborrowed with `&mut`.

`__enter__` may return `&T` or `&mut T` using the single-reference-parameter
contract. The `as` binding carries that loan, allowing the common `&mut self`
result without moving the manager out of its hidden owner. The loan ends before
exit; it cannot escape an owned context or remain live across a borrowed
manager's exit call. Owned entry results may still be moved out normally.

`yield` inside
`with` is rejected until generator frames can retain exit obligations. Concrete
lookup requires no traits or user generics.
