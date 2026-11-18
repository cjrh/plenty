# Exports and generated C contracts

`export def` gives a top-level function an explicit C entry point in a library
build or an explicit [executable callback build](#executable-callbacks). It
remains an ordinary Plenty callable; `pub` independently controls imports.
Ordinary application builds keep export functions private.

```text
export def add(a: i32, b: i32) -> i32 = "calc_add":
    "Add two checked integers."
    a + b
```

Parameters can be numeric scalars, `bool`, opaque pointers, owned exported classes, or
shared/exclusive references to numeric scalars, `bool` or exported classes. Returns can
be numeric, `bool`, opaque pointers, unit, or `Result[T, E]`, where T is
numeric/`bool`/unit and E is numeric, `AllocError`, `ParseError`, `RangeError`, `Failure`,
or a nongeneric user enum whose variants have no payloads.
`Result[Class, AllocError]` factories use [owned handles](owned-exports.md).
Other managed objects, returned references, generic exports,
and other aggregate results are unsupported; bodies can use ordinary features.
Enums with payload variants and error sums such as `AllocError | ParseError`
are rejected; sums are planned with the broader C representations (B27).

An integer domain type such as `NonZero[T]` or `Bounded[T, lo, hi]` cannot
appear in an export signature, as a parameter or a result: C callers are
unchecked. Take the underlying integer and convert it with `try_from`; see
[domain types and foreign code](../../language/03-types/numbers.md#domain-types-and-foreign-code).

C symbols normally begin with the library name plus `_`.
[`= exact "symbol"`](raw-host-interfaces.md#export-names-and-pointers) opts into a
protocol-required name. `NAME_plenty_` and `_plenty_`
source names are reserved for generated metadata and compatibility adapters.
Duplicate symbols, import/export collisions, and generated-name collisions fail
checking or artifact validation. The same namespace applies to imported source
modules' exports. Library builds require at least one export but no main.

Generated headers include `<stdbool.h>` and spell boolean parameters and
results `bool` for both C and C++. `Result[bool, E]` writes one byte through
`bool *out_ok`; boolean error codes remain unsupported. Generated static and
runtime-loaded Plenty wrappers preserve the boolean success type.

## Scalar borrows

`&i32` becomes `const int32_t *`; `&mut i32` becomes `int32_t *`.
Pointers must be non-null, aligned, initialized, and live for the call. Shared
arguments exclude mutation through every alias; exclusive arguments cannot overlap
another loan or any concurrent/reentrant access. The callee retains no storage.

Adapters read/write exactly the C type's size, allocate nothing, and copy mutable
values back on every normal return. A trap does not return and promises neither
copy-back nor rollback. Generated Plenty wrappers preserve the reference types
so ordinary loan checking applies.

## Result outputs

Result exports return `uint32_t` status: 0 for Ok, 1 for Err. After input arguments
come `T *out_ok` (omitted for unit) and `E *out_error`. Both must be non-null,
aligned, writable, mutually disjoint, and disjoint from input loans; previous
contents need not be initialized. Only the selected output is written.
Err does not undo input mutations. Adaptation and Result reconstruction allocate
nothing and do not expose private sum layouts.

Enum errors use `uint32_t *out_error`. A variant's code is its declaration
index. The builtin errors have these codes:

| Error | Codes |
| --- | --- |
| AllocError | OutOfMemory = 0; CapacityOverflow = 1 |
| ParseError | Invalid = 0; OutOfRange = 1 |
| RangeError | OutOfRange = 0 |
| Failure | Unspecified = 0 |

Prefer a precise enum; `Failure` is a last resort when no precise error exists.
For a user enum, the generated interface re-declares the enum, so it stays
self-contained, and its wrappers return that interface type:

```text
enum CalcError:
    Empty
    Negative
export def check(value: i64) -> Result[f64, CalcError] = "calc_check":
    ...
```

The header names each code once per enum as `LIBRARY_ENUM_VARIANT`, before the
functions that use it:

```c
/* CalcError codes, written to *out_error when a call returns 1. */
#define calc_CalcError_Empty UINT32_C(0)
#define calc_CalcError_Negative UINT32_C(1)

/* On Err, *out_error is one of the calc_CalcError_* codes. */
uint32_t calc_check(int64_t p0, double *out_ok, uint32_t *out_error);
```

Interfaces use the enum's unqualified name, so two exported enums with the same
name, an enum named like an export or exported class, or a constant that
collides with another generated C identifier fails the build. Runtime loaders
also reserve `Library` and `load`.

Status and error code are distinct: code zero still means failure when status is
one. Generated headers list these codes and caller requirements. Author docstrings
appear as separate escaped documentation.

Unannotated exports require serialized calls on one caller thread;
[`@thread_safe` exports](host-threads.md) support certified host-thread entry.
Headers prohibit unwinding or
nonlocal jumps across Plenty frames. Traps and explicit unwrap failures can
terminate the process; adapters invent no recovery mechanism. Keep the generated
header, interface, and binary together; see [contracts](contracts.md) and
[library artifacts](../../toolchain/01-building/artifacts.md).

## Executable callbacks

`--executable-exports NAME` explicitly enables the same C adapters in an
application object (`--emit-object`) or executable (`--compile`). The application
still requires ordinary Plenty main; native support calls its adapters only
after normal application startup. A generated `NAME.h` accompanies the output,
with the same checked scalar/borrow/Result/owned-handle contracts and originating
destructors as a library. `address "symbol"` in a trusted interface refers to
that C adapter, never the private Plenty body. No retained closures or implicit
callback registry are introduced.

```sh
plenty --emit-object app.plenty --executable-exports app -o build/app.o
plenty --compile app.plenty --executable-exports app -o build/app
```

Ordinary exports must use the `NAME_` prefix; `exact` keeps its explicit opt-in
meaning. `main` and `plenty_` symbols are reserved for application/runtime entry.
Adapters and destructor addresses remain image-local on ELF (protected visibility).
Native code can link directly or receive explicitly taken addresses. This does
not enable dynamic executable symbol discovery (`-rdynamic`), arbitrary retained
callbacks, thread transfer or reentrancy. All pointer/loan/serialization obligations
in the generated header still apply. Returned owners must be destroyed exactly
once after callback cessation. Reachability follows direct references and address
relocations; unused adapters and their private checked bodies can be removed by
native section GC. This explicit mode uses the existing runtime-helper checks
throughout the application instead of registering a startup signal-trap table:
otherwise that table would retain unused callback and destructor bodies. Live
checks retain their failure semantics, source diagnostics and actor containment.
Stack probing stays enabled. On unqualified AArch64 callback builds, specialized
below-SP loop-probe-PC overflow diagnostics are not registered; stack-page
protection remains, but that specialized diagnostic is not promised.

This initial mode is qualified on Linux GNU x86-64, requires the baseline CPU
level and excludes test hooks,
fuzzing, distribution output and CLI package-graph builds. Invalid modes/names
or output/header collisions are errors. Object/header and executable/header sets
use the [library publication guarantees](../../toolchain/01-building/artifacts.md#publication-guarantees).
Whole-program caching is bypassed in this explicit mode so header and adapters
come from one checked frontend result; compiled-function caching remains active.
An owned handle and an exclusive borrow can have **identical C prototypes**.
Native callers that depend on one ownership convention should pin the full
`NAME_plenty_contract_v1_HASH` from a reviewed, compatible interface. Enforce
this independently of the newly generated header: require the pinned symbol at
[final link](../../toolchain/01-building/linking.md#external-build-systems),
or declare and call it on a live path before creating an owner or invoking any
callbacks. Merely including today’s header, or extracting today’s hash during
each build, does not check the caller’s expectation. Adding even an unrelated
export changes this [whole-interface guard](contracts.md#compatibility-fingerprints);
review intentional changes before updating the pin.
