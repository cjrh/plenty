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
ABI, lifetimes, permissions, and error/ownership rules. Typed callback values and C record layouts remain unsupported.
[Trusted raw host interfaces](raw-host-interfaces.md) provide exported C callback
addresses, raw scalar memory access and resident storage for explicit bindings.

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
  annotated `@thread_safe`: its callee is chosen at run time. The separate
  [`@host_callback` contract](host-threads.md#host-protocol-callbacks) applies
  only to foreign host entry contexts, never ordinary Plenty workers.
- The annotation certifies calls, not values. Which native values worker
  code may hold is the [opaque type's thread contract](#opaque-types-on-worker-threads).

A scalar resource, such as a file descriptor, crosses inside an owner class
that is not `@copy`. The owner moves like any affine value, and its destructor,
which calls an annotated release function, runs on the thread that drops it:
the worker, the parent after a failed thread start, or the last
[`Shared`](../01-builtins/shared.md) holder.

## Opaque types on worker threads

An `opaque` declaration states which threads may use its pointees, as
`@thread_movable` or `@thread_shareable` on the line above it:

```text
@thread_shareable
opaque RawFrame
@thread_safe
extern def frame_height(frame: RawFrame) -> i64 = "frame_height"
@thread_safe
extern def frame_drop(frame: RawFrame) -> () = "frame_drop"
```

Worker code may hold an opaque value only if its type has one of these
contracts. Opaque values copy, and a copy that reaches another thread is a
share, so the contracts differ in how a value may cross:

| Contract | Like Rust | Worker code may hold it | It may cross threads |
|---|---|---|---|
| none | | never | never |
| `@thread_movable` | `Send` | yes | only as a field of a moved class value that `copy(x)` rejects |
| `@thread_shareable` | `Send + Sync` | yes | in any form: copied, moved, borrowed, in `Shared` or a channel |

The rules have no exceptions:

- A `@thread_movable` type is rejected in the form a value crosses in when
  that form is a copy, a borrow, a `Shared` value, a bare message, or a class
  value that `copy(x)` accepts: any `@copy` class, and any class with no
  destructor anywhere in it. A [`Mutex`](../03-concurrency/mutex.md) guarding
  the owner restores the moved form, as Rust's `Mutex<T>` needs only `T: Send`.
- Items of a [parallel pipeline](../../language/09-iteration/06-pipelines.md#parallel-pipelines) cross as
  `pool.map` inputs do: an owned source list moves them to the chunk jobs and
  a borrowed one lends them.
- `@thread_shareable` already includes `@thread_movable`; writing both is an error.
- Every `extern def` with a C symbol that takes or returns a marked type,
  directly, through a borrow or as the success value of a `c_string` or `c_strings`
  adapter's `Result`, must be `@thread_safe`. Worker code can call anything
  that accepts a value it holds, including the release function an owner's
  `__del__` calls. A function-address declaration (`= function`) cannot be
  `@thread_safe` and needs no annotation: worker code may never make that
  call.
- An unmarked type keeps its unrestricted single-thread bindings.

Destructors are checked as for scalar owners: a worker that moves, receives
or shares an owner reaches its `__del__`.

### When a binding may mark a type

Mark a type `@thread_movable` only if every pointee:

- may be used and released on a thread other than the one that created it;
- has no thread affinity, such as a GUI handle, a thread-local allocator or a
  library context bound to its creating thread;
- shares no unsynchronized mutable state with other pointees.

Then keep the pointer in a private field of one owner class that holds the
only copy and releases it in `__del__`. The compiler checks only the type a
value crosses in: that the pointer is inside a moved class value that
`copy(x)` rejects. It does not check that the field is private, that this
class's own `__del__` releases the pointer, or that no other value holds a
copy of it. Plenty code that copies the pointer into a second owner breaks the
promise without a diagnostic, so keep the `opaque` declaration and the code
that builds owners private to the binding module.

Mark a type `@thread_shareable` only if, in addition, any number of threads
may call every declared function on the same pointee at once. The rule for
mutation has no exceptions:

> A `@thread_shareable` type is read-only through every declared call;
> mutation happens only on `@thread_movable` types, which Plenty never calls
> concurrently on one pointee.

The release function frees the pointee rather than mutating it: its one owner
calls it once, after every loan has ended. A value built up
in steps needs a separate `@thread_movable` builder type and a `@thread_safe`
call that consumes the builder and returns the shareable type. Rust engines
can assert the promises at compile time, with `T: Send + Sync` on each
shareable type and `T: Send` on each builder.

The compiler checks the annotations and their consistency, not this rule. An
opaque argument is a bare pointer, so a declaration does not say whether the C
function writes through it; `&mut` of an opaque type is an output slot that
receives a new pointer, not a write to an existing pointee.

For a function taking a `@thread_movable` type, `@thread_safe` covers
concurrent calls on distinct pointees only.

[How std.dataframe wraps its engine](dataframe-binding.md) applies these
guidelines to a Rust engine: builders, owner classes, compile-time assertions
and a per-call audit.

The [dataframe](../../stdlib/dataframe.md#worker-threads) and
[linear algebra](../../stdlib/linalg.md#worker-threads) bindings follow these
guidelines.
