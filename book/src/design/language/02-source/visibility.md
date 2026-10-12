# Visibility

`pub` exposes top-level functions, aliases, classes, enums, protocols, and numeric
constants. Other declarations are private to their module. Class fields and methods require their
own `pub`; public enums expose every variant.

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
