//! File-kind recognition shared by module loading and package/editor tooling.
use std::path::{Path, PathBuf};

pub(crate) const EXTENSIONS: [&str; 4] = ["plenty", "p", "plentyi", "pi"];

pub(crate) fn is_source(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "plenty" || e == "p")
}

pub(crate) fn is_interface(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e == "plentyi" || e == "pi")
}

pub(crate) fn is_module(path: &Path) -> bool {
    is_source(path) || is_interface(path)
}

/// All existing spellings, in a stable order for ambiguity diagnostics.
pub(crate) fn candidates(base: &Path) -> Vec<PathBuf> {
    EXTENSIONS
        .iter()
        .map(|e| base.with_extension(e))
        .filter(|p| p.exists())
        .collect()
}
