# Embedded contracts and compatibility

Library artifacts contain the exact generated UTF-8 `.plentyi` contract,
including its format and target. The versioned discovery function
`NAME_plenty_interface_v1(size_t *length)` returns immutable bytes and writes
their length. The bytes have no NUL terminator; callers cannot modify or free
them and must keep the code resident. Discovery allocates nothing.

Contracts survive supported ELF stripping and section garbage collection once
their archive member is linked. Inspection reads dedicated contract sections,
not arbitrary matching strings. It never loads or executes the binary. ELF
objects, shared objects, executables, and regular static archives are supported;
thin/compressed archives and malformed contracts are rejected without following
thin-archive paths.

Inspection validates version, namespace/section agreement, C ABI, UTF-8, and
target. Limits are 512 MiB per binary, 4 MiB per contract, 16 MiB total contract
data, and 256 contracts. Duplicate namespaces are rejected even if bytes agree.
Extraction preserves a previous output on failure and cannot overwrite the input.
See [build commands](../../toolchain/01-building/commands.md) for extraction and
verification.

## Compatibility fingerprints

Generated interfaces carry a SHA-256 fingerprint and require
`NAME_plenty_contract_v1_HASH` before each native call. An incompatible binary
lacking that symbol fails linking or symbol resolution. The guard allocates
nothing; C headers declare it for optional C-side checking.

Signatures, borrowing, cleanup/adaptation rules, namespace, and target affect the
fingerprint. Parameter spelling, docstrings, private layout, and bodies do not.
Matching is conservative: adding an export changes the fingerprint. This is not
a general stable Plenty binary ABI or a promise of compatibility between
compiler/runtime builds when combining static runtimes.

Verification validates the exact contract and checks its guard symbol is defined;
shared libraries must expose the guard dynamically. Metadata alone cannot pass.
Older format-1 contracts without fingerprints can be extracted but must be
regenerated for verification. Verification executes no library or linker code,
and does not prevent later binary replacement: the normal symbol guard remains
necessary.

Extracted contracts remain trusted `.plentyi` source. Metadata matching detects
contract mismatch, not malicious code, native bugs, or authenticity.
