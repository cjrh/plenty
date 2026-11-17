//! Identical SQLite configuration for embedded and unified Cargo runtimes.
use std::path::Path;

pub const ARCHIVE: &str = "plenty_sqlite";

pub fn build(native: &Path, out: &Path) {
    let source = native.join("sqlite-3.54.0");
    println!("cargo:rerun-if-changed={}", source.display());
    let mut build = crate::native_tools::build(out);
    build.file(source.join("sqlite3.c"));
    for (name, value) in [
        ("SQLITE_THREADSAFE", Some("1")),
        ("SQLITE_DEFAULT_MEMSTATUS", Some("0")),
        ("SQLITE_DQS", Some("0")),
        ("SQLITE_LIKE_DOESNT_MATCH_BLOBS", None),
        ("SQLITE_OMIT_DEPRECATED", None),
        ("SQLITE_ENABLE_FTS5", None),
        ("SQLITE_OMIT_SHARED_CACHE", None),
    ] {
        build.define(name, value);
    }
    build.compile(ARCHIVE);
}
