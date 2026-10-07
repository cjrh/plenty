# C interop and dynamic libraries

Status: researched proposal, 2026-10-05. Nothing in this document is implemented
language syntax. The code blocks are sketches, not runnable Plenty examples.

Plenty should support ordinary C libraries without making every application
author become an FFI expert. A binding author supplies an explicit, trusted
contract; application code uses typed functions, ownership, borrowing, `Result`,
and automatic cleanup. The contract belongs in an importable interface module.
Static archives, linked shared libraries, and libraries opened at runtime should
use the same contract representation.

This does not require weakening ordinary Plenty ownership rules. It requires a
well-defined place where the compiler trusts external claims. A mistaken contract
or a buggy native library can still corrupt memory; annotations cannot prove C
implementation behavior. Ease of use comes from reusable bindings and generated
adapters, rather than pretending that the boundary is checked end to end.

## What existing systems teach us

Rust supports C shared-library linking and producing `cdylib` artifacts. Loading
a library at runtime is also possible, for example through `libloading`. The
important limitation is that signatures and ownership promises must be correct;
dynamic libraries are not fundamentally incompatible with Rust safety. We should
avoid attributing ecosystem usage patterns to an inability to use C DLLs.
[Rust linkage reference](https://doc.rust-lang.org/reference/linkage.html),
[libloading Library API](https://docs.rs/libloading/latest/libloading/struct.Library.html).

Cython's `.pxd` files demonstrate the value of reusable declaration modules and
compile-time namespacing. They are a useful aesthetic model for Plenty interface
files, though Plenty needs additional ownership and lifetime contracts.
[Cython external C declarations](https://cython.readthedocs.io/en/latest/src/userguide/external_C_code.html).

Zig provides C type primitives, C-compatible records, exports, and header
translation. Its documentation explicitly requires matching target and C compiler
flags during translation: `long`, packing, enum representations, and other ABI
details depend on them. Reusing a mature C frontend is attractive; copying the
surface syntax alone would not provide this machinery. `zig translate-c` produces
Zig code, not a ready-made Cranelift interface or ownership specification.
[Zig C interop](https://ziglang.org/documentation/master/#C).

`ctypes` offers a convenient model for loading libraries, typed function lookup,
and conversions. It also documents that invalid non-null pointers can crash the
process. Plenty should retain the convenience while resolving known signatures
ahead of time and checking its side of each contract.
[Python ctypes](https://docs.python.org/3/library/ctypes.html).

## Separate source visibility, binary linkage, and loading

These are three independent decisions:

| Concern | Proposed meaning |
| --- | --- |
| `pub` | Another Plenty module may name the declaration. |
| C export | Emit an explicitly named, C-compatible externally visible symbol. |
| Static dependency | Link an archive into the application or library. |
| Shared dependency | Link against a shared library; the platform loader loads it. |
| Runtime loading | Open a selected library at runtime and resolve a known interface. |

Making a function `pub` must not automatically freeze its native ABI. Ordinary
Plenty functions can keep compiler-private conventions and symbol mangling.
Export declarations generate C ABI entry adapters and, eventually, C headers.
Likewise, source imports should not have incidental side effects that load DLLs.
Runtime loading is an explicit operation returning `Result`.

The initial interface file can use a dedicated suffix such as `.plentyi`, imported
through the same absolute module namespace as Plenty source. The suffix and exact
grammar remain open; distinguish interface files explicitly in the compiler rather
than deciding how to interpret a file from its contents.

## Two layers of declarations

Keep the actual C signature separate from the ergonomic Plenty signature. A C
function returning an integer and writing through an output pointer should not
be falsely declared as returning a native Plenty `Result`. The compiler generates
the conversion between these layers.

Illustrative interface sketch, deliberately not executable:

```text
# graphics.plentyi — proposed concepts, grammar undecided
foreign library graphics:
    abi = "C"
    opaque Image

    symbol image_load(path: ptr[const c_char], out: ptr[ptr[Image]]) -> c_int
    symbol image_destroy(image: ptr[Image]) -> ()

    # A declarative adapter, not the actual C function signature:
    pub bind load(path: str) -> Result[owned Image, ImageError]:
        call image_load(c_string(path), out image)
        success when return == 0
        on success own image with image_destroy
        on failure image is null
```

An initial implementation should support a smaller contract grammar and handwritten
adapter bodies before attempting this full declarative convenience. `ptr`, `const`,
`owned`, `symbol`, and `bind` here name concepts, not agreed Plenty keywords.

A pointer-to-immutable-C-data is not automatically a Plenty immutable reference:
some other C alias may mutate it. The binding must establish that the stronger
Plenty borrowing guarantee holds, or copy the data into a Plenty value. `const`
alone neither proves this guarantee nor says how long memory remains valid.

## What an interface contract must express

| Topic | Information the compiler or adapter needs |
| --- | --- |
| Native ABI | Target, calling convention, symbol name, scalar widths, argument/result lowering. |
| Buffer extent | Pointer plus length/capacity, units, alignment, and initialized extent. |
| Nullability | Whether null is permitted and whether it means absence, empty buffer, or failure. |
| Access | Read/write permissions, exclusivity where required, and whether native code retains an alias. |
| Lifetime | Call-only access, borrowing from a named owner, or independent owned result. |
| Transfer | Who owns an input or output on both success and failure paths. |
| Release | Exact destructor/deallocator, library instance, allocator context, size/alignment when needed. |
| Failure | Status mapping, immediate `errno` capture where specified, initialized outputs on failure. |
| Callbacks | Signature, borrowed/retained context, registration lifetime, reentrancy, thread of invocation. |
| Threads | Thread affinity, concurrent-call permissions, movable/shareable handles. |
| Control flow | No unwinding or nonlocal jump through Plenty frames. |

Useful default: pointer access lasts only for the call and native code does not
retain the pointer. This must be an explicit binding-author promise, not an
inference from an unannotated C header. Contracts absent from the declaration leave
a raw operation requiring an explicit trusted/unsafe adapter.

Ownership transfer on failure matters. If a C call consumes a handle even when it
returns an error, the source value remains moved on both paths. If a call accepts
ownership only on success, an adapter must preserve or return the original owner
on failure. A single `owned` marker without conditional semantics is insufficient.

Borrowed results initially require a named owner and simple lifetime relation,
such as “valid while this image remains borrowed.” Reject relationships the
checker cannot express rather than silently extending the lifetime. This is a
reason to design FFI contracts alongside public borrowing, not after freezing it.

## Representation and allocation

Normal Plenty `class`, `enum`, `str`, `list`, `dict`, `set`, `Option`, and `Result`
representations remain private to the compiler/runtime. They should never become
C-compatible merely because a function is exported. Current managed objects have
ownership headers and runtime metadata; C code does not know those conventions.

Provide a separate opt-in foreign record representation later, with target C
layout, fixed fields, alignment rules, and compile-time layout checks. Initially
support scalar values and pointers to opaque objects, then plain records passed
by pointer. Records passed by value, unions, bitfields, packed layouts, variadic
calls, and platform-specific calling conventions need deliberate ABI work.

Foreign type aliases such as `c_int`, `c_long`, `c_size_t`, and `c_char` belong in
an explicit FFI module. They describe target C types; they do not reintroduce
ambiguous native Plenty `int`. C enum values and boolean representations also need
declared conversion rules rather than unchecked reinterpretation.

The single native `str` stays UTF-8 with a length and embedded-NUL support. A C
string adapter validates or rejects embedded NUL, creates a terminated buffer
when required, and reports allocation failure. Returned bytes need explicit
encoding, length, and ownership. No automatic assumption that arbitrary C bytes
are valid UTF-8. A byte-buffer/view API should precede useful buffer-oriented FFI.

Never free a foreign allocation using Plenty's global allocator by default. Owned
foreign handles carry their exact release operation and any required allocator
context. Exported APIs provide matching release functions for their own outputs.
Crossing DLL boundaries on systems with different runtime heaps makes this
particularly important. Custom allocators and their lifetimes should use this
same provenance model, rather than a separate incompatible mechanism.

A fallible adapter must release partially acquired foreign resources if subsequent
conversion fails. Conversely, an allocation error result cannot reliably rescue a
native library that itself aborts on allocation failure. Bindings must document
which failures can actually be returned. Native error channels cannot be guessed
from names or inferred solely from C signatures.

## Dynamic library lifetime and callbacks

The easiest first runtime-loading policy is to keep successfully loaded libraries
resident until process exit. That avoids dangling function pointers, borrowed
static data, callbacks, destructors, and native background threads after unload.
Libraries can still be loaded lazily and report missing symbols as `Result` errors.
Unload support can follow once all these lifetime relationships are representable.

If unloading is later supported, the library owner must outlive every function
handle, owned foreign value, destructor thunk, callback registration, and borrowed
piece of static data. Merely finishing the last direct function call is not enough.
Even a registration destructor may need to wait for in-flight callbacks before
releasing its context. Loader APIs themselves can run native initialization code;
opening a library is not a passive metadata read.
[libloading loading and symbol contracts](https://docs.rs/libloading/latest/libloading/struct.Library.html).

Start callbacks with synchronous, same-thread calls and an explicit context
pointer. Reentrant callbacks must obey active borrows; they cannot acquire another
mutable borrow of the object already lent to C. Retained or foreign-thread
callbacks require a later registration/lifetime model and thread-safe runtime
support. Do not mark handles transferable between threads just because they are
represented by pointers.

Foreign exceptions, `longjmp`, and Rust unwinding through Plenty frames are outside
the initial contract. C++ adapters catch exceptions on their side and return a
status. Plenty currently terminates on runtime traps and its Rust runtime uses
abort-on-panic; library exports initially share this limitation. A status-returning
wrapper does not turn those aborts into recoverable errors.
[Rust FFI and unwinding](https://doc.rust-lang.org/nomicon/ffi.html#ffi-and-unwinding).

## Compiler and runtime implications

Repository evidence at proposal time:

- `src/codegen.rs` emits native object files, enables position independence, uses
  `CallConv::Tail` for Plenty functions, and explicitly uses `SystemV` for runtime
  calls. Foreign calls need target-derived C conventions; hard-coded System V is
  not a portable DLL implementation.
- `build.rs` embeds a host-target runtime archive. Shared-library output needs
  a reviewed runtime link mode, exported-symbol policy, and startup path without
  the executable's `main`/`plenty_main` dependency.
- `plenty-runtime/README.md` documents non-atomic counts, thread-local destruction,
  and compiler-private 64-bit slots. Native threads cannot safely share current
  Plenty objects merely because an FFI function spawns them.

Cranelift supplies native calling conventions and indirect calls. Its `Tail`
convention is not a stable external ABI. Generate C entry/exit adapters around
Plenty functions and preserve the internal convention. C aggregate classification
and target layout remain frontend responsibilities; an IR function signature alone
does not translate arbitrary C headers.
[Cranelift CallConv](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/isa/enum.CallConv.html).

Do not require a JIT or `libffi` for runtime loading of statically known signatures.
Resolve addresses once, then emit typed indirect calls in the AOT program. Arbitrary
runtime-discovered signatures are outside scope. Keep header translation as an
optional, cacheable binding-generation tool so ordinary Plenty builds remain fast
and do not depend on a bundled C frontend.

## Suggested delivery sequence

1. Reserve distinct internal, C import, and C export ABI concepts now. Keep native
   object layouts private and module visibility separate from binary exports.
2. Add target-correct scalar/pointer C imports, static/shared link configuration,
   explicit raw adapters, opaque owned handles, and call-scoped buffer contracts.
3. Add C exports and static/shared-library outputs with generated headers,
   matching destroy functions, versioned symbol naming, and C integration tests.
4. Add explicit runtime loading of known interfaces, initially without unloading.
5. Add record layout, retained callbacks, header-assisted binding generation, and
   more expressive borrowed results as independent extensions.

Every stage needs native integration tests built against a small C fixture:
successful/error transfers, partial initialization, matching destruction,
embedded-NUL rejection, library lifetime, and reentrant callbacks. Validate each
supported ABI with actual C callers/callees; metadata parsing tests alone are
insufficient.

Open decisions before implementation: first supported host ABIs; naming and file
format for interfaces; exact trusted-boundary syntax; whether the first useful
release includes shared-library exports as well as imports; and which borrowed
buffer types the language exposes. None requires adopting Python object layouts,
a garbage collector, Rust trait lookup, or a public Plenty native ABI.
