# Resource scopes

`with expression as name:` evaluates an owned manager once and moves it into
a hidden mutable owner. A concrete class manager exposes visible methods
`__enter__(self: &mut Class) -> T` and `__exit__(self: &mut Class) -> ()`,
with no other parameters. The entry result becomes a body-local binding.
Without `as`, an unused owned result drops before the body. Entry is an
ordinary call: it does not implicitly unwrap a Result. Perform fallible
acquisition with `?` in the manager expression.

Comma-separated managers nest left to right. Each acquisition and entry
completes before evaluating the next expression, which can use earlier entry
bindings. If a later acquisition propagates failure, only already entered
managers exit.

On leaving the body, its locals drop in reverse order, then `__exit__` runs
once, then the manager drops normally. Nested managers exit in reverse order.
Return values are evaluated and preserved before cleanup. `return`, `?`,
`break`, and `continue` use this order for the scopes they leave; control
inside an inner loop does not exit an outer manager. `__del__` is a separate
fallback hook, so exit and later destruction must be safe together. Exit is
not an implicit flush or durability guarantee; fallible operations expose
their own Results.

`with &mut existing:` instead reborrows a mutable named manager or class field.
The exclusive loan lasts through `__exit__`, preventing competing access,
replacement, and movement. After exit the caller retains its mutated manager;
its destructor remains at the owner's normal scope boundary. An existing
exclusive reference must also be explicitly reborrowed with `&mut`.

`__enter__` may return `&T` or `&mut T` under the
[returned-reference contract](../05-ownership/04-returned-references.md).
The entry binding carries that loan and must finish before exit: it cannot
escape an owned context or stay live across a borrowed manager's exit call.
Owned entry results may move out normally.

Resource-scope bookkeeping adds no runtime allocation; manager methods retain
their own costs. `yield` inside `with` is unsupported. Traps and process aborts
do not unwind managers. Intrinsic thread, channel, and executor managers use
the same scope-exit obligations; their shutdown contracts are described under
[concurrency](../../facilities/03-concurrency/index.md).
