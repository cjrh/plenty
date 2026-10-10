# Trusted loading primitives

Trusted interfaces can implement their own runtime loader with these exact
versioned declarations. They are checked exceptions to the reserved `plenty_`
symbol namespace; opaque type names are chosen by the binding author.

```text
opaque Lease
opaque Address
extern def open_raw(path: &str as utf8, output: &mut Lease) -> u32 = "plenty_library_open_v1"
extern def symbol_raw(lease: Lease, name: &str as utf8, output: &mut Address) -> u32 = "plenty_library_symbol_v1"
extern def close_raw(lease: Lease) -> () = "plenty_library_close_v1"
extern def check_raw(lease: Lease, discovery: &str as utf8, expected: &str as utf8) -> u32 = "plenty_library_contract_v1"
extern def require_origin(expected: Address, actual: Address) -> () = "plenty_library_require_origin_v1"
```

Open and lookup return zero and write output only on success. Errors are 1
(OutOfMemory), 2 (CapacityOverflow), 3 (InvalidPath), 4 (OpenFailed), 5
(InvalidSymbol), 6 (MissingSymbol), and 7 (IncompatibleContract).
Paths must be nonempty without NUL; symbols must be ASCII C identifiers shorter
than 512 bytes. Path conversion is fallible; lookup and error handling allocate
nothing. The OS loader has independent memory management.

Successful open grants one lookup lease. Close consumes it exactly once; null is
a no-op. Other operations need a live lease, and wrappers must close it on every
exit, including partial lookup failure. Closing never unmaps successfully loaded
code or native static state; addresses remain valid until process exit. The
GNU/Linux implementation uses eager local binding. Imports alone do not open
libraries, and native constructors may run before later checks fail.

Private opaque addresses can select typed [indirect extern calls](trusted-declarations.md).
The binding must verify every lookup, exact signature, ownership, and thread
contract. Low-level lookup does not automatically verify metadata.

`check_raw` resolves the named discovery function with C signature
`const uint8_t *(size_t *)`, then compares exact length and bytes with expected.
Expected metadata must be nonempty and at most 4 MiB. Null, wrong-sized, or
different discovery data returns IncompatibleContract; missing discovery returns
MissingSymbol. Comparison allocates nothing. Discovery must supply valid readable
memory; matching cannot prove memory safety or undo native constructor effects.

`require_origin` compares non-owning identities. Null expected identity or mismatch
terminates with the instance-mismatch diagnostic.
