# Runtime library loading

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
