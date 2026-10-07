# Inspect embedded contracts

Extract a generated Plenty source interface directly from a library:

```sh
plenty --extract-interface build/libcalc.so --library-name calc -o calc.plentyi
```

This reads file data; it never loads or executes the library. ELF objects, shared
objects, executables, and regular static archives are supported. Stripped shared
objects retain the contract. An archive member's contract survives section
garbage collection when that member is included in a final link.

The Rust API `read_library_interfaces(path)` returns every embedded
`LibraryInterface`, sorted by library name, with its exact source and target.
`extract_library_interface(path, name, output)` selects one. A file can hold
several different library contracts. Duplicate namespaces are rejected, even when
their bytes agree, because the originating implementation would be ambiguous.

Inspection validates the format version, library/section name agreement, C ABI,
UTF-8, and supported target. Limits are 512 MiB per binary, 4 MiB per contract,
16 MiB total contract data, and 256 contracts. Malformed, compressed, and thin
archive interfaces are rejected; thin archive paths are never followed. Only
dedicated `.plenty.interface.NAME` sections are read, not matching strings found
elsewhere in a binary. Extraction errors leave the previous output alone and
cannot overwrite the input library.

An extracted interface has the same trust requirements as any `.plentyi` source
contract. Metadata is not authentication or proof of the implementation's
behavior.

## Compatibility fingerprints

Generated interfaces include a SHA-256 fingerprint of their canonical source
contract and a no-op C symbol named `NAME_plenty_contract_v1_HASH`. All generated
Plenty call wrappers reference that symbol before entering the native function.
A stale interface therefore requires a symbol that an incompatible library does
not provide: static linking fails, and shared linking or later symbol resolution
fails. The guard does not allocate. C headers declare the same optional guard.

Signatures, borrowing modes, generated cleanup/adaptation rules, namespace, and
target affect the fingerprint. Parameter spelling, author documentation, private
class layout, and function bodies do not. Whole-interface matching is deliberately
conservative: adding an export also changes the fingerprint. It does not promise
compatibility between compiler/runtime builds for static runtime composition.

Inspection checks the embedded hash and guard declaration. Older format-1
interfaces without fingerprints can still be extracted; `LibraryInterface`
reports `fingerprint: None` for those. A matching hash detects mismatched contracts,
not malicious libraries or implementation bugs.

For an explicit pre-link check, run:

```sh
plenty --verify-interface build/libcalc.so calc.plentyi
```

The Rust API is `verify_library_interface(binary, interface)`. It requires a
fingerprinted generated interface, validates both inputs, compares the exact
contract, and checks that the corresponding function symbol is defined. For a
shared library, the symbol must be present in the dynamic symbol table. Metadata
alone, without its implementation symbol, cannot pass. Older interfaces must be
regenerated to use this check. No linker or library code runs during verification.

Verification is a build-time check, not a lock on a file: replacing the binary
afterward still requires the normal link/symbol guard. Arbitrary `--link-arg`
arguments are not interpreted as paths to inspect automatically.
