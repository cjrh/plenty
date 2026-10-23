# Recorded semantic probe results

Observed 2026-10-24 on x86_64 Linux, from Plenty main `00d32a13` (version 0.3.0).
Rust: `rustc 1.99.0 (b940084d7 2026-09-28)`, LLVM 23.1.1, edition 2024.
The separate local Rust source checkout used for source inspection was
`080cc27146389dbb5c8772c476b1937be9c6110f`; it is not the tested compiler build.

| Probe | Observed result |
| --- | --- |
| Plenty and hand-lowered Rust semantic traces, ordinary/debug and release | Both print `33, 2, 1, 3, 4`, one number per line; successful exit. |
| Checked addition overflow in both modes | Both fail, report overflow and print no destructor trace. |
| Default Rust panic, both modes | Failure prints destructor trace `99`; this is a mismatch for a Plenty fatal trap. |
| Separate `-C panic=abort` Cargo build | Panic fails without the destructor trace. |
| Native Rust caller | Borrows original vector element (pointer equality), uses a native `Cursor`, receives typed I/O error and passes a consuming closure. |
| Reservation-before-element-effects | Impossible capacity returns allocation error with zero element callbacks; ordinary construction returns `[0, 1, 2]`. |
| Distinct constant list-index loans | Plenty accepts and executes; literal Rust translation rejects with E0499; `split_at_mut` candidate passes. |
| Moved Rust owner | rustc rejects subsequent access with E0382. |

Two library tests and one separate Rust-host integration test pass in each of
debug/release. The Python runner checks four Plenty/Rust differential cases,
three panic cases and two compiler-rejection cases. Formatting and Clippy with
warnings denied pass for the isolated Cargo workspace.

These results establish small semantic requirements and possible translations.
They do not establish a backend, general ownership translation, allocator parity,
safe inference of foreign signatures, source diagnostics, or performance. No
compile-time or runtime speed ranking was measured. The capacity test is not an
out-of-memory test; the fallible Vec sketch omits Plenty's separate owner
allocation and exact `AllocError` classification.
