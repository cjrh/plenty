# Consuming and stored closures

`def once [captures](parameters) -> T:` creates an affine one-shot environment.
Calling consumes it and transfers owned captures into the body, where they can
be moved into the result. `mut value` permits changing a capture during that
call; the closure binding itself need not be mutable. Even a capture-free
`def once (...)` is affine. Calling twice, through a reference, or through a
reusable `Callable` constraint is rejected.

`OnceClosure[[inputs], result]` describes its concrete signature and producer
identity. It does not unify with reusable `Closure`. The generic constraint
`F: OnceCallable[[inputs], result]` accepts named functions, reusable closures,
and one-shot closures. A consuming callback API takes `F` by value and may
bind a mutable local to accommodate mutable reusable closures. Specialization
still checks concrete move rules: calling a one-shot argument twice is rejected.
`OnceCallable` is a constraint, not a storage type.

Uncalled environments drop captures in reverse order. After entry, the body
owns cleanup: moved result captures survive and the rest drop on exit. Argument
failure drops the pending environment without entering the body. One-shot
environments can own and resume concrete generator frames; `mut source`
permits advancing a captured frame. Creation, movement, invocation, and cleanup
of the inline environment allocate nothing.

One-shot borrowed captures permit a local consuming call. Their loans remain
live through argument evaluation and invocation and may end afterward. Such
environments cannot escape, pass by value across functions, own-nest, enter
sums/containers, or suspend in a generator. Passing one by reference does not
grant permission to consume it. Reference returns, yielding, and capture
shadowing have the same restrictions as reusable closures.

Wholly owned concrete environments can be stored in generic class fields,
tuple components, list elements, dictionary values, and generic enum payloads,
including through standard sum wrappers. A signature annotation alone does not
resolve a storage layout. Different tuple slots or enum variants may hold
different producers; a list's elements share one concrete producer type.
Borrowed captures and environments containing generators cannot enter heap
storage. A rejection names its cause: an unresolved signature, a borrowed
capture, a captured generator, a reference, or a generator. Classes and enums
themselves are inline; their construction adds no environment allocation.
Collection construction and growth remain fallible.
For different shared-call producers behind one signature, use an explicitly
allocated [BoxedCallable](05-boxed-callables.md).

Calling a stored reusable callback borrows its field or element, exclusively
for mutable captures. A live indexed callback loan protects the collection
against invalidating mutation. Moving, replacing, and dropping storage retain
ordinary ownership and cleanup rules. Values containing environments cannot
be copied or compared; environments cannot be dictionary keys or set elements.

A stored one-shot callback must first be extracted as an owner, using `pop`,
consuming iteration or matching, or tuple unpacking. Calls through references
or indexed places cannot consume it. A class field can own such a callback,
but partial moves out of classes remain unsupported. Invocation, extraction,
and cleanup add no allocation; growing its collection can allocate.
