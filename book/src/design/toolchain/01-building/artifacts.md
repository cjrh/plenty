# Library artifacts and publication

Library builds produce a binary, C header, source interface, and native link
arguments. Export ownership and ABI rules belong in the
[C interface contract](../../facilities/04-interop/index.md).

Embedded metadata can be extracted without loading or executing the binary.
Explicit interface verification checks the contract and its compatibility symbol;
see [embedded contracts](../../facilities/04-interop/contracts.md).

For embedding applications, `read_library_interfaces(path)` returns all
`LibraryInterface` entries sorted by library name, including exact source and
target; `extract_library_interface(path, name, output)` selects one for output.
`verify_library_interface(binary, interface)` performs explicit verification
without executing a linker or library code.

## Publication guarantees

The compiler stages a library's files in a private directory beside the requested
output, on the destination filesystem. It checks destinations before building
and before publication. Each must be absent or an ordinary file; symlinks,
directories, and paths overwriting loaded source or imported interfaces are rejected.

Compilation, native tool, argument, and staging failures preserve previous files.
Publication replaces files by individual atomic renames and keeps backups until
the complete set is installed. An installation error rolls back replacements and
removes new outputs. If rollback fails, the diagnostic identifies the retained
recovery directory containing backups.

Publication is not a crash-atomic transaction across files. Build systems must
exclude concurrent writers and coordinate readers. A crash or power loss can
leave a mixed set; fingerprints and verification help detect it.

Interface extraction uses the same mechanism for its single output. It rejects
symlink destinations and cannot overwrite the input binary. Native tools receive
paths as individual arguments, including paths with spaces or commas.
