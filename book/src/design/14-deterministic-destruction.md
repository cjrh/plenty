# Deterministic destruction

The runtime's iterative heap destruction is tested with a 100,000-record chain
whose static descriptors refer back to themselves through inline `Option`.
The sole owner transfers to a worker with a 64 KiB stack; all hooks run exactly
once with allocation disabled. This validates the runtime mechanism, not source
support for recursive declarations or a public thread API.

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

Drop order also constrains tail-call optimization. A normal call in tail position
must retain caller-owned resources through the call when their specified destruction
occurs afterward. Do not move an observable destructor before a call merely to emit
a native tail call. Initially disable that optimization when such cleanup remains,
or when a callee borrows caller-local storage. The current conservative check
uses parameter/local types, so it also disables tail calls after explicit early
drops in a function with resource-bearing slots. More precise cleanup analysis
could recover those tail calls later. Generators count as resource-bearing because
their frames may capture classes regardless of their yield type.

The destruction queue processes nested values in depth-first declaration order
without recursive native stack growth through automatic field cleanup. A hook
runs through a native ABI adapter before its fields are queued. Drops performed
inside the hook drain synchronously, preserving their order relative to its other
effects; temporarily suspending the outer queue prevents sibling cleanup from
running early. Source-level recursive hook calls can still use the native stack.
Exact cleanup traces, allocation accounting, and sanitizer tests cover this path.

Reference: [Rust destructor scopes and field cleanup](https://doc.rust-lang.org/reference/destructors.html).
These are Plenty's selected rules; they do not require copying every Rust feature.
