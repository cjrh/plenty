# Closure environments

An explicit capture list, `def [offset, values](index: i64) -> i64:`, transfers
bindings into a concrete inline environment. Scalars copy as usual; owners move.
Immutable strings retain their ordinary sharing semantics. There is no implicit
deep copy. Capturing functions do not convert to the code-only `Callable` type.

| Capture | Meaning | Mutation |
| --- | --- | --- |
| `value` | Own the captured value | Read or borrow it |
| `mut value` | Own private state | Update the environment's value |
| `&value` | Borrow a surrounding binding | Shared access |
| `&mut value` | Borrow a mutable surrounding binding | Exclusive access |

Calls borrow the environment and can repeat. If any capture is mutable, calling
requires an exclusive environment borrow and a `mut` closure binding. An owned
capture's source binding need not be mutable; a borrowed mutable source must be.
Assignment to a captured scalar updates its slot. Captured owners support normal
field and method borrowing but cannot be moved out. Captures cannot be redeclared
or shadowed by loop, comprehension, unpacking, or match bindings in that body.
Use [a consuming closure](27-consuming-closures.md), `def once [...]`, when the
body needs to take ownership of its captures.

Assignment moves a closure; `copy` is unavailable. Captured owners drop exactly
once, in reverse capture order, when the environment leaves scope. Scope exits,
early return, `break`, `continue`, and `?` retain the usual cleanup semantics.
Creating, moving, passing, returning, calling, and dropping environments require
no heap allocation. Native tests disable allocation across the complete lifecycle,
including nested environments, typed generic consumers, and Option wrapping.
Owned environments can also reside in suspended generator frames; relocation and
cleanup there are tested with allocation disabled.
Operations performed by the body retain their own ordinary allocation contracts.

Shared capture loans prohibit owner changes or destruction until the last closure
use. Exclusive captures also prohibit competing reads and borrows. Loans follow
moves, and borrowing an environment keeps all transitive captured loans live
through the call and evaluation of later arguments. A closure may borrow another
closure under these same rules. Borrowing environments can be passed by reference;
by-value calls, escape from their creating function, owning nesting, enum/container
storage, and suspension in generators are rejected.

`Closure[[parameter types], result]` constrains an environment's signature. The
compiler infers its producer identity and specializes consumers for the concrete
layout. Different expressions do not unify merely because their signatures match.
This annotation adds neither boxing nor a dynamically sized environment. Owned
factories move results into caller-provided storage.

Generic consumers infer parameters inside the signature, including the result.
An owned environment can itself be a concrete generic argument, retaining its
layout through moves and returns. Borrowing environments use explicit
reference-to-Closure parameters or `&F`/`&mut F` with a `Callable` constraint.
The latter retains the concrete capture loans through nested generic consumers;
passing such an environment by value remains rejected.

Owned closures can capture other owned closures, ranges, and inline Option/Result
values. Nested addresses are repaired when the environment moves. Owned closures
also fit in Option/Result without allocation. Reusable closures cannot capture
generator frames; consuming closures can own and resume them.
Closures cannot be placed in heap collections, classes, tuples, or user enums.

Calls may target temporaries, such as `make(3)(4)` or `wrapped.unwrap()(4)`.
The callee is evaluated once before its arguments and kept alive until the
containing expression ends. Argument propagation releases pending owners and that
environment. A temporary can be mutated by its call. Capturing closures cannot
return references or yield, and closure calls currently retain their caller frame.

Closure values display as `<closure>` and cannot be compared. Environment nesting
is limited to 64 levels and its storage must fit the native stack-layout limit.
Environment identities are nominal; cached layout facts and visited-type traversal
avoid repeatedly expanding shared capture/type graphs during compilation.
