# Rust-source backend semantic probes

The newer [checked emitter](emitter/README.md) consumes real Plenty checked IR
and measures Rust compile/link costs. The probes directly in this directory
remain hand-lowered and cover richer ownership semantics than that emitter.

These are **hand-lowered candidate Rust implementations**, not a new emitter.
The earlier `experiments/rust-crates/run.py` contains the actual restricted
Plenty-to-Rust emitter. This experiment asks additional semantic and
Rust-facing API questions without pretending to implement a full backend.

Run from this directory, supplying a built Plenty compiler:

```sh
cargo fmt --check
cargo clippy --all --workspace --all-targets -- -D warnings
python3 run.py --plenty ../../target/release/plenty
```

The runner uses only the standard Python library and offline dependency-free
Cargo builds. It needs Unix resource limits (tested on Linux), rustc, Cargo,
and a working Plenty native toolchain. It disables core dumps for intentional
abort tests. Results and binaries stay under ignored `target/`; the runner
writes `target/observations/results.json`. Runtime allocation fault injection
is not required or exercised. `usize::MAX` tests capacity failure, not physical
out-of-memory exhaustion.

`semantics.plenty` and `overflow.plenty` are real Plenty input, compiled in both
ordinary and release modes. `src/lib.rs` manually expresses selected matching
semantics. The runner compares their stdout and success/failure category, and
checks that overflow is reported; it does not assert identical trap formatting
or failure status numbers. See [RESULTS.md](RESULTS.md) for recorded evidence.

`tests/rust_host.rs` is a separate Rust test crate consuming the candidate
library: Rust owns a vector and a standard-library `Cursor`, the library
borrows them, and a Rust closure crosses the interface by value. No C ABI,
serialized values, or opaque-handle boxes are involved. These signatures are
handwritten candidate contracts; no automatic trait or lifetime discovery is
demonstrated. The closure's Box deliberately comes from the host.

The negative `.rs` files must fail, with E0499 and E0382 respectively. Do not
add them as Cargo targets or fix the errors: they demonstrate the rejected
naive indexed-borrow translation and Rust's use-after-move check.

These hand-lowered probes do not change compiler behavior. Full collection
representation, source maps, module linking, arbitrary control flow and automatic
emission are outside these original probes; see the separate checked emitter
above for the newer scalar work. The design lives in the book's
Rust-source-backend proposal.
