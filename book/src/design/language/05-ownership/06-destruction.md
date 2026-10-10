# Deterministic destruction

Live owned locals drop at lexical scope exit in reverse binding declaration
order, with inner scopes before outer scopes. `return`, `?`, `break`, and
`continue` clean up the scopes they leave. Moving transfers the cleanup
obligation; moved or already dropped values are not dropped again. Replacement
destroys the previous initialized value before installing the new one; see
[assignment](../04-expressions/assignment.md) for evaluation order.

`drop(value) -> ()` consumes an owner early. It cannot destroy a borrowed
referent or consume an owner during a conflicting loan. Dropping a shared
immutable string releases that owner's share, without freeing other owners'
storage. Ending a borrow at last use does not move observable resource
destruction away from its specified scope exit or explicit `drop`.

Owned expression temporaries drop at the end of their full expression unless
transferred. A block transfers its result before destroying its other locals.
Loop conditions and each comprehension iteration finish their own temporaries.
An early propagation cleans pending evaluated operands before enclosing scope
cleanup; later operands do not run.

A class may declare `def __del__(self) -> ()` (an exclusive receiver) for
additional cleanup. The hook runs before automatic field cleanup. It cannot
return a recoverable error, yield, allow references to the dying value to
escape, or be called as an ordinary method. Fields clean up automatically:

| Owner | Cleanup order |
| --- | --- |
| Class fields and active enum payload fields | Declaration order |
| Tuple components | Component order |
| List elements | Index order |
| Dictionary entries | Insertion order, each key before its value |
| Closure captures | Reverse capture order |

A child finishes destruction before its next sibling begins. Recursive heap
cleanup requires no allocation or recursive native stack growth; source-level
recursive calls inside user hooks can still use the native stack. User cleanup
retains its own effects and allocation costs. Types with custom destruction,
including nested fields, cannot be copied. Explicit fallible shutdown should
return `Result`; a destructor provides fallback cleanup rather than reporting
shutdown failure.

Suspended or never-started generators drop their remaining captures and locals
without resuming the body. Statements following `yield` are not cleanup hooks.
[Resource scopes](../08-errors/03-resource-scopes.md) insert exit calls into
ordinary cleanup. [Tail transfers](../06-functions/05-tail-calls.md) finish caller
cleanup before entering the callee. Fatal traps and process aborts do not unwind.
