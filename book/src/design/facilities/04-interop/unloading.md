# Unloadable shared libraries

On Linux GNU x86_64, `--unloadable-library` checks an explicit, limited runtime
profile when building a shared library:

```sh
plenty --shared-library plugin.plenty --library-name plugin \
  --unloadable-library -o libplugin.so
```

The compiler API exposes the same choice as `LibraryOptions.unloadable`.
The generated C header identifies profile version 1 and its caller obligations.
The profile is also recorded in the fingerprinted embedded interface, so ordinary
and certified builds have distinct compatibility guards even with identical
export signatures. Interface extraction preserves this marker.
This profile supports actual `dlclose` unloading and subsequent fresh loads,
including calls from host threads that remain alive between loads. It does not
promise real-time execution or override the [host-thread contract](host-threads.md).

## Supported code

The checker follows every exported function, direct calls, address-taken
functions, and implicit class destructors. It admits scalar arithmetic, trusted
raw pointers, ordinary classes and enums, boxes and shared values, and the
listed built-in text, byte-buffer, list, dictionary, set, range and slice operations. Ordinary
allocation is supported: the library runtime uses the system allocator rather
than application thread caches. Collection hashing keeps only fixed-size state.

Thin Plenty function values are supported: their address-taken targets are
checked too. The initial profile rejects generators, captured or erased callable
values, threads,
executors, cancellation and synchronization facilities, actors, general IO, clocks,
networking, signal listeners, native engines and other unaudited operation
families. Rejection means the operation has no unloading contract in this
profile; it does not mean all such operations inherently prevent unloading.
F-strings are supported, including the shared formatter's narrow unbuffered
stderr reporting primitive; this does not admit general stream or filesystem IO.
The check is conservative across runtime branches. New primitives must be
audited before being admitted. Uncalled helpers do not
extend the profile, and an import alone grants no capabilities.

## Final close

Before the final native `dlclose`, the host must:

- Finish every invocation, including nested calls and callbacks, and prevent
  new invocations from starting.
- Destroy every Plenty-owned handle or value using its originating library's
  destruction function while that library is still loaded.
- Stop retained callbacks and discard every pointer into the library, including
  function addresses, static storage and borrowed data.
- Complete the teardown promised by every trusted foreign binding.

Host threads can remain alive after returning from their Plenty calls. Managed
export handles retain their existing [thread and ownership rules](owned-exports.md);
unloadability does not authorize moving those handles between host threads.
A fresh load creates a fresh library instance; addresses and handles from a
previous instance are invalid, even when the loader reuses the same address.

Native `dlclose` can retain a mapping because another reference, dependency or
`RTLD_NODELETE` keeps it alive. The profile makes unloading safe under its
contract; it cannot force an external loader to remove a mapping. Repeated
plugin initialization/deinitialization within one loaded instance remains the
plugin author's responsibility.

## Trusted foreign code

The compiler cannot inspect named C functions or calls through raw function
addresses. Selecting this profile additionally asserts that those bindings
leave no background execution, callback, thread-local destructor or process
registration pointing into the library after final teardown. This includes
native dependencies and callbacks that re-enter Plenty. A binding that starts
work must finish and join it before close; a completion flag written just
before a worker returns is insufficient because the return still executes
library code. Thread safety alone is not an unloading contract.

The runtime deliberately rejects signal listeners and actors because they retain
process registrations or reserved memory without a full teardown API. Detached
DNS workers are also outside the profile. Application allocation caches use TLS
destructors, which can prevent a library from unloading while foreign threads
remain alive; the separate library runtime bypasses those caches.

[Plenty's generated runtime loaders](loading.md) continue to keep loaded code
resident. This profile is for external native hosts that manage their own
`dlopen`/`dlclose` lifecycle; it does not change those loaders into unloaders.

`--runtime-interface` and `runtime_interface_source` currently describe ordinary
libraries. Their generated loader contract therefore does not match a certified
binary, whose fingerprint includes the unloading marker. Use the certified
build's C header and extracted interface for native-host integration.
