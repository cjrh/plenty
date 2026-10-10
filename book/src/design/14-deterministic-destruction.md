# Deterministic destruction

The runtime's iterative heap destruction is tested with a 100,000-box chain
whose static descriptors refer back to themselves through `Option[Box[...]]`.
The sole owner transfers to a worker with a 64 KiB stack; all hooks run exactly
once with allocation disabled. Source-level tests also build boxed recursive
class and enum chains of 100,000 nodes each and drop both on a 256 KiB native
stack with allocation disabled. This does not provide a public thread API.

Plenty uses ownership-driven destruction, without a tracing garbage collector.
The compiler inserts cleanup on ordinary control-flow exits. This applies to memory
and custom class cleanup, including owned file handles. General foreign handles
remain future library work.
Reference-counted immutable string storage is compatible with this model: releasing
a string owner decrements its count and frees dynamic storage at zero. Borrows do
not acquire ownership or independently destroy their referents.

The runtime reclaims built-in values, classes, and abandoned generator frames;
`drop(value)` optionally consumes an owner early. A class may implement
`def __del__(self) -> ()` for custom cleanup; automatic field cleanup follows it.

- A live owned local is dropped when its lexical scope exits, in reverse binding
  declaration order. Inner scopes clean up before outer scopes. This includes
  `return`, `break`, and `continue` for the scopes each exit crosses.
- Moving a value transfers its cleanup obligation; the moved-from place is not
  dropped. At a branch join, cleanup is conditional on whether the place still
  holds a live value. Replacement evaluates the RHS first, destroys the previous
  initialized value, then installs the new one.
- The `drop(value) -> ()` builtin consumes an owned value and performs early
  destruction. It cannot destroy a borrowed referent or consume an owner while a
  conflicting loan is live. For a shared immutable string it releases that owner's
  share; it cannot force other owners' storage to be freed.
- Borrow lifetimes may end at last use. Observable destruction of owned resources
  remains at the specified scope exit or explicit `drop`, not an optimizer-chosen
  last use. Borrow checking treats destruction as an exclusive access to the owner
  and everything its cleanup may access.
- Owned expression temporaries clean up at the end of their full expression unless
  transferred to another owner. A block's result is transferred before its other
  locals are dropped. Initially reject escaping borrows of temporaries rather than
  adding implicit temporary-lifetime extension rules.
  Loop conditions and each comprehension iteration finish their own temporaries.

`__del__` takes exclusive access to the value and returns `()`. A general trait
system is not required. The hook runs before automatic field cleanup. Class fields and active
enum payloads drop in declaration order; list elements drop in index order. Dictionary
entries drop in insertion order, with each key before its value. A child finishes
destruction before the next sibling begins. Types with custom destruction cannot be
copied, even explicitly, and cannot be partially moved or have their destructor
called directly as an ordinary method. The hook cannot let references to the dying
value escape. Nested owned fields are cleaned automatically; hooks manage only the
additional resource-specific work.

Drop hooks have no recoverable return value and cannot yield. Resources that need to
report shutdown errors should also offer an explicit operation returning `Result`;
their destructor provides fallback cleanup. An internal state records an already
closed resource so explicit close followed by drop does not release it twice.
Normal `Result` error paths still run scope cleanup. Fatal traps and process aborts
do not unwind, so cleanup is not promised in those cases.

A suspended generator retains its live owned locals until resumption, completion,
or destruction. Destroying its frame cleans those values without resuming the body;
code following a `yield` is not a cleanup hook. Observable cleanup must follow the
same scope and field rules, including captures in a never-started frame.

A call in tail position transfers control the way Rust's `become` does:

1. Its arguments are evaluated left to right. A value moved into an argument
   belongs to the callee; the caller does not drop it.
2. The statement's temporaries, then the caller's remaining owned parameters and
   locals, are dropped in the order of an ordinary function exit.
3. Control transfers to the callee.

The caller's destructors therefore run before the callee's body, and a
destructor-bearing parameter or local does not prevent a native tail call. Values
already moved or explicitly dropped are not dropped again, on any path. An
argument that propagates with `?` takes the ordinary error exit: evaluated
arguments and locals are dropped and the callee is never entered.

Three kinds of call are not tail transfers and keep cleanup after the call:

- A call passing a reference into the caller's own storage: a local, an owned
  parameter, or a temporary, including a method call on one. That storage must
  outlive the call, so every local does. One such argument makes the whole call
  ordinary.
- A `return` inside a `with` block. The call returns, then the pending context
  exits and joins run, then the locals are dropped.
- A call whose result is used further, for example wrapped in `Ok`, propagated
  with `?`, or combined by an operator.

A reference argument does not prevent the transfer when it refers to storage
the caller itself borrowed through a reference parameter, because that storage
belongs to an older frame. This covers the parameter, a reborrow or local
reference binding of it, a field or element reached through it, a payload bound
by matching on it, an item of a borrowed iteration over it, and the reference
result of a call that received it. The compiler decides this for each argument
from the origin of its loan and records the result on the call; code generation
does not infer it from types. A method call through a `self` reference and a
closure body passing a borrowed capture qualify the same way.

A call passing or returning inline storage follows the three steps above but
keeps the caller's frame to hold that storage, so it still uses native stack for
each call.

Values stored inline, such as class instances, tuples, and enum payloads, drop
immediately in declaration order; their nesting depth is bounded by their type.
Boxes and collections join the destruction queue, which processes them in
depth-first declaration order without recursive native stack growth. When a
value being destroyed from the queue holds a box or collection, that heap child
finishes after the value's later inline siblings. A hook runs through a native
ABI adapter before its fields are released. Drops performed
inside the hook drain synchronously, preserving their order relative to its other
effects; temporarily suspending the outer queue prevents sibling cleanup from
running early. Source-level recursive hook calls can still use the native stack.
Exact cleanup traces, allocation accounting, and sanitizer tests cover this path.

Reference: [Rust destructor scopes and field cleanup](https://doc.rust-lang.org/reference/destructors.html).
These are Plenty's selected rules; they do not require copying every Rust feature.
