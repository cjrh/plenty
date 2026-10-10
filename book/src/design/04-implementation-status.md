# Implementation status

This page records current capabilities and limits, not task priority. Future
work is tracked only in the [backlog](../backlog.md).

The core language can compile single- and multi-file programs: typed functions,
control flow, collections, classes, sum types, generators, ownership, and automatic
cleanup are implemented. It is still an early language implementation, with a
small built-in library and important limits on borrowing. Basic console, argument,
numeric text, and whole-file APIs now work. Important remaining gaps include
broader borrowing, richer worker effects, and public allocator control. An implemented row below describes the
supported subset, not Python's full API or Rust's full ownership system.

| Area | Status on this branch |
| --- | --- |
| Python-shaped lexer/parser, typed function declarations | Implemented |
| Infix expressions, conditional expressions and blocks | Implemented |
| Immutable bindings and explicit `mut` reassignment | Implemented |
| Nested assignment | List elements, existing dictionary values, and class fields; RHS then indices once in path order, delayed address resolution, no enclosing copies or allocations |
| Checked sized integers, booleans, strings, unit returns | Implemented |
| Floating-point types and arithmetic (`f32`, `f64`, `/`) | Implemented with IEEE arithmetic and explicit numeric casts |
| Transparent module-level type aliases | Implemented |
| Function values | Named functions, explicit generic specializations, and capture-free multiline anonymous functions use allocation-free `Callable[[parameters], result]` values; indirect calls preserve ownership and returned-reference loans |
| Closure captures | Explicit owned/shared/exclusive captures, mutable state, transitive loans, concrete `Closure` / `OnceClosure` signatures, factories, nested environments, Option/Result storage, and temporary calls; complete lifecycle tested with allocation disabled |
| Consuming closures | `def once [...]` transfers owned captures into the body; local borrowed captures, nested environments, and captured generator frames retain checked cleanup and allocation-free storage |
| Higher-order APIs | `Callable` and `OnceCallable` generic constraints accept concrete callbacks with signature inference; borrowing consumers preserve capture loans, and consuming consumers retain move checking |
| Stored closures | Concrete owned reusable/consuming environments in generic class/enum fields, tuples, lists, and dictionary values, with checked invocation, extraction, replacement, and failure cleanup; see [stored closures](33-stored-closures.md) |
| Cranelift AOT and compile-and-run file command | Implemented |
| Native linker configuration | CLI `--linker` / `--link-arg` and Rust `CompileOptions`; cc-compatible driver interface, default `cc` |
| Native object output | `--emit-object` / Rust APIs emit an application object without linking; `--emit-runtime` extracts the matching archive and native dependencies for external linking |
| Native target validation | `x86_64-unknown-linux-gnu` only; requested target, packaged runtime, and Cranelift ISA must agree before emission. `--print-target` reports the packaged target |
| Explicit binary `main` entry point | Implemented: parameterless `main` returns `()`, `i32`, `Result[(), E]`, or `Result[i32, E]`; a returned `Err` is reported on standard error with status one; module scope contains declarations and imports |
| Rust runtime, embedded precompiled archive | Implemented; runtime compilation happens when building Plenty |
| Direct and mutual tail calls | Implemented; caller cleanup, including destructors, runs before the transfer. Direct recursive source-tail calls without an approved lifetime/ABI plan are compile errors; non-tail and indirect recursion remain stack-limited |
| Early returns and return-aware branch checking | Implemented in AOT |
| Concrete enums, tagged payloads, exhaustive matching | Implemented, including fallible `Enum.Variant(...)` |
| Generic data declarations | Concrete enums and classes with cached specialization, aliases, inferred class construction, nested payloads, IntType/Callable/OnceCallable constraints, generic methods, and ordinary ownership rules; see [generic data types](28-generic-data-types.md) |
| Fixed-layout classes, constructors, methods, custom cleanup | Implemented, including checked `Class(...)`, fallible initializers, and partial-field cleanup |
| `Option[T]`, `Result[T, E]` | Implemented with allocation-free inline wrappers, unit payloads, and unqualified `Some`, `Nothing`, `Ok`, `Err` |
| Unit values | Expressions, function returns, enum and tuple payloads implemented; standalone bindings, parameters, and collection/class storage deferred |
| Value reclamation, owned moves, explicit copy/drop | Implemented |
| Local/parameter references and last-use borrow checking | Bindings, disjoint class fields, collection elements, and returned references tied to one reference parameter; stored references deferred |
| Inline values | Class instances, tuples, and enum values store their fields inline in their owner's storage; construction never allocates. Inline storage is limited to 64 KiB per type; [reference](12-classes-fixed-layout-records.md) |
| `Box[T]` | One owned heap value; `Box(value)` returns `Result`, a box converts to its content wherever the content's type is required, and a boxed class's fields and methods are reached directly; [reference](30-recursive-data.md) |
| Borrowed enum matching | Shared matches preserve owners; mutable matches update every enum's payloads. Payload loans protect variants, support restricted returns, and allocate nothing; [reference](31-borrowed-enum-matching.md) |
| Interpreter, REPL, JIT | Out of scope |
| Lists, dictionaries, sets, ranges, `for`, comprehensions | Implemented |
| Allocation-free ranges | `range(...)` and `range[T](...)` return inline range values directly; calls, returns, standard sums, indexing, membership, and repeated iteration need no range allocation |
| Borrowed collection iteration | Shared list loops borrow owned elements; mutable list loops yield mutable element references. Shared copyable elements and dictionary keys remain values |
| Tuples and unpacking | `(a, b)`, `(a,)`, `tuple[A, B]` / `(A, B)` annotations, literal indexing, flat binding/loop unpacking, and inline tuple construction |
| Collection convenience APIs | Basic indexing, membership, append/add, updates, keys/values, optional list/dictionary `get`, list/dictionary `pop`, set `discard`, and fallible forward list slices; slice syntax and steps are deferred |
| Text convenience APIs | Length, indexing, iteration, concatenation, equality, membership, fallible joining, forward slicing, literal replacement, explicit-separator splitting, sized numeric parsing, and fallible scalar formatting |
| Dictionary `items()` | Borrowed key/value loops and comprehensions, including mutable value references; storable views and implicit snapshots are not supported |
| Allocation-free text queries | `startswith`/`endswith` return `bool`; `find`/`rfind` return optional scalar positions; `count` returns non-overlapping occurrence counts |
| Text classification | `isascii` checks ASCII membership; `isspace` requires nonempty Unicode White_Space text; neither allocates |
| Allocation-free list queries | `count`, `find`, and `rfind` observe integer/float/bool/string lists; searches return `Option[i64]` |
| Literal text affix removal | `removeprefix`/`removesuffix` remove one exact boundary match and return independent fallible strings |
| Recoverable text trimming | `strip`, `lstrip`, and `rstrip` remove Unicode whitespace at selected ends |
| Recoverable text repetition | `repeat(i64)` creates repeated UTF-8 with checked lengths and one output allocation |
| In-place collection utilities | `list.reverse()` reorders elements; list/dictionary/set `clear()` drops contents while retaining capacity |
| Recoverable bulk collection mutation | `list.extend(list)` and dictionary/set `update` consume same-typed sources and reserve before changing contents |
| Set relationships | `issubset`, `issuperset`, and `isdisjoint` observe same-typed sets without allocating |
| In-place set filtering | `intersection_update` retains common members; `difference_update` removes them; both borrow the source and reuse destination capacity |
| Fallible set algebra | `union`, `intersection`, `difference`, and `symmetric_difference` return independent sets and preserve both same-typed inputs |
| While loops, break/continue | Implemented |
| Lazy native `Generator[T]`, typed yield, consuming iteration | Implemented with allocation-free concrete frames, direct construction, specialized consumers, and moves through factories and standard sums; recursive inline layouts are rejected |
| Absolute module imports and `pub` visibility | Implemented: one source root, private-by-default declarations/members, qualified imports and aliases; cycles and re-exports deferred |
| Modern program input, file I/O, and command-line argument APIs | Recoverable console I/O, arguments, Linux whole-file helpers, and scoped File operations implemented, including bounded reads, capability queries, update/exclusive modes, saved text positions, truncation, `readlines`, and `writelines`; direct file iteration remains deferred |
| Recursive class/enum types | Self and mutual recursion through `Box` or a collection, aliases and finite generic instances, and allocation-free deep drop; recursion without a heap boundary is rejected. Automatic deep copy, equality, and formatting are gated; [reference](30-recursive-data.md) |
| Native C imports | Trusted `.plentyi` interfaces, sized scalars and nominal opaque pointers, explicit symbols, private wrappers, and static or shared linking; [C interface reference](24-c-interfaces.md) |
| Ownership-aware C adapters | Call-scoped scalar/output references, allocation-free UTF-8 views, fallible C-string conversion, and owned class wrappers with matching native destruction and explicit transfer/error policies; [adapter reference](24-c-interfaces/01-ownership-and-text-adapters.md) |
| C library exports | Numeric and Result C entries, scalar/class borrows, owned factories and consuming arguments, matching destruction, static/shared output, precise C headers, and owning `.plentyi` wrappers; [library reference](24-c-interfaces/02-library-exports.md) |
| Library contracts and tooling | Embedded interfaces, bounded compile-time extraction, SHA-256 compatibility symbols, explicit binary/interface verification, and staged artifact publication with rollback; [contract inspection](24-c-interfaces/04-embedded-contracts.md) |
| Runtime shared-library loading | CLI `--runtime-interface` and Rust APIs generate typed scalar/borrow/Result and owned-handle loaders; exact metadata checks, cached addresses, allocation-free LoadError values, resident mappings, and object-instance checks; [runtime loading](24-c-interfaces/06-runtime-loading.md) |
| User generic functions | `def f[T](...)`, argument-based inference or explicit `f[Type](...)`, cached concrete specializations, and builtin `IntType` constraints |
| Structural protocols | Plain and parameterized method contracts checked against concrete classes; exact signatures, receiver mutability, and normal module visibility, with no dynamic dispatch |
| Typed ranges and contextual numeric inference | `range[T](...)` for all integer widths; annotations guide literals and direct arithmetic range comprehensions; typed values never implicitly change width |
| `?` error propagation | Implemented for `Result` and `Option`, with matching error types or explicit erasure into `Failure`, and automatic early-exit cleanup |
| `with` context managers | Concrete owned or explicitly borrowed managers, owned/unit/reference entry results, lexical exit on fallthrough, return, `?`, break, and continue; no suspension inside the body |
| Recoverable allocation failure | Default literals/comprehensions and allocating constructors, mutation, copy, text, and formatting return `Result`; no `try_` alternatives. Explicit `?`, `match`, or `.unwrap()` handle outcomes |
| Recoverable duplication | `copy(value)` returns `Result[T, AllocError]`, preserving the source and reclaiming partial copies on failure |
| Recoverable dictionary snapshots | `keys()` and `values()` return `Result[list[T], AllocError]` in insertion order, with no implicit deep copy |
| Recoverable text operations | `str.concat(other)`, `str.join(parts)`, `str.slice(start, stop)`, and `str.replace(old, new)` return `Result[str, AllocError]`; `str.split(separator)` and `str.splitlines(keepends=False)` return `Result[list[str], AllocError]` |
| Checked text lookup | `str.get(index)` returns `Option[str]`; `text[index]`, `get`, and iteration never allocate because strings of at most seven UTF-8 bytes are stored inline |
| Custom allocators and allocator provenance | Object allocations retain internal allocator identity; container buffers still use the global allocator. Public allocator selection and allocator lifetimes are not supported |
| Scoped native threads | `with spawn(...)?`, checked shared/exclusive borrows, consuming owned jobs with recoverable SpawnError, worker effect/destructor checks, one-use result joins, automatic joining on every normal exit, and borrowed-start ThreadError; [reference](32-scoped-native-threads.md) |
| Bounded channels | Positive-capacity MPMC queues, explicit endpoint sharing, blocking/nowait transfer, recoverable construction and unsent messages, allocation-free operations, and endpoint context managers for ordered shutdown; [reference](35-bounded-channels.md) |
| Thread-pool executors | Bounded fixed worker pools, owned named/closure submission, recoverable jobs, affine futures, pending cancellation, eager ordered map over lists/ranges, and automatic drain/join; [reference](36-thread-pool-executors.md) |
| Explicit parallel operations | Ordered executor `map_result` with typed earliest-input errors and `reduce_tree` with fixed adjacent-pair grouping; bounded jobs, fallible allocation, and deterministic ownership cleanup; [reference](37-parallel-operations.md) |
| Cancellation and bounded waits | Shared cooperative cancellation tokens, monotonic timed channel operations, and non-consuming future waits; no per-operation allocation; [cancellation](38-cooperative-cancellation.md) and [timed waits](39-timed-waits-and-selection.md) |
| Channel selection | Allocation-free blocking/nowait/timed receive selection between two heterogeneous channels, inline typed result, first-receiver priority, and atomic message transfer; [reference](39-timed-waits-and-selection.md) |
| Automatic parallel loops and SIMD | Not implemented; ordinary loops and comprehensions remain serial |
| Tutorial sources | mdBook Markdown examples run directly in tests; standalone literate sources and generated lessons are not implemented |
| Async/await | Out of scope |

Collections, classes, generators, and enums containing owned values transfer ownership.
`copy(value)` explicitly duplicates mutable contents; `drop(value)` consumes an
owner early. Immutable strings may share storage; tuples and enums copy their inline fields. Collection
updates operate in place. Named local and parameter references use `&T` / `&mut T`,
with last-use loan checking over an access CFG. Class fields, list elements, and
dictionary values and enum payloads can also be borrowed; stored references are deferred. Returned
references must originate from a function's single reference parameter. Move and
borrow diagnostics point at the offending use, name the binding or field path,
and add notes for the move or the borrow's start and later use; see
[ownership and reclamation](13-ownership-and-reclamation.md).

The original four feature proposals are in [Proposals](../proposals/index.md).
They record the reasoning and suggested staging; this document describes the
implemented result, including integration choices that differ from those proposals.

The [next-phase design review](../proposals/next-language-phase.md) records earlier
entrypoint, module, FFI, protocol, memory, parallelism, SIMD, and documentation
discussions. Some of that work is implemented or has changed since the review.
Use the [backlog](../backlog.md) for remaining candidates and priorities, and the
[current language contract](05-current-language-contract.md) for usable behavior.
