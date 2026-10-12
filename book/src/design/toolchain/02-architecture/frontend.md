# Frontend and checking

`frontend.rs` handles syntax, name resolution, local inference, mutability, and
lowering to typed operations.
Type references retain source positions. Aliases resolve through an iterative,
cached dependency walk before signatures are collected; alias-only cycles are
rejected without recursive traversal of long chains.

Nominal definitions reserve identities before resolving fields. Weak stored
edges and table-owning handles represent recursive type graphs without compiler
memory cycles. Concrete type facts and native metadata use finite graph walks.
A complete class or enum caches its facts; a collection or box combines its
contents' facts instead of walking again.
Aliases normalize to `Ty` values and generate no runtime operations.

Module loading canonicalizes qualified names and checks imported modules together
before producing one application object. Import traversal is bounded to depth
128 and 4,096 modules. This is not separate module compilation.

A module is lexed one top-level item at a time. The parser holds the tokens of
the item it is parsing and of the next one, so a module's tokens never all exist
beside its syntax tree (issue #54). A lexical error anywhere in a module is still
reported before any syntax error in it.

`op.rs` defines backend-neutral structured operations and an independent checker.
Its compile-time operand stack is private compiler machinery. `CompiledFn` carries
a signature, documentation, body, and typed local slots. Unit produces zero stack
values. Loads and stores address slots; exhaustive branches produce conditional values.

## Control flow and access

`Return`, `TailCall`, `Break`, and `Continue` terminate paths. The checker joins
only continuing branches, validates transfers against signatures or loop entry
stacks, and rejects operations after guaranteed exits. Loops lower to headers,
bodies, and exits; the `for` increment precedes each `continue`.

An `if` statement lowers to one operation that holds its `elif` arms as a list,
each with its own condition, so no pass recurses once per arm and a chain costs
linear time. Every pass does recurse once per level of block or expression
nesting, as do the clone and drop of the syntax tree. The parser therefore
[bounds that depth to 64](../../language/04-expressions/index.md#nesting) and
later passes rely on it instead of checking again.

Ownership checking tracks definite slot availability through branches and loop
backedges. Borrow checking flattens typed access facts into a separate source-level
CFG with branch successors, early exits, loop edges, and stable binding-place IDs.
Loan liveness reaches a fixed point before native lowering. Functions without
loan facts skip this analysis. It never runs on Cranelift IR and needs no
whole-program alias analysis or per-call inspection of function bodies.

Native lowering still consumes structured operations. Cranelift SSA variables
represent slots; stores define variables and sealed joins reconcile paths.
Source spans and marker operations carry diagnostic provenance without changing
move or loan rules. A generic instance also records the use that first required
it, so that lowering and ownership errors in its body are
[reported at that use](../../language/07-generics/index.md#diagnostics-from-instances).

## Embedding APIs

Source-string checking and compilation accept Plenty source and reject imports.
File APIs (`check_file`, `check_module_file`, and file compilation) resolve imports
from an explicit optional root. Checking performs no execution or native emission.
