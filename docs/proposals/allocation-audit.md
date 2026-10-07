# Runtime allocation audit

Audited after `4f63036`. This covers generated-program allocation, not compiler
memory. Existing ordinary construction remains aborting; explicit `try_` entry
points provide incremental recovery without silently changing expression types.

| Path | Current mechanism | Next boundary |
| --- | --- | --- |
| Empty/reserved collections | `try_collection_new`, exposed by `try_new`/`try_with_capacity` | Already recoverable |
| Collection growth and explicit duplication | Checked reservations and guarded partial copies | Already recoverable through `try_` methods |
| Class storage | `try_record_new` exists; ordinary construction unwraps it | Expose `Class.try_new(arguments)` |
| User enum storage | Same record allocator; ordinary variants unwrap it | Explicit fallible variant construction |
| Generator frame | `memory::allocate` aborts | Checked frame allocation, then a source-level boundary |
| Literals/comprehensions | Ordinary collection construction and insertion | Explicit fallible construction context; preserve evaluation order |
| Strings | Literals are immortal; builders have fallible alternatives | Audit implicit concatenation and formatting separately |
| Console/file APIs | Explicit I/O results include allocation errors | Keep partial-read/write contracts |
| `print` and runtime diagnostics | Convenience formatting can allocate | Retain aborting contract until fallible formatting exists |
| Destruction | Intrusive queue; no allocating work list | User destructor bodies can still allocate |
| Allocator selection | Fixed Rust global allocator | Provenance and allocator lifetime design required |

## Construction contract

The first class API will be `Class.try_new(arguments) -> Result[Class, AllocError]`.
Arguments evaluate once, left to right, before allocation. Owned arguments move
into the call even if allocation fails; failure drops them. Allocation failure
must not run `__init__` or `__del__` for an instance that never existed. Successful
construction uses the same initialization and destruction behavior as `Class(...)`.
Allocations inside argument expressions or an ordinary `__init__` remain subject
to their own contracts; this API does not catch aborts or arbitrary failures.

Allowing initialization itself to return an error needs a separate contract for
partial fields and custom destruction. Never invoke a whole-instance `__del__`
on an incompletely initialized value. Similarly, a generator allocation error
must release captured arguments without executing its body.

## Validation obligations

For each exposed boundary, inject failure at every allocation, check cleanup and
argument evaluation order, and retry afterward. Check successful construction
with owned/nested fields and custom destructors. Exercise runtime pointer/layout
changes under Miri and generated native code under allocation accounting.

Remaining aborting paths must stay documented: a recoverable constructor does
not establish a process-wide out-of-memory guarantee.
