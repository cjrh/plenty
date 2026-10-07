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
behavior. Extraction alone does not check that a subsequent link uses that same
binary; keep the binary and interface together.
