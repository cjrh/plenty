# Trusted declarations and ABI types

Imports resolve `name.plenty` or `name.plentyi`; having both is ambiguous.
`.plentyi` permits foreign declarations alongside ordinary Plenty types and
wrappers:

```text
pub opaque Image
extern def load_raw(code: i32) -> Image = "image_load"
extern def free_raw(image: Image) -> () = "image_free"
pub extern def version() -> u32 = "image_version"
```

`extern def` requires fully typed parameters, a return type, and an explicit
C symbol or function-address parameter. Bodies, generics, varargs, defaults,
and implicit header translation are unsupported. `pub` only controls source
imports. Checking does not invoke native functions or inspect C headers.

The raw ABI supports `i8`–`i64`, `u8`–`u64`, `f32`, `f64`, nominal opaque
pointers, and unit for void results, plus the
[explicit borrowing/text adapters](borrowing-text.md). On x86-64 and AArch64 GNU/Linux,
these match C `intN_t`, `uintN_t`, `float`, `double`, and pointers. Binding aliases
can name C types: `int` is `i32`, `long` and `ptrdiff_t` are `i64`, and `size_t`
is `u64`. Bool, aggregate values, and private Plenty layouts have no raw ABI.

`opaque Name` declares a nominal pointer without a pointee layout. Distinct names
do not interchange. Values can be copied, compared for address equality, passed
back to C, or tested with `.is_null()`; `Name.null()` constructs null. They cannot
be dereferenced, converted to integers, or used in pointer arithmetic. They do
not own or free their pointees. Public signatures cannot expose private opaque
types. Prefer private pointers behind [owning wrappers](foreign-owners.md).

C symbols must be identifiers outside the `main`, `plenty_`, and `__plenty_`
namespaces, except the checked [loading primitives](loading-primitives.md).
Conflicting declarations are rejected. The binding author must supply the exact
ABI, lifetimes, permissions, and error/ownership rules. Callbacks and C records
are unsupported.

## Calling a native address

```text
opaque Function
extern def invoke(function: Function, value: i32) -> i32 = function
```

The identifier after `=` must name an opaque pointer parameter. It selects the
callee and is omitted from C arguments; remaining arguments use ordinary adapters.
The binding must provide a non-null address of the exact declared signature and
keep the originating code resident. Arbitrary addresses are not validated.
Keep this primitive behind a checked private wrapper; it is not a source-language
callable value.

## Worker threads

A worker body, helper, callback or destructor that can reach a foreign call is
rejected unless the declaration carries `@thread_safe`:

```text
@thread_safe
pub extern def pid() -> i32 = "getpid"
@thread_safe
extern def close_raw(fd: i32) -> i32 = "close"
```

The compiler cannot inspect native code, so the annotation is the binding
author's promise, trusted like the rest of the declaration. Annotate a
declaration only if the C function:

- may run on any thread, concurrently with any other call into its library,
  and needs no setup on a particular thread;
- changes no process-wide state other threads depend on, such as the working
  directory, signal dispositions, `umask`, locale or environment, which the
  [native effect table](../03-concurrency/effects.md) rejects for `std` too;
- retains no borrowed argument after it returns.

The rules have no exceptions:

- An unannotated declaration stays rejected wherever the worker reaches it:
  directly, through imported or `std` helpers, generic instances,
  [thin callable targets](../03-concurrency/effects.md#thin-callable-targets)
  or destructors. The diagnostic names the C symbol and the call path.
- Every declaration of one C symbol in a program agrees on `@thread_safe`.
- A [function-address declaration](#calling-a-native-address) cannot be
  annotated: its callee is chosen at run time.
- The annotation certifies calls, not values. A type containing an `opaque`
  pointer still cannot enter a worker, channel or executor job, because
  opaque values copy and the compiler cannot bound which threads use the
  pointee.

A resource that crosses threads is therefore a scalar, such as a file
descriptor, inside an owner class that is not `@copy`. The owner moves like
any affine value, and its destructor, which calls an annotated release
function, runs on the thread that drops it: the worker, the parent after a
failed thread start, or the last [`Shared`](../01-builtins/shared.md) holder.
The binding author still decides whether shared (`&Owner`) methods are safe
to call from several workers at once.
