# Visibility

`pub` exposes top-level functions, aliases, classes, enums, protocols, and
[constants](modules.md#constants). Other declarations are private to their module. Class fields and methods require their
own `pub`; public enums expose every variant, but enum methods need their own `pub`.

Generic functions check access in their **defining module**, including protocol
requirements and calls on a type parameter. Calling a generic function does not
lend it the caller's private access. To let a generic helper in another module
(such as `std.io.read_exact`) call a method, mark both the type `pub class` or
`pub enum` and the method `pub def`; a public method alone does not expose a private type.
Private fields and other private methods remain inaccessible. This keeps generic
code under the same lexical visibility rules as ordinary functions. A failing
protocol check is reported at the user's call; nested instantiations retain the
outer call location and an instantiation note. Code within the type's own module
can still use its private members through a locally defined generic function.

The standard library has one exception for its own modules. A private
declaration or member of a `std` module whose name starts with `_` is visible
to every other `std` module, so modules such as `std.crypto` can call native
hooks on `std.bytes.Bytes` without making them public API. Code outside `std`
sees these names as private; the [signature index](../../stdlib/signatures.md)
omits them.

A generated field constructor is public only when its class and every field are
public. An explicit `__init__` controls its own visibility. A public factory may
use private constructors inside their defining module. Construction through an
alias obeys the same access checks.

Public signatures, aliases, fields, and enum payloads cannot expose private
nominal types, including through aliases or nested containers. Public generic
contracts also require public arguments and constraint types; see
[generics](../07-generics/index.md).

Access checks apply to field reads, writes, borrows, methods, and construction.
Automatic destruction is unaffected by destructor privacy; direct lifecycle
calls are prohibited. Structural printing and equality include private fields,
so privacy does not hide data. `pub` does not specify a C ABI or binary export.
