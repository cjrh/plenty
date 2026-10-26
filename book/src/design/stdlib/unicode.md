# `std.unicode`

Normalization and a deliberately small classification surface, pinned to
Unicode **17.0.0** independently of the host Rust compiler. `version()` returns the
string `"17.0.0"`.

| Function | Result |
|---|---|
| `nfd(text: &str, max_scalars: i64)` | Canonical decomposition |
| `nfc(text: &str, max_scalars: i64)` | Canonical decomposition and composition |
| `nfkd(text: &str, max_scalars: i64)` | Compatibility decomposition |
| `nfkc(text: &str, max_scalars: i64)` | Compatibility decomposition and canonical composition |
| `combining_class(scalar: &str)` | `Option[u32]`: canonical combining class of exactly one scalar |
| `is_mark(scalar: &str)` | `Option[bool]`: General_Category is Mark for exactly one scalar |

Normalization returns `Result[str, std.text.TextError]`. It borrows input and
returns independent text. The explicit scalar limit bounds the input, fully
decomposed intermediate, and result. Exceeding any gives `LimitExceeded`; a
negative limit or one above `i64.MAX // 4` gives `InvalidLimit`. An empty string
fits a zero limit. Output UTF-8 is consequently at most four times the limit.
The limit counts Unicode scalars, not bytes or grapheme clusters, and canonical
composition does not let an oversized decomposition bypass it.

Classification allocates nothing and returns `Nothing` for zero or multiple
scalars. Mark and nonzero combining class are distinct properties. This module
does not offer locale collation, grapheme segmentation, case folding, general
categories beyond Mark, or display-width calculations. Existing string casing
and ASCII classification remain the [text builtins](../facilities/01-builtins/text.md).
Compatibility normalization can erase meaningful presentation distinctions;
callers choose it explicitly. No implicit normalization occurs in equality,
hashing, paths, decoding, or text builders.

Stable canonical ordering and blocked composition follow
[UAX #15](https://www.unicode.org/reports/tr15/), including Hangul and composition
exclusions. Checked Plenty performs the algorithms; allocation-free scalar
lookups reuse bundled `unicode-normalization` 0.1.25 tables and primitives.
Canonical ordering uses stable insertion: O(n²) worst-case time for adversarial
descending combining runs, O(n) otherwise, where n is decomposed scalar count.
Storage is O(n), with two scalar lists and bounded UTF-8 construction; allocation
failure is recoverable. Choose a suitably small limit for untrusted input.
No hidden global state or thread affinity is introduced; ordinary ownership
and worker-effect checking still apply.

The complete Unicode 17 `NormalizationTest.txt` fixture checks all twenty
normalization equalities for each row through one compiled Plenty harness.
Unicode upgrades are deliberate source changes: replace the pinned upstream
sources, update the version function, fixture and licenses together, and rerun
the corpus, allocation tests, tutorial tests and packaging tests. Do not derive
table versions from the host toolchain. Vendoring preserves the dependency-free
runtime build and avoids the upstream iterator's infallible scratch allocation.
Provenance and exact update steps are recorded beside the vendored sources.

The native tables add no external libraries or build-time network access and
ship inside both runtime archives. Supported target: x86_64 Linux GNU. See
[the lesson](../../tutorial/08-resources/25-text-unicode.md) and
[`std.text`](text.md) for bounded output construction and error variants.

## Acknowledgements

- Normalization uses tables and lookups from [unicode-normalization](https://github.com/unicode-rs/unicode-normalization) 0.1.25, vendored in the runtime (MIT OR Apache-2.0), with data from the [Unicode Character Database](https://www.unicode.org/ucd/) 17.0.0 (Unicode-3.0).
