# Unicode scalar primitives

`lookups.rs`, `normalize.rs`, `perfect_hash.rs`, and `tables.rs` are unmodified
sources from the crates.io release **unicode-normalization 0.1.25**, Unicode
**17.0.0**, upstream https://github.com/unicode-rs/unicode-normalization.
The adjacent COPYRIGHT and MIT/Apache licenses are copied from that release.
`tables.rs::UNICODE_VERSION` is the authoritative data version.

Only scalar lookup, full scalar decomposition and pair composition are used.
The small parent-module `quick_check` enum exists to compile unused upstream
table functions; no upstream iterator or allocator is included. Crate-relative
imports are preserved through private root aliases. Vendoring keeps both
direct-rustc runtime archives self-contained and lets Plenty perform fallible
scratch allocation. Do not replace that path with infallible tinyvec iterators.

To upgrade: download a pinned crates.io release, copy these four files and all
three license/copyright files unchanged, inspect API/version changes, update
`stdlib/std/unicode.plenty` and the reference version, and replace
`tests/fixtures/unicode-17/NormalizationTest.txt` with the matching official UCD
fixture (renaming the fixture directory and test include when appropriate).
The fixture source is https://www.unicode.org/Public/17.0.0/ucd/NormalizationTest.txt;
its Unicode license is stored beside it. Run `cargo test --locked --features
runtime-checks --test integration test_std_text_unicode::`, tutorial and
runtime-packaging tests, and strict workspace Clippy. Review changed
normalization output as an intentional library contract update. Nothing is
downloaded during normal builds.
