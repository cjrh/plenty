# Runtime allocation audit

Allocation is fallible by default in the current language. This audit covers
generated programs, not compiler memory. Public allocating operations return
`Result`; there are no `try_` alternatives or prefix `try` displays. Postfix `?`
propagates failure, `match` handles it, and `.unwrap()` explicitly terminates on
failure. Nonallocating operations keep their ordinary return types.

| Path | Current contract |
| --- | --- |
| Collections and reservation | `list[T]()`, `new`, `with_capacity`, and `reserve` return checked results |
| Growth and duplication | `append`, `add`, `insert`, `extend`, `update`, and `copy` use checked reservations and partial-owner guards |
| Class storage | `Class(...)` and `Class.new(...)` return `Result[Class, AllocError]`; an initializer may also return an allocation result |
| User enum storage | Heap variant constructors return `Result[Enum, AllocError]`; builtin inline sums and errors remain allocation-free |
| Generator frames | A call to a yielding function returns `Result[Generator[T], AllocError]`; failure releases captures without executing the body |
| Ranges | `range(...)` currently has a heap owner and returns `Result[range[T], AllocError]` |
| Iterator collection | List/set construction and `from` check output growth; iterator bodies handle their own operations |
| Literals/comprehensions | Construction returns `Result`; failed growth stops evaluation and cleans partial contents |
| Tuples | Nonempty tuple displays return a checked allocation result; evaluated components are consumed and cleaned on failure |
| Strings | Literals are immortal. `+`, indexing, builders, and snapshots return results. Iteration yields one `Result[str, AllocError]` per scalar |
| Console/files | I/O results include allocation errors; partial reads/writes retain their documented effects |
| Formatting | `str.repr` and `print` use checked buffers. Printing an existing string needs no formatting allocation |
| Equality | A fixed 256-entry stack memo avoids heap allocation; eviction can repeat work on larger shared graphs |
| Indexed replacement | List/dictionary replacement allocates nothing. Missing dictionary keys trap; adding keys uses `insert` |
| Destruction | An intrusive queue needs no allocated work list. User destructor bodies handle their own operation results |
| Allocator provenance | Object prefixes retain their process-lifetime allocator table; container buffers still use the global allocator |

Class arguments evaluate left to right before allocation and move into the call
on either outcome. Storage failure runs neither initializer nor destructor for
the nonexistent instance. Initializer failure drops initialized fields; the
instance's custom destructor becomes active only after successful initialization.

Collection display owners are allocated before entries or iterables are evaluated.
The first failed growth skips later expressions and releases the initialized
prefix. A `?` inside an entry or filter returns from the enclosing function; the
outer display is not an exception handler. Earlier side effects are not rolled back.

`Result` and `Option` wrappers, builtin error payloads, propagation, and native
entrypoint status conversion need no allocation. A Result-returning `main` drops
its error and exits with status one; it does not allocate an error diagnostic.

Failure sweeps exercise construction, growth, duplication, formatting buffers,
final string storage, stopped comprehensions, and owned tuple components. Native
allocation accounting checks reclamation; Miri checks runtime layouts and pointer
access. Destructors used in allocation-failure tests either avoid allocation or
restore the test budget before explicitly allocating output.

Explicit unwrapping, invalid operations such as out-of-bounds indexing, and fatal
runtime invariants can still terminate without unwinding. Legacy backend test
entrypoints retain their historical contracts and are not modern language APIs.
Public allocator selection, lifetimes, and routing of container buffers remain
future work.
