# Owned library objects

An export returning `Result[SomeClass, AllocError]` publishes an opaque owned
handle: a box holding the instance. Instances are otherwise stored inline, so the
export adapter allocates the box before calling the Plenty function. If that
allocation fails, the function does not run, consumed handle arguments are
released, and the status reports `AllocError`. If the function returns `Err`,
the empty box is freed. The class fields and native layout remain private. Class names must have
distinct ASCII basenames within the library, including across imported modules.

For library `calc` and class `Resource`, the C interface declares:

```c
typedef struct calc_Resource calc_Resource;
uint32_t calc_create(int64_t p0, calc_Resource **out_ok, uint32_t *out_error);
void calc_Resource_destroy(calc_Resource *value);
```

Status 0 initializes `out_ok` with one non-null owned handle. Status 1 initializes
the allocation-error code and leaves `out_ok` unchanged. Release each successful
handle exactly once through `calc_Resource_destroy`; never call `free` or a destroy
function from another library instance. Destroy accepts null as a no-op.

Destroy runs the class destructor and field cleanup. All aliases become invalid;
there must be no outstanding borrows or concurrent/reentrant access. Calls stay
on the creating thread, and the originating library remains loaded. Headers place
these requirements beside the factory and destroy declarations. Generated destroy
symbols and handle type names are reserved against user export collisions.
The compiler checks the complete C identifier namespace: for example, exporting
both `Thing` and `Thing_destroy` as class names is rejected because one class's
type name would collide with the other's release function. Generated symbols
cannot collide with foreign imports either. Class names beginning `plenty_` are
reserved at this boundary.

The generated `.plentyi` publishes an owning class with a private opaque pointer
and an automatic destructor. Its factory creates an empty wrapper, which needs no
allocation, then calls the native factory and stores the handle on success. A
native failure drops the empty wrapper. Scope exit, `drop`, moves, and `?` use
ordinary Plenty ownership rules. The wrapper never exposes the native object's
fields or permits copying the owner.

Only allocation-error factories are supported for class results, because
allocating the handle can fail. A consumed handle is moved out of its box, so a
function returning an owner it received returns a new handle. Automatically exported
methods are outside this subset; export explicit free-function adapters instead.

## Borrowing handles

An `&Resource` parameter becomes `const calc_Resource *`: a read-only, call-scoped
borrow of a live handle from the same library instance. Shared arguments may
alias, but no alias may mutate or destroy the object during the call.

An `&mut Resource` parameter becomes `calc_Resource *`, borrowed exclusively.
No other argument or alias may access the owner or its fields during the call.
Ownership remains with the caller. Plenty rejects whole-class replacement through
a reference, so the handle remains valid, but fields can change even when a call
returns `Err`. An error does not imply rollback. Input and result output storage
must not overlap.

Generated Plenty wrappers preserve these reference signatures, so callers get
normal loan checking. Borrow adaptation itself does not allocate. Exported free
functions can call the class's ordinary methods internally.

## Consuming handles

A class parameter passed by value becomes an owned C handle argument. The C call
consumes it on entry, even when the result is `Err`. It cannot alias another input
or have outstanding borrows. The caller must neither access nor destroy the old
handle afterward. The callee may return ownership explicitly through a supported
class result; only that returned owner is then usable.

Generated Plenty wrappers take their class arguments by value. They clear their
private pointer immediately before the native call so automatic cleanup cannot
destroy a transferred owner again. If an output wrapper allocation fails before
the native call, the input owners are still cleaned up normally. The source call
consumes its arguments on every path, and borrow checking rejects later reuse.
