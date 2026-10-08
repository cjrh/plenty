# Consuming closures

`def once [values](...) -> T:` creates an affine, one-shot closure. Construction
moves owned captures into an inline environment, exactly as for reusable closures.
Calling it consumes the environment and transfers captured owners into ordinary
body parameters. The body can return or otherwise move those owners. `mut value`
permits changing a captured value during that call. The closure binding itself
does not need `mut`; it is consumed, not borrowed.

Calling twice, calling through a reference, or passing it to a reusable `Callable`
constraint is rejected. A capture-free `def once (...)` is still affine. Captured
names cannot be shadowed. Reference returns, yielding, and heap storage retain the limits of
[reusable environments](26-closures.md).

Uncalled environments drop their captures in reverse order. When called, the body
owns cleanup: captures that are moved into the result remain alive, and all other
captures drop on exit. Argument failure before entry drops the pending environment.
There is no heap allocation for creating, moving, calling, or dropping the closure.

`OnceClosure[[parameter types], result]` is the explicit signature annotation for
a consuming environment. Like `Closure`, it retains the concrete producer layout
and specializes consumers. Factories, generic signature inference, moves through
inline Option/Result, and temporary calls such as `factory(value)()` preserve that
identity. `OnceClosure` and reusable `Closure` do not unify.

The generic constraint `F: OnceCallable[[inputs], result]` accepts named functions,
reusable closures, and one-shot closures with that signature. A consuming API
takes `F` by value. It can bind `mut callback = f` to also support mutable reusable
environments. Concrete specialization still checks every move and borrow, so
calling a one-shot argument twice is rejected even inside a generic function.
The signature supplies the same inference evidence as `Callable`; a fully concrete
bound can specialize a generic named function value. `OnceCallable` is a constraint,
not a storage type. `Callable` constraints accept only reusable callbacks.

Owned one-shot environments can capture other owned closures and concrete
generator frames. `mut source` allows resuming a captured generator; a called or
uncalled environment releases the remaining frame exactly once. Nested inline
addresses are repaired across environment moves, Option/Result wrapping, and
suspension inside a generator. Native tests exercise these paths with allocation
disabled. Printing a closure shows `<closure>` without inspecting its captures.

One-shot closures may also capture `&value` and `&mut value` for a local call.
Borrowed captures stay references while owned captures transfer into the body.
Capture loans, including transitive ones, remain live through argument evaluation
and invocation, and can end after that call. These environments cannot escape,
cross a function boundary by value, own-nest, enter sums/containers, or suspend
inside a generator. Passing them by reference cannot grant permission to consume
them; use a local call while their captures are in scope.
