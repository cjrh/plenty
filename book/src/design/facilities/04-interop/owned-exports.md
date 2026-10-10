# Owned library objects

An export returning `Result[Class, AllocError]` publishes an opaque owned handle.
Its adapter reserves a box before invoking the Plenty body. Allocation failure
skips the body, releases consumed handle arguments, and reports AllocError. A
body Err frees the empty box. Class fields and layout remain private.

For library calc and class Resource, the C declarations include:

```c
typedef struct calc_Resource calc_Resource;
uint32_t calc_create(int64_t p0, calc_Resource **out_ok, uint32_t *out_error);
void calc_Resource_destroy(calc_Resource *value);
```

Status zero returns one non-null owned handle; status one writes the error code
without changing out_ok. Destroy each successful handle exactly once with its
originating destroy function; never use free or another library instance's
function. Null destruction is a no-op. Destruction runs ordinary class and field
cleanup and invalidates all aliases. No borrow or concurrent/reentrant access may
remain; calls stay on the creating thread and code stays resident.

Generated `.plentyi` wrappers own private pointers and destroy them automatically.
Wrapper construction is inline and allocates nothing. Factory failure drops an
empty wrapper; ordinary moves, drop, scope exit, and `?` apply. Owners cannot be
copied or expose the native object's fields. Only allocation-error class factories
are supported; methods require explicit free-function exports.

Class names need distinct ASCII basenames across source modules. Handle types,
destroy functions, and generated namespaces cannot collide with user exports or
foreign imports. Names beginning `plenty_` are reserved. For example, Thing and
Thing_destroy conflict as exported classes.

## Borrowing

`&Resource` becomes a read-only call-scoped `const calc_Resource *`.
Shared arguments may alias, but no alias may mutate or destroy the live handle.
`&mut Resource` becomes an exclusive `calc_Resource *`: no other input or alias
may access it or its fields. Handles must belong to the same library instance.
Borrow adaptation allocates nothing.

Ownership remains with the caller. Whole-class replacement through a reference is
rejected, but field changes survive Err; errors do not roll back mutations.
Inputs and output storage must not overlap. Plenty wrappers preserve these loans.

## Consumption

A by-value class argument consumes its C handle on entry, including when the
call returns Err. It cannot alias another input or have outstanding borrows.
The caller cannot later access or destroy it. A supported class result explicitly
returns a new usable handle, even when the body returns the received instance.

Generated wrappers clear their private pointers immediately before native entry,
preventing double destruction. The source call consumes arguments on every path;
the factory's box-allocation failure also cleans up consumed inputs.
