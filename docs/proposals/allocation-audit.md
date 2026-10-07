# Runtime allocation audit

Initially audited after `4f63036`, updated through the collection, formatting,
tuple, and allocator-provenance foundation work. This covers generated-program allocation, not compiler
memory. Existing ordinary construction remains aborting; explicit `try_` entry
points provide incremental recovery without silently changing expression types.

| Path | Current mechanism | Next boundary |
| --- | --- | --- |
| Empty/reserved collections | `try_collection_new`, exposed by `try_new`/`try_with_capacity` | Already recoverable |
| Collection growth and explicit duplication | Checked reservations and guarded partial copies | Already recoverable through `try_` methods |
| Class storage | `Class.try_new`, optional `Result[(), AllocError]` initializer | Implemented, including partial-field cleanup |
| User enum storage | `Enum.Variant.try_new` | Implemented; ordinary variants still abort |
| Generator frame | `generator_function.try_new` | Implemented; ordinary calls still abort |
| Eager iterator collection | `list[T].try_from` and `set[T].try_from` | Implemented; iterator body operations retain their own contracts |
| Literals/comprehensions | Explicit `try [...]` / `try {...}` use checked construction and insertion | Implemented; failed growth stops iteration and cleans partial contents |
| Tuple products | Existing record storage; `try (a, b)` checks allocation | Implemented; evaluated components are consumed and cleaned on failure |
| Strings | Literals are immortal; builders have fallible alternatives | Audit implicit concatenation and formatting separately |
| Console/file APIs | Explicit I/O results include allocation errors | Keep partial-read/write contracts |
| Aggregate formatting/output | `str.try_repr` / `try_print` use checked buffers; output starts after formatting succeeds | Implemented; ordinary `print` and fatal diagnostics retain aborting allocation contracts |
| Structural equality bookkeeping | Memoization of shared aggregate pairs may allocate | Still an ordinary aborting path; scalar/list query APIs document their supported nonallocating element types |
| Destruction | Intrusive queue; no allocating work list | User destructor bodies can still allocate |
| Allocator selection | Object prefix records its process-lifetime allocator table; internal `try_allocate_in` selects it | Growth buffers still use Rust's global allocator. Public allocator selection, state/lifetimes, and per-container buffers remain future work |

## Construction contract

The class API is `Class.try_new(arguments) -> Result[Class, AllocError]`.
Arguments evaluate once, left to right, before allocation. Owned arguments move
into the call even if allocation fails; failure drops them. Allocation failure
must not run `__init__` or `__del__` for an instance that never existed. Successful
construction uses the same initialization and destruction behavior as `Class(...)`.
Allocations inside argument expressions or an ordinary `__init__` remain subject
to their own contracts; this API does not catch aborts or arbitrary failures.

Initialization can return `Result[(), AllocError]`. Initialized fields are dropped
on failure; whole-instance `__del__` is activated only after success. Other
initializer error types remain future work. Generator allocation failure releases
captured arguments without executing the body.

## Validation obligations

For each exposed boundary, inject failure at every allocation, check cleanup and
argument evaluation order, and retry afterward. Check successful construction
with owned/nested fields and custom destructors. Exercise runtime pointer/layout
changes under Miri and generated native code under allocation accounting.

Remaining aborting paths must stay documented: a recoverable constructor does
not establish a process-wide out-of-memory guarantee.

Native failure sweeps cover each reached collection/format-buffer allocation,
including final string storage, stopped comprehension loops, owned prefixes, and
tuple component cleanup. A failed `try_print` format writes no prefix; a later
I/O failure may still have written bytes. Runtime allocation accounting checks
reclamation, and Miri covers the allocation prefix and element-pointer stability.
The test-only failure budget is restored before destructor output in the tuple
cleanup test: user destructor bodies retain their own allocation contracts.
