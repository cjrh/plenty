# Frontend and checking

`frontend.rs` handles syntax, name resolution, local inference, mutability, and
lowering to typed operations.
Type references retain source positions. Aliases resolve through an iterative,
cached dependency walk before signatures are collected; alias-only cycles are
rejected without recursive traversal of long chains.

Nominal definitions reserve identities before resolving fields. Weak stored
edges and table-owning handles represent recursive type graphs without compiler
memory cycles. Concrete type facts and native metadata use finite graph walks.
Cloning a structural type shares its already-owned child storage. When a field
escapes a nominal table, cloning promotes its weak edges and copies only the
structural paths that contain them. This is a single pass through the structural
type, without following nominal table links; stored definitions still keep weak
edges so sharing cannot create a table ownership cycle. Callable and boxed
callable signatures follow the same rule: a signature is shared unless an input
or the output holds a weak edge.
A complete class or enum caches its facts; a collection or box combines its
contents' facts instead of walking again.
Aliases normalize to `Ty` values and generate no runtime operations.

The compiler's own hash tables (`src/hash.rs`) use FxHash rather than std's
SipHash. Their keys are names and types from the program being compiled, so
flood resistance buys nothing, and SipHash was about 11% of checking
instructions (issue #302). Iteration order stays unspecified.

Module loading canonicalizes qualified names and checks imported modules together
before producing one application object. Import traversal is bounded to depth
128 and 4,096 modules. This is not separate module compilation.

A module is lexed one top-level item at a time. The parser holds the tokens of
the item it is parsing and of the next one, so a module's tokens never all exist
beside its syntax tree (issue #54). A lexical error anywhere in a module is still
reported before any syntax error in it.

The whole syntax tree is live when lowering starts, so its layout sets much of
checking's peak memory. Blocks, call arguments and parameters are collected on
parser scratch stacks and stored at their exact length. Every node holds a
40-byte token that names its module by an interned file number, not a
reference-counted path (issue #422).

`op.rs` defines backend-neutral structured operations and an independent checker.
Its compile-time operand stack is private compiler machinery. `CompiledFn` carries
a signature, documentation, body, and typed local slots. Unit produces zero stack
values. Loads and stores address slots; exhaustive branches produce conditional values.

The checker collects every signature on one thread, then checks function bodies
on all available cores: a body reads only the signature table. It reports the
first error in program order whichever body failed first, so diagnostics do not
depend on the core count. Operations, types, and signatures are shared between
threads without copying.

Name resolution of a module's functions, and of its classes' fields and
methods, runs on all cores when the module has at least 256 functions or 64
classes: each reads only the module's scope. Frame layout and generator
operation validation after lowering also run on all cores. Both report the
first error in source or program order, as a serial pass would.
Threads share generator, closure, and nominal layout caches. A cached layout
records how deep its frames and environments nest, and is reused only where it
cannot hide the nesting limit of 64; otherwise the type is measured again. Each
function's layout error is therefore the same whichever thread measured a type
first (issue #126).

Parsing, the rest of name resolution, type resolution, class expansion,
generic preparation, signatures, and lowering bodies to operations run on one
thread (issue #59). Type resolution can create generic data instances in a
table shared by every module, and lowering interns strings, records generic
instances, and infers returned fields in tables shared by every body. Generic
preparation resolves each concrete function's parameter and return types once;
registering the function's signature reuses them.

Three ways to run these phases on several threads were measured and not
adopted. Each gained little and raised peak memory, because each thread's
allocator keeps its own free memory:

- Parsing one module in pieces: for 5,000 units, parsing took 0.09 s instead
  of 0.21 s, lowering took 0.06 s longer, and peak memory rose from 417 MB to
  538 MB.
- Parsing imported modules in parallel, a level of imports at a time: for a
  program whose imports load 21 standard modules (12,549 lines), parsing took
  9 ms instead of 23 ms and lowering 5 ms longer; the check was no faster, and
  peak memory rose from 28 MB to 35 MB.
- Lowering bodies speculatively on all cores and keeping them in queue order:
  checking 5,000 units was about 9% faster and object emission no faster, and
  peak memory rose 12–14%.

[Measuring compilation](emission.md#measuring-compilation) gives the time each
phase takes.

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
Each CFG node stores up to two successors inline; only matches with more than
two arms allocate a separate successor slice. Edges retain source branch order,
including duplicate successors when empty arms join the same continuation.
Loan liveness reaches a fixed point before native lowering. Functions without
loan facts skip this analysis. It never runs on Cranelift IR and needs no
whole-program alias analysis or per-call inspection of function bodies.

Native lowering still consumes structured operations. Cranelift SSA variables
represent slots; stores define variables and sealed joins reconcile paths.
Only slots whose address is taken (`&x`, `&mut x`) or whose values have inline
storage live in the function's stack frame; the frame holds those slots alone.
Augmented assignment of a local (`x += 1`) takes no address, and starting a
thread adds no slot: task storage has its own frame space and a worker reaches
parent slots only through borrowed arguments. Generators keep every slot in
memory.
Source spans and marker operations carry diagnostic provenance without changing
move or loan rules. A generic instance also records the use that first required
it, so that lowering and ownership errors in its body are
[reported at that use](../../language/07-generics/index.md#diagnostics-from-instances).

## Embedding APIs

Source-string checking and compilation accept Plenty source and reject imports.
File APIs (`check_file`, `check_module_file`, and file compilation) resolve imports
from an explicit optional root. Checking performs no execution or native emission.

## Editor queries

`ide::analyze`, behind `plenty lsp`, runs the same load, resolution, and
lowering with a per-thread recorder switched on (`src/frontend/ide.rs`).
Module loading records modules and imports; name resolution records
declarations, module-level names, and locals with their binding positions;
lowering records what needs types: methods resolved through the receiver,
fields, enum variants, and the types of locals. An ordinary compilation
records nothing. Expressions keep only the position of their first token, so
member expressions also carry the position of the member name. In this mode an
error in one function's resolution or lowering is recorded and checking moves
to the next function. See the [language server](../01-building/language-server.md).
