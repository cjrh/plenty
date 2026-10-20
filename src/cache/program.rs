//! Whole-program objects (issue #317): a build whose sources, compiler and
//! settings match a stored entry writes the stored object instead of running
//! the frontend and code generation.
//!
//! An entry is named by a [`ProgramKey`]: the compiler's identity and the
//! build's request (its entry file and package graph, or its source text, and
//! the code generation settings), but not the contents of the files it
//! imports, which a build discovers only by parsing. The entry instead
//! records those [`Inputs`], and a lookup uses it only while they are
//! current. A new build of the same request replaces the entry, so each
//! program keeps one object, its latest.
//!
//! The first build of a request stores only an empty entry, a marker that
//! it was built: an object is stored once a request is built again. Most
//! programs built once, such as a test suite's temporary programs, are never
//! built again, and their objects would fill the cache.
use super::{read_entry, write_entry, Cache, PROGRAMS, PROGRAM_MAGIC};
use crate::frontend::Inputs;
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Changes whenever the entry format or the meaning of the key changes.
const SCHEMA: &str = "1";

/// The compiler binary's path, size and modification time. The frontend's
/// output depends on the compiler's whole code, not only on its version, and
/// hashing an 80 MB binary would cost more than a warm build saves; a rebuilt
/// or replaced compiler has a new modification time. The standard library
/// and the runtime are embedded, so this covers them too; the key also
/// hashes the standard library's text.
pub(crate) fn compiler_identity() -> &'static str {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        let Ok(path) = std::env::current_exe() else {
            return String::new();
        };
        let metadata = std::fs::metadata(&path).ok();
        format!(
            "{} {:?} {:?}",
            path.display(),
            metadata.as_ref().map(|m| m.len()),
            metadata.and_then(|m| m.modified().ok())
        )
    })
}

/// The name of one program's entry.
pub(crate) struct ProgramKey(String);

impl ProgramKey {
    /// `request` describes the sources canonically; `settings` every code
    /// generation choice that changes the object.
    pub(crate) fn new(request: &[u8], settings: &str) -> ProgramKey {
        let mut hash = Sha256::new();
        let mut field = |bytes: &[u8]| {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        };
        field(format!("plenty program cache {SCHEMA}").as_bytes());
        field(env!("CARGO_PKG_VERSION").as_bytes());
        field(cranelift_codegen::VERSION.as_bytes());
        field(compiler_identity().as_bytes());
        field(crate::frontend::standard_digest());
        field(crate::native_target().as_bytes());
        field(settings.as_bytes());
        field(request);
        ProgramKey(format!("{:x}", hash.finalize()))
    }

    fn path(&self, dir: &Path) -> PathBuf {
        dir.join(PROGRAMS).join(&self.0[..2]).join(&self.0)
    }
}

impl Cache {
    /// Write the stored object for `key` to `object` if its inputs are still
    /// `current`, and report whether it did. Anything unusable is a miss.
    #[cfg_attr(feature = "profile", hotpath::measure)]
    pub(crate) fn load_program(
        &self,
        key: &ProgramKey,
        current: impl FnOnce(&Inputs) -> bool,
        object: &Path,
    ) -> io::Result<Lookup> {
        let started = Instant::now();
        let path = key.path(&self.dir);
        let entry = read_entry(&path, PROGRAM_MAGIC);
        let seen = entry.is_some() || path.exists();
        let found = entry.and_then(|(time, payload)| {
            let (inputs, rest) = split(&payload)?;
            let inputs = Inputs::decode(inputs)?;
            let start = payload.len() - rest.len();
            current(&inputs).then_some((time, payload, start))
        });
        let Some((compile_time, payload, start)) = found else {
            self.program.store(2, Ordering::Relaxed);
            return Ok(Lookup::Miss { seen });
        };
        std::fs::write(object, &payload[start..])?;
        self.program.store(1, Ordering::Relaxed);
        self.bytes_read
            .fetch_add((super::HEADER + payload.len()) as u64, Ordering::Relaxed);
        let saved = compile_time.saturating_sub(started.elapsed());
        self.saved_ns
            .fetch_add(saved.as_nanos() as u64, Ordering::Relaxed);
        Ok(Lookup::Hit)
    }

    /// Record `object`, built from `inputs` in `compile_time`, as `key`'s
    /// entry, replacing any older one; or, for a request not `seen` before,
    /// only an empty entry. Failing to is not an error.
    #[cfg_attr(feature = "profile", hotpath::measure)]
    pub(crate) fn store_program(
        &self,
        key: &ProgramKey,
        inputs: &Inputs,
        object: &Path,
        compile_time: Duration,
        seen: bool,
    ) {
        if self.writes_disabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(inputs) = inputs.encode() else {
            return;
        };
        let mut payload = Vec::new();
        if seen {
            let Ok(code) = std::fs::read(object) else {
                return;
            };
            payload.reserve_exact(8 + inputs.len() + code.len());
            payload.extend_from_slice(&(inputs.len() as u64).to_le_bytes());
            payload.extend_from_slice(&inputs);
            payload.extend_from_slice(&code);
        }
        let path = key.path(&self.dir);
        let number = self.temporaries.fetch_add(1, Ordering::Relaxed);
        // Quietly: a read-only directory that served every function would
        // otherwise warn on every build, and a failed function write has
        // already warned.
        if write_entry(&path, PROGRAM_MAGIC, number, compile_time, &payload).is_err() {
            return;
        }
        self.wrote.store(true, Ordering::Relaxed);
        self.bytes_written
            .fetch_add((super::HEADER + payload.len()) as u64, Ordering::Relaxed);
        self.finish();
    }
}

/// What a lookup found.
pub(crate) enum Lookup {
    Hit,
    /// `seen`: the request has an entry, perhaps only a marker or stale.
    Miss {
        seen: bool,
    },
}

/// The encoded inputs and the object after them; `None` for a marker.
fn split(payload: &[u8]) -> Option<(&[u8], &[u8])> {
    let length = usize::try_from(u64::from_le_bytes(payload.get(..8)?.try_into().ok()?)).ok()?;
    let rest = &payload[8..];
    (length <= rest.len()).then(|| rest.split_at(length))
}
