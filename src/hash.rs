//! Hash tables for the compiler's own lookups.
//!
//! Most keys are names from the program being compiled: identifiers, type
//! names and qualified paths. Std's default SipHash resists collision flooding
//! from untrusted keys, which a compiler reading its own input does not need;
//! replacing it removed about 11% of checking instructions (issue #302).
//! FxHash is what rustc uses for the same kind of table. Iteration order stays
//! unspecified, as it was with SipHash's per-process random keys.

pub(crate) type HashMap<K, V> = std::collections::HashMap<K, V, rustc_hash::FxBuildHasher>;
pub(crate) type HashSet<T> = std::collections::HashSet<T, rustc_hash::FxBuildHasher>;
