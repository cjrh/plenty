# Standard library

The standard library is the set of modules under the reserved `std` namespace.
They are written in Plenty or expose existing compiler facilities, and are embedded in the compiler: importing one needs no
source installation, network, Cargo, or rustc. Native compilation still requires
the supported [linker environment](../toolchain/01-building/linking.md).

A standard module must be imported. Names available without an import belong to
the [prelude](../language/02-source/names.md) and are specified under
[built-in facilities](../facilities/index.md).

## Modules

| Module | Contents | Error type |
|---|---|---|
| [`std.math`](math.md) | Floating-point functions, checked rounding, integer algorithms, generic numeric helpers, and constants | `MathError` |
| [`std.path`](path.md) | Lossless Linux paths, lexical operations, checked text conversion and escaped display | `IoError`, `AllocError` |
| [`std.fs`](fs.md) | Lazy directories, detached entries, metadata/links and Plenty walking with pruning | `IoError`, `AllocError`, `WalkError` |
| [`std.deque`](deque.md) | Owned generic ring buffers with constant-time removal at either end | `AllocError` |
| [`std.error`](error.md) | Operation and path context around a typed cause, descriptions, and standard-error reports | `Context[E]`, `AllocError` |
| [`std.bytes`](bytes.md) | Compact owning byte buffers with checked ranges, explicit copies and validated text conversion | `BytesError` |
| [`std.time`](time.md) | Checked durations, monotonic instants and deadlines, wall timestamps, and sleep | `RangeError` |
| [`std.thread`](thread.md) | Explicit scoped thread and owned-job imports | `ThreadError`, `SpawnError[F]` |
| [`std.channel`](channel.md) | Bounded channels, endpoints and selection | Channel/send/receive/selection errors |
| [`std.concurrent`](concurrent.md) | Executors, futures and parallel operations | Pool/submission/future/parallel errors |
| [`std.sync`](sync.md) | Shared cooperative cancellation tokens | `AllocError` |

Planned modules appear only in the [backlog](../../backlog.md). The tutorial
introduces the library in
[Use the standard math library](../../tutorial/07-modules/03-standard-math.md),
[Enumerate and classify paths](../../tutorial/08-resources/21-directories.md), and
[Prune before descending](../../tutorial/08-resources/22-walking.md).

## Importing a standard module

Standard modules use the ordinary
[import forms](../language/02-source/modules.md):

```python
import std.math
import std.math as math
from std.math import sqrt, pi
from std.math import MathError as Error
```

An import binds only the names it lists. `import std.math` requires the full
`std.math.sqrt` qualification; an alias shortens it.

`std` is reserved. Each of these is an error:

- a `std` directory, `std.plenty`, or `std.plentyi` in the source root;
- a declaration named `std`;
- an import that binds a non-standard module as `std`.

An unknown standard module, including a bare `import std`, is an error that
lists the available modules. It never falls back to a local file.

## How standard modules are compiled

A standard module passes through ordinary parsing, visibility checks, type
checking, specialization, and native emission. Its private helpers stay private.
Repeated imports, through any alias or dependency path, share its nominal
types, so a `MathError` from one module equals a `MathError` from another.

The four concurrency modules export canonical builtin identities instead of
wrapper functions. Their imports reach the existing ownership and worker checks
directly, including scoped joins and blocking-wait diagnostics. Their prelude
spellings remain available. Importing one of these under its own builtin name
is permitted only when it denotes the same builtin; unrelated name collisions
remain errors. Generic constructor and error aliases retain the same types.

Embedded modules have toolchain identities such as `std/math.plenty`,
independent of the installation path. A diagnostic located inside a standard
module, such as a note on a generic helper instantiated with an unsupported
type, uses that name. Imports currently require the file-based compilation APIs.

The [runtime packaging](../toolchain/02-architecture/runtime.md) page describes
how the sources ship with the compiler.

## Reading a module page

Each module page gives every public function's signature and states:

- whether it returns a plain value or a `Result`, and which error variants it
  reports;
- whether it borrows or consumes its arguments;
- whether it allocates;
- the limits that differ from the Python module of the same name.

Recoverable failures use typed errors: `std.math` defines `MathError`, while
`std.path` and `std.fs` share the native `IoError` and storage `AllocError`.
The filesystem walker adds `WalkError` to retain attempted paths and operations.
`?` propagates an error to a caller returning the same error type or
[`Failure`](../language/08-errors/01-sums-and-propagation.md).
[`std.error`](error.md) states the conventions modules follow for typed causes,
native codes, and allocation failure. Keyword and
default arguments are unimplemented, so every argument is explicit.
