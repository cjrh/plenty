# C interfaces

An import resolves either `name.plenty` or `name.plentyi`. Having both is an
ambiguity error. `.plentyi` explicitly marks a trusted binding module: it permits
C declarations as well as ordinary Plenty types and wrapper functions.

```text
pub opaque Image
extern def load_raw(code: i32) -> Image = "image_load"
extern def free_raw(image: Image) -> () = "image_free"
pub extern def version() -> u32 = "image_version"
```

`extern def` requires fully typed parameters, a result type, and an explicit C
symbol string. No body, generics, varargs, default arguments, or implicit header
translation are supported. `pub` controls Plenty imports only. A wrapper using
Plenty's private calling convention makes an ordinary target-C-ABI call; C calls
are never emitted using Plenty's tail calling convention.

The initial ABI supports `i8`–`i64`, `u8`–`u64`, `f32`, `f64`, opaque pointers,
and `()` for a void result. Call-scoped scalar/pointer references and explicit
text adapters are described in [ownership and text adapters](24-c-interfaces/01-ownership-and-text-adapters.md).
On the supported x86_64 Linux GNU target these match
the corresponding C `intN_t`/`uintN_t`, `float`, `double`, and pointers. Use
explicit aliases in the binding for C names; C `int` is `i32`, `long` and
`ptrdiff_t` are `i64`, and `size_t` is `u64` on this target. `bool`, aggregate
values, and private Plenty object layouts have no raw C ABI in this subset.

`opaque Name` declares a nominal raw pointer type, with no size or pointee layout.
Different opaque names do not interchange. Values can be copied, compared for
address equality, passed back to C, and checked with `.is_null()`; `Name.null()`
constructs its null value. They cannot be dereferenced, cast to/from integers, or
used for pointer arithmetic. They do **not** own or free their pointees. Keep raw
pointers and their operations private behind ownership-aware wrappers where
possible. Public signatures cannot expose private opaque types.

C symbols must be identifiers outside `main`, `plenty_`, and `__plenty_`
namespaces. Conflicting declarations of one C symbol are rejected during
checking. The compiler does not inspect C headers or prove that the actual
library matches a declaration. The binding author must provide the exact ABI,
valid pointer lifetimes, alias permissions, and error/ownership rules; no foreign
unwinding or nonlocal jumps may cross Plenty frames. Source checking does not
invoke native functions. A mistaken trusted interface can violate memory safety.

Linking is explicit, independent of source imports:

```sh
plenty --compile app.plenty -o app --link-arg /path/to/libimages.a
plenty --compile app.plenty -o app --link-arg -L/path/to/libs --link-arg -limages
```

Archives and ordinary linked shared libraries are supported through the native
driver. Use driver-specific options such as `-Wl,-rpath,/path/to/libs` explicitly
when the system loader needs a search path. This is link-time binding, not
runtime `dlopen`. Library inputs must match the supported target. Missing symbols
produce linker diagnostics. Object-only output leaves resolution to the external
build system; interfaces do not add libraries or loader side effects implicitly.

Native C fixtures exercise scalar widths, floating point, register and stack
arguments, nominal pointers, null handling, static/shared linking, and signature
diagnostics. Ordinary `pub` functions remain private native symbols. C exports,
callbacks, C records, and runtime loading are not supported yet.
The [export contract proposal](../proposals/ffi-export-contracts.md) records the
accepted direction for generated ownership metadata and C headers that document
the same guarantees and caller requirements.
