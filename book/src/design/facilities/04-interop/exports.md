# Exports and generated C contracts

`export def` gives a top-level function an explicit C entry point in a library
build. It remains an ordinary Plenty callable; `pub` independently controls
imports. In application builds, export functions retain private native symbols.

```text
export def add(a: i32, b: i32) -> i32 = "calc_add":
    "Add two checked integers."
    a + b
```

Parameters can be numeric scalars, owned exported classes, or shared/exclusive
references to either. Returns can be numeric, unit, or `Result[T, E]`, where T is
numeric/unit and E is numeric, `AllocError`, `ParseError`, `RangeError`, `Failure`,
or a nongeneric user enum whose variants have no payloads.
`Result[Class, AllocError]` factories use [owned handles](owned-exports.md).
Bool, other managed objects, raw pointers, returned references, generic exports,
and other aggregate results are unsupported; bodies can use ordinary features.
Enums with payload variants and error sums such as `AllocError | ParseError`
are rejected; sums are planned with the broader C representations (B27).

C symbols begin with the library name plus `_`. `NAME_plenty_` and `_plenty_`
source names are reserved for generated metadata and compatibility adapters.
Duplicate symbols, import/export collisions, and generated-name collisions fail
checking or artifact validation. The same namespace applies to imported source
modules' exports. Library builds require at least one export but no main.

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

Headers require serialized calls on one caller thread and prohibit unwinding or
nonlocal jumps across Plenty frames. Traps and explicit unwrap failures can
terminate the process; adapters invent no recovery mechanism. Keep the generated
header, interface, and binary together; see [contracts](contracts.md) and
[library artifacts](../../toolchain/01-building/artifacts.md).
