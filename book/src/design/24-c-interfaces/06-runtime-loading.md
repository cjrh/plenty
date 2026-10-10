# Runtime library loading

Build the library and generate a runtime interface from the same source:

```sh
plenty --shared-library calc.plenty --library-name calc -o libcalc.so
plenty --runtime-interface calc.plenty --library-name calc -o plugin.plentyi
```

Consumers import `plugin`, call `plugin.load(&path)`, and use the resulting
library's methods. Their build requires no link argument for `libcalc.so`.
Generation accepts `--module-root`, protects loaded source files, and publishes
the `.plentyi` through staged replacement. It needs no linker or native compiler.
The Rust API offers `emit_runtime_interface` with the same publication rules.
This generated loader is distinct from the linked `calc.plentyi` contract;
`--extract-interface` and `--verify-interface` still work with that linked
contract. Runtime generation currently requires the original export source.
The [runtime-loading lesson](../../tutorial/62-load-a-library-at-runtime.md)
builds and runs a complete owned-object example during tutorial validation.

`runtime_interface_source(path, root, library_name)` in the compiler's Rust API
generates a trusted module from the same export source used to build a library.
Save it as, for example, `plugin.plentyi`. Its public API is `load(path: &str) ->
Result[Library, LoadError]`, and the returned `Library` has a method for each
export. Generated methods support numeric scalar inputs/results, shared or
mutable scalar borrows, unit results, and the scalar/unit Result forms supported
by C exports, plus owned handles described below. The exact error type and numeric width are preserved. Mutations to
borrowed inputs remain visible on Err, as with linked calls. Generating the
source does not run or link native code. The compiler type-checks the generated
module before returning or publishing it.

Load opens the requested library, checks exact discovery metadata, requires its
fingerprint guard, and resolves every export before returning a usable table.
The private lookup lease is cleaned up on every exit. A `Library` stores cached
addresses; calls use native indirect C instructions without repeated lookup.
Scalar adapters do not allocate; native callees retain their own allocation
behavior. The table itself is an ordinary Plenty class, stored inline.
Its private fields prevent callers from constructing an unchecked table.
Generation currently allows 128 functions including generated destructors, and
240 parameters per method (consuming owners also need temporary slots). Names
beginning with `_`, `new`, and `self` are reserved for generated fields and
lifecycle helpers. Exported class names `Library` and `load` are reserved for the
table and loader. Native symbol names must be shorter than 512 bytes, and the
expected contract must fit the 4 MiB discovery bound.

Loading performs three Plenty allocations: a temporary lease owner, a terminated
path buffer, and the returned method table. Any can report `OutOfMemory`; the
temporary values are reclaimed. Missing discovery/guard/export symbols never
produce a partial public table. Runtime tests inject failures at each allocation,
repeat incomplete lookups, and disable allocation across cached scalar calls,
borrows, and loader-error handling. Native constructors and the OS loader can
perform additional allocations outside this accounting.

The allocation-free builtin `LoadError` distinguishes OutOfMemory, CapacityOverflow, InvalidPath, OpenFailed,
InvalidSymbol, MissingSymbol, and IncompatibleContract. Generated loading uses
the normal `Result` and `?` rules. Metadata agreement is compatibility checking,
not authentication. Library bodies can still trap or violate their contracts.

## Owned objects

Generated loaders support the same `Result[Class, AllocError]` factories, class
borrows, and consuming class arguments as linked exports. Each returned wrapper
stores its native handle, originating library identity, and exact destructor
address. Objects may outlive the `Library` table; dropping them still runs the
originating destructor exactly once. Code residency supplies that lifetime.
Constructing the Plenty wrapper requires one fallible allocation before native
acquisition. On wrapper allocation failure, acquisition is skipped; on native
failure the empty wrapper is cleaned up. Consumed arguments transfer on both Ok
and Err, and returned owners receive a newly armed wrapper.

Two files can have the same public contract and different private object layouts.
Before passing any owned or borrowed class argument, a generated method compares
its originating contract-guard address with the table's identity. Mixing objects
from different loaded instances terminates with a diagnostic **before** crossing
the C boundary. This is a programming-error check, separate from recoverable
loading errors; it does not add an error variant to every exported method.
Repeated loads of the same resident instance are compatible. Keep calls and
objects on their creating thread, as required by the generated C contracts.

## Trusted binding primitives

Trusted `.plentyi` modules can build explicit runtime-loading wrappers using
versioned runtime declarations. These are the only exceptions to the
reserved `plenty_` C symbol namespace; the compiler checks their exact ABI.
Opaque type names are chosen by the binding author.

```text
opaque Lease
opaque Address
extern def open_raw(path: &str as utf8, output: &mut Lease) -> u32 = "plenty_library_open_v1"
extern def symbol_raw(lease: Lease, name: &str as utf8, output: &mut Address) -> u32 = "plenty_library_symbol_v1"
extern def close_raw(lease: Lease) -> () = "plenty_library_close_v1"
extern def check_raw(lease: Lease, discovery: &str as utf8, expected: &str as utf8) -> u32 = "plenty_library_contract_v1"
extern def require_origin(expected: Address, actual: Address) -> () = "plenty_library_require_origin_v1"
```

Open and lookup return zero on success and write their output only on success.
Error codes are 1 (OutOfMemory), 2 (CapacityOverflow), 3 (InvalidPath),
4 (OpenFailed), 5 (InvalidSymbol), 6 (MissingSymbol), and 7 (IncompatibleContract). Paths must be nonempty
and contain no NUL. Symbol names must be ASCII C identifiers under 512 bytes.
Lookup uses a fixed stack buffer. Path conversion is fallible; errors themselves
require no allocation. The OS loader has its own memory management.

On success, open produces one lookup lease. Close consumes that lease exactly
once; null is a no-op. Other operations require a live lease. The wrapper must
close it on every exit, including partial lookup failure. Closing never unmaps
successfully loaded code: mappings and their native static state stay resident
until process exit. Function addresses can therefore outlive the lookup lease.

The supported GNU/Linux loader uses eager local symbol binding. A path with `/`
is relative to the process working directory or absolute; a bare name uses the
platform loader search rules. No Plenty source import opens a library by itself.
Native constructors may run during open, including before a later binding check
fails. Loading a library trusts executable native code; it is not a sandbox or
an authenticity check.

Use a private opaque address with a typed indirect `extern def` to call a resolved
function. The binding author must check every lookup and provide the exact C
signature, ownership rules, and thread restrictions. These low-level primitives
do not check library metadata automatically. `check_raw` resolves a versioned
discovery symbol with signature `const uint8_t *(size_t *)`, then compares its
length and bytes against the expected contract. Expected metadata is limited to
4 MiB and must be nonempty; null, wrong-sized, or different returned data is an
incompatible contract. A missing discovery symbol is a missing-symbol error.
The comparison allocates nothing. The discovery implementation is trusted to
return valid readable memory; metadata checks cannot establish native memory
safety or prevent constructors from having run already.
`require_origin` compares two non-owning identity pointers. A null expected
identity or a mismatch terminates with the instance-mismatch diagnostic.
