# Library exports

`export def` gives a top-level function an explicit C entry point when building a
library. It remains an ordinary callable Plenty function inside source programs.
`pub` independently controls imports between source modules.

```text
export def add(a: i32, b: i32) -> i32 = "calc_add":
    "Add two checked integers."
    a + b
```

The initial subset accepts numeric scalar parameters, `&T` / `&mut T` borrows of
numeric scalars, and numeric scalar or unit returns. Booleans, managed objects,
returned references, raw pointers, generic exports, and aggregate results are
rejected. The body can use ordinary language features
internally. No native Plenty object layout becomes part of the C ABI.

```sh
plenty --shared-library source.plenty --library-name calc -o build/libcalc.so
plenty --static-library source.plenty --library-name calc -o build/libcalc.a
```

Create the output directory first. Library names are non-builtin ASCII identifiers
beginning with a letter. C symbols must begin with the library name plus `_`;
`calc_plenty_interface_v1` is reserved for metadata discovery. Duplicate symbols,
import/export symbol collisions, and duplicate generated interface names fail
during checking or artifact validation. Imported source modules are included in
the library; their explicit exports must follow the same namespace convention.

Library builds require at least one export and do not require or run `main`.
Ordinary functions, including `pub` functions and an optional source `main`, retain
private symbols. Application commands still produce applications; their
`export def` functions are ordinary private Plenty functions in that mode.

## Artifacts and linking

Each build writes `calc.h`, `calc.plentyi`, and `calc.link-args.txt` beside the
requested library. The header is usable from C and C++. The interface module
contains generated `pub extern def` declarations that Plenty can import normally;
linking the matching binary remains explicit. Keep generated interfaces in the
consumer's source root, without a competing `calc.plenty` module at the same path.

The static archive includes the matching runtime without application startup.
Its link-args file contains required native dependencies, one argument per line,
including explicit `--link-arg` values. Build systems must pass each line as one
argument after the archive, preserving spaces. Static outputs use an
ar-compatible program selected by `--archiver` (default `ar`). They do not invoke
the linker driver. Static libraries in one process must use the same Plenty
compiler/runtime build; runtime symbols can be shared by the native linker.

Shared output uses the cc-compatible `--linker` driver and explicit `--link-arg`
options. It resolves native dependencies at build time and uses an export map to
hide its runtime and private Plenty symbols. Its link-args file is empty. The
consumer must arrange normal platform shared-library discovery, for example with
an explicit run path. Shared libraries contain their own private runtime.

Both outputs currently support `x86_64-unknown-linux-gnu` only. The compiler
embeds prebuilt application and library runtimes; producing a library does not
invoke rustc or Cargo. The Rust API provides `LibraryOptions`, `LibraryKind`,
`LibraryArtifacts`, and `compile_file_to_library`.

## Generated contracts

The checked export signatures drive C entry adapters, headers, and `.plentyi`
declarations. Headers describe scalar value copying and caller obligations,
including serialized calls on one caller thread and no unwinding or nonlocal
jumps through Plenty frames. Traps and explicitly unwrapped failures can terminate
the process; C entry adapters do not invent recoverable errors. Function docstrings
appear separately as author documentation, escaped so they cannot alter C syntax.

Scalar references become exact-sized C pointers: `&i32` becomes `const int32_t *`,
and `&mut i32` becomes `int32_t *`. They must point to one aligned, initialized,
non-null scalar for the whole call. Shared borrows permit shared reads but exclude
mutation through every alias. Mutable borrows require exclusive access and cannot
overlap any other borrowed argument. These requirements include concurrent and
reentrant access. The callee retains no borrowed storage after returning.

Adapters copy input scalars into private stack slots and write mutable values back
on every normal return. They read and write exactly the C type's size; C callers
never need to allocate a Plenty value slot. No heap allocation is needed for this
adaptation. A runtime trap does not return, and does not promise copy-back or
rollback. The generated `.plentyi` signatures preserve these loans for Plenty
callers, where the ordinary borrow checker enforces them.

The exact UTF-8 `.plentyi` bytes, including format version and target comments,
are embedded in a retained `.plenty.interface.calc` section. They survive supported
ELF stripping and section garbage collection once the archive member is linked.
The exported `calc_plenty_interface_v1(size_t *length)` function returns the
immutable bytes and writes their byte length. They have no NUL terminator and
remain borrowed until library unload; the caller must neither modify nor free
them. This discovery call allocates nothing.

The source interface is the first metadata format for this subset, not a stable
general-purpose binary Plenty ABI. Automatic extraction, compatibility validation
against a linked binary, runtime loading, owned object exports, and richer error
adapters are not implemented. Keep the generated header/interface and binary
together. The [export design](../../proposals/ffi-export-contracts.md) explains the
broader direction; the [backlog](../../backlog.md) tracks remaining work.
