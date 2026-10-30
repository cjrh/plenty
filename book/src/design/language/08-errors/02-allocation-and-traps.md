# Allocation and traps

Source-level heap construction and allocating operations use checked paths.
`AllocError` has two allocation-free nullary variants:
`AllocError.OutOfMemory` for allocator rejection and
`AllocError.CapacityOverflow` for impossible sizes or layouts. Constructing,
copying, comparing, matching, or propagating it inside Result allocates nothing.
`AllocError` itself is not a family on which `?` operates.

Class instances, tuples, and enum values are inline and construct directly
without allocation. Allocations in their component expressions keep their
own Results. A fallible class initializer can make construction return
`Result[Class, AllocError]`; [class initialization](../03-types/classes.md)
defines cleanup of a partially initialized instance. String literals, scalar
values, and ranges require no heap allocation.

Collections, boxes, and newly allocated dynamic text expose allocation errors.
Collection displays, comprehensions, constructors, growing updates, and
`copy(value)` return Result. Their APIs have no separate `try_` naming family.
Successful reservation permits subsequent insertions within that capacity
without storage allocation. Failed growth preserves logical contents and order,
although capacity may have increased. Failed deep copy preserves the source
and destroys partial output. Exact APIs and input-transfer behavior are in
[collections](../../facilities/01-builtins/collections.md) and
[text](../../facilities/01-builtins/text.md).

Failure does not generally roll back argument effects, consumed generator
steps, external input, or partial writes. Operations specify what is preserved
and what has already been consumed. Allocation-free wrapper transport,
borrowing, inline environments, generator frames, and cleanup bookkeeping do
not make allocations performed by user expressions or destructors disappear.
Cleanup on an allocation-error path must still handle its own fallible work.
No public allocator switching or custom-memory-manager API is exposed.

`value.unwrap()` extracts a success payload without allocation, moving it
when owned. `Err` or `Nothing` deliberately traps. Invalid indexing, absent
dictionary keys in indexed access, integer overflow, division by zero, a
numeric cast to an integer type that cannot represent the value, invalid range
parameters, and an empty `split` separator are also fatal runtime errors;
optional lookups provide recoverable absence where supported, and integer
arithmetic written inside a [`checked:` block](../03-types/numbers.md#checked-blocks)
returns overflow and division by zero as `Err`. Traps terminate
the process without unwinding language scopes. They are not caught by Result
propagation or resource scopes.

## Trap reports

A trap flushes standard output, writes its report to standard error, and exits
with status one. The line gives the position of the failing operation in the
form of a compile-time diagnostic, `error: file:line:column: message`:

```text
error: main.plenty:7:11: index out of bounds: index 5, length 3
```

The position is the start of the expression that trapped: the left operand of an
arithmetic operator, the type name of a cast, the indexed collection, the
receiver of `unwrap` or `split`, or the `range` call. A trap in an imported function or a generic
instance reports the position in the function's body, in the file that defines
it, not the call or instantiation. The file is relative to the
[source root](../02-source/modules.md), `tools/geometry.plenty` for the
module `tools.geometry`, so an executable does not depend on the directory it
was built in. Compile-time diagnostics print the full path. Reporting a trap
allocates nothing.

When the failing body belongs to the embedded standard library, an additional
line names the nearest non-std caller:

```text
error: std/collections.plenty:20:10: index out of bounds: index 0, length 0
note: called from main.plenty:7:12
```

This caller is the actual invocation, including generic methods, indirect calls,
tail calls, and nested standard-library helpers. A user callback that calls std
becomes the nearest caller; a trap in the callback itself keeps its own ordinary
report. A suspended std generator uses its current resume site, not its creation
site. The note is omitted when there is no source caller on the current thread,
such as a std function started directly as a worker. Caller context does not
cross thread creation or executor submission. This is one caller note, not a
stack trace; it does not change the trap message into an API-specific error.

### Caller locations

The same nearest non-std caller is available to standard-library code that
reports a failure without trapping. [`std.test`](../../stdlib/test.md#assertions)
assertions use it to begin their report with `file:line:column: `. The compiler
records it as described above, so a std function needs no extra parameter and
user code no annotation. Plenty code outside the standard library cannot read
it: the hook, `__caller_location`, is rejected with `` `__caller_location` is
reserved for the standard library ``. Without a source caller on the current
thread the report has no position.

| Trap | Message |
|---|---|
| Integer overflow in `+`, `-`, `*`, `//`, `pow`, or negation, outside a `checked:` block | `integer overflow` |
| `//` or `%` by zero, outside a `checked:` block | `division by zero` |
| Integer `pow` with a negative exponent | `negative integer exponent` |

| Shift count negative or not below the integer width | `shift count outside integer width` |
| Numeric cast to an integer type that cannot represent the value | `integer cast out of range` |
| `unwrap()` on `Nothing` or `Err` | `unwrap on Nothing`, `unwrap on Err` |
| List, text, range, or `Bytes` index out of range | `index out of bounds: index I, length N` |
| Absent dictionary key | `dictionary key not found`; indexed assignment adds `; use insert to add a new key` |
| A range longer than `i64` | `range length exceeds i64` |
| Empty `split` separator | `string split requires a nonempty separator` |

A bounds report prints the index as written, before a negative index counts
from the end. A missing dictionary key is not printed.

[Stack overflow](../../toolchain/02-architecture/runtime.md) is reported
separately and has no position. [Trap sites](../../toolchain/02-architecture/runtime.md)
describes how positions reach the runtime.
