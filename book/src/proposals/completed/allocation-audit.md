# Runtime allocation audit

> **Status: Implemented.** Reconciled with the compiler on 2026-10-10.
> [Allocation and traps](../../design/language/08-errors/02-allocation-and-traps.md)
> owns the contract; this page is the per-path audit behind it.
>
> **Remaining:** public allocator selection, allocator lifetimes, and routing of
> container buffers (B13).

Allocation is fallible by default in the current language. This audit covers
generated programs, not compiler memory. Public allocating operations return
`Result`; there are no `try_` alternatives or prefix `try` displays. Postfix `?`
propagates failure, `match` handles it, and `.unwrap()` explicitly terminates on
failure. Nonallocating operations keep their ordinary return types.

| Path | Current contract |
| --- | --- |
| Collections and reservation | `list[T]()`, `new`, `with_capacity`, and `reserve` return checked results |
| Growth and duplication | `append`, `add`, `insert`, `extend`, `update`, and `copy` use checked reservations and partial-owner guards |
| Class storage | Fields are stored inline; `Class(...)` and `Class.new(...)` allocate nothing and return `Class`. A fallible `__init__` makes both return `Result[Class, AllocError]` |
| Enum storage | Payloads are stored inline; variant constructors allocate nothing, for user enums and builtin sums alike |
| Boxes | `Box(value)` returns `Result[Box[T], AllocError]`; it is the heap boundary for recursive types |
| Generator frames | A call to a yielding function returns its concrete inline frame directly and allocates nothing |
| Ranges | `range(...)` returns a copyable inline value and allocates nothing |
| Iterator collection | List/set construction and `from` check output growth; iterator bodies handle their own operations |
| Literals/comprehensions | Construction returns `Result`; failed growth stops evaluation and cleans partial contents |
| Tuples | Tuples are stored inline; a tuple display allocates nothing |
| Strings | Literals are immortal and strings of at most seven bytes are stored inline. `+` and builders return results. Indexing and iteration yield `str` and allocate nothing |
| Console/files | I/O results include allocation errors; partial reads/writes retain their documented effects |
| Formatting | `str.repr` and `print` use checked buffers. Printing an existing string needs no formatting allocation |
| Equality | Structural equality allocates nothing. It is rejected at compile time for recursive data (B33) |
| Indexed replacement | List/dictionary replacement allocates nothing. Missing dictionary keys trap; adding keys uses `insert` |
| Destruction | An intrusive queue needs no allocated work list. User destructor bodies handle their own operation results |
| Allocator provenance | Object prefixes retain their process-lifetime allocator table; container buffers still use the global allocator |

Class arguments evaluate left to right and move into the call. Storage cannot
fail, because the instance is inline. Initializer failure drops initialized
fields; the instance's custom destructor becomes active only after successful
initialization.

Collection display owners are allocated before entries or iterables are evaluated.
The first failed growth skips later expressions and releases the initialized
prefix. A `?` inside an entry or filter returns from the enclosing function; the
outer display is not an exception handler. Earlier side effects are not rolled back.

`Result` and `Option` wrappers, builtin error payloads, propagation, and native
entrypoint status conversion need no allocation. A `main` that returns `Err`
[reports it](../../design/toolchain/01-building/entry-report.md) on standard error
and exits with status one. If formatting the report fails to allocate, it falls
back to a fixed allocation-free message.

Failure sweeps exercise construction, growth, duplication, formatting buffers,
final string storage, and stopped comprehensions. Native
allocation accounting checks reclamation; Miri checks runtime layouts and pointer
access. Destructors used in allocation-failure tests either avoid allocation or
restore the test budget before explicitly allocating output.

Explicit unwrapping, invalid operations such as out-of-bounds indexing, and fatal
runtime invariants can still terminate without unwinding.
Public allocator selection, lifetimes, and routing of container buffers remain
future work.
