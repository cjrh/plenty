# Frontend and checking

`frontend.rs` handles name resolution, local inference, mutability, and
lowering to typed operations. Tokens and the lexer (`frontend/lexer.rs`), the
syntax tree (`frontend/syntax.rs`) and the core parser (`frontend/parser.rs`)
read no lowering state; feature modules add their own parsing beside their
lowering.
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
Generic instance arguments distinguish `Type(Ty)` from typed constant values;
values never become storage types. The constant evaluator also serves value
arguments during type resolution, before ordinary module-constant folding.
Quantity constant output types resolve lazily with cycle detection. The
[`Bounded` checker](../../language/03-types/bounded.md) validates each concrete
interval and lowers admitted literals, checked conversions and `from` on a
[proved range](../../language/03-types/bounded.md#proved-ranges) to a retyping
of the integer (`ClassOp::WrapBounded`), not a constructor call. `from`
compares the argument's [interval](../../language/03-types/intervals.md) with
the bounds.

The checker computes intervals while lowering (`src/frontend/intervals.rs`,
with the arithmetic in `src/interval.rs`). Lowering an integer expression
leaves its interval in a one-entry register keyed by the expression's syntax
node; the operator that lowered it as an operand reads the register, and any
other consumer finds a different node and gets the type's full range. An
immutable local's interval is stored by slot when its binding is lowered.
Bounds are `i128`, and a bound that does not fit proves nothing. An operation
proved infallible is emitted as `Op::Proved`, or as a division with a
`Divisor` proof, and adds nothing to the function's or block's error set: one
decision both drops the error and selects the unchecked instruction.

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
later passes rely on it instead of checking again. Every shape at the limit
compiles on a 2 MiB thread stack, the default for a spawned Rust thread, even
in an unoptimized compiler. Lowering walks the left spine of an `and`/`or`
chain in a loop, and the functions it recurses through per level (`expr`,
calls, methods, operand observation and expected-type lowering) keep only
dispatch in their own frames: an unoptimized build gives every local of every
match arm its own stack slot, so the other arms are separate functions whose
frames are gone before the next level starts. The parser is then the deepest
pass for nested blocks and calls. Per level of a nested `match` its frames
total about 28 KiB, 21 KiB of which is `Parser::statement`; per nested call
they total about 22 KiB, 19 KiB of which is `Parser::expr_inner`.

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

## Std ceiling checking

`frontend/ceilings.rs` checks fixed upper bounds without introducing value
generics or another integer type. Embedded std source can append
`__max(1024)` to a parameter or field declared `Positive[i64]` or
`NonNegative[i64]`. The decimal non-negative bound is metadata on its
`TypeRef`; resolved types, layout, equality and callable signatures are
unchanged. The marker is rejected in user declarations and is hidden in public
editor signatures and the generated std signature index.

Direct function, method and generated class-constructor calls check their
supplied arguments before named/default wrappers replace expressions with local
names. Defaults are checked at their declaration. Direct field assignments and
`replace(owner.field, value)` use the same check, including through borrowed or
boxed owners. The module-constant evaluator handles literals, module constants
(including integer-domain and Quantity constants), and supported constant
operators/casts. Constant folding runs after type resolution so quantities
retain normalized dimensions and storage types during evaluation.
Ordinary local bindings, arbitrary function calls and explicit fallible domain
conversions are not constant evaluation. Indirect callable invocations and
writes through a separately borrowed field do not retain this metadata.
Existing std runtime guards cover values that reach operations through those
paths; this mechanism does not claim a proof-carrying bounded integer type.

The annotations cover the fixed ceilings enforced by the std limit-domain
migration: format/JSON/TOML/XML/template/regex nesting, fs/tempfile cleanup depth
and attempts, sketch construction limits, unicode scalar limits, JWT token size,
HKDF output size, image dimensions, actor heaps and procfs read sizes. Decoded sketch
`max_capacity` has no ceiling. General bounded integers and lower bounds above
one remain [B96](../../../backlog.md).

The same mechanism checks open modes. `__open_mode` after a `str`, `&str`, or `&strview`
parameter or field requires an argument the module-constant evaluator can fold
(a literal, a module constant or a constant expression) to pass the runtime's
`OpenMode::parse`, the check builtin `open` applies to its mode.
`std.io.open_file` and `open_path` use it, so their invalid literal modes are
compile errors with the builtin's diagnostic. Like `__max`, it is reserved for
std declarations and hidden from public signatures.

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
