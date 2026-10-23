//! A local, content-addressed cache of compiled functions (issue #144).
//!
//! Cranelift compiles a function's *stencil*: its IR without the identities
//! of the external symbols it names. The IR refers to a symbol by an index
//! into a per-function table that is applied to the compiled code afterwards,
//! and the placing thread resolves those indices to this program's symbols
//! ([`super::codegen::symbols`]). Two programs that reach the same `std`
//! function build the same stencil, so Cranelift's machine code and
//! relocations for it can be reused: [`Cache`] stores the serialized compiled
//! stencil under a key that covers everything the output depends on.
//!
//! The key is Cranelift's: a SHA-256 of the stencil, the target triple, and
//! every ISA flag, which includes the optimization level, the register
//! allocator, and whether the verifier runs. Plenty extends it with the cache
//! format version and the compiler and Cranelift versions. Nothing else the
//! compiler does changes the code for a given stencil, so no other identity
//! is needed; the reference page explains the argument
//! (`book/src/design/toolchain/02-architecture/cache.md`).
//!
//! Entries are files named by key hash, written whole to a temporary name
//! and renamed, so concurrent compilers sharing a directory never read a
//! partial entry and need no lock. Any failure degrades to compiling without
//! the cache, with at most one warning per process.
use std::borrow::Cow;
use std::cell::Cell;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use cranelift_codegen::incremental_cache::CacheKvStore;
use sha2::{Digest, Sha256};

mod compiled;
mod program;
pub(crate) use compiled::MachineCode;
pub(crate) use program::{Lookup, ProgramKey, compiler_identity};

/// Where the compiled-function cache lives, if anywhere.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CacheMode {
    /// `PLENTY_CACHE=off` disables it; otherwise `PLENTY_CACHE_DIR`, or
    /// `$XDG_CACHE_HOME/plenty`, or `~/.cache/plenty`.
    #[default]
    Environment,
    Off,
    Directory(PathBuf),
}

impl CacheMode {
    pub(crate) fn directory(&self) -> Option<PathBuf> {
        match self {
            CacheMode::Off => None,
            CacheMode::Directory(path) => Some(path.clone()),
            CacheMode::Environment => {
                if std::env::var_os("PLENTY_CACHE").is_some_and(|v| v == "off") {
                    return None;
                }
                if let Some(dir) = std::env::var_os("PLENTY_CACHE_DIR") {
                    return Some(PathBuf::from(dir));
                }
                if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
                    return Some(PathBuf::from(xdg).join("plenty"));
                }
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|home| PathBuf::from(home).join(".cache").join("plenty"))
            }
        }
    }
}

/// Changes whenever the entry format or the meaning of the key changes.
const SCHEMA: &str = "1";
/// Compiled functions live below the cache directory here.
const ENTRIES: &str = "fn";
/// Program objects live here; see [`program`].
const PROGRAMS: &str = "program";
/// Bytes kept by default when trimming: `PLENTY_CACHE_LIMIT` overrides.
const DEFAULT_LIMIT: u64 = 1 << 30;
/// Trimming walks the whole directory, so a process that wrote entries runs
/// it only when the last one is older than this.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// A temporary file older than this belongs to a crashed compiler.
const STALE_TEMPORARY: Duration = Duration::from_secs(60 * 60);
const MAGIC: &[u8; 4] = b"PLFN";
const PROGRAM_MAGIC: &[u8; 4] = b"PLPG";
/// Magic, compile time in nanoseconds, payload length, payload SHA-256.
const HEADER: usize = 4 + 8 + 8 + 32;

/// The size limit, from `PLENTY_CACHE_LIMIT`: bytes, or a number with a
/// `K`, `M`, or `G` suffix. Unparseable values keep the default.
pub fn limit() -> u64 {
    std::env::var("PLENTY_CACHE_LIMIT")
        .ok()
        .and_then(|text| parse_limit(&text))
        .unwrap_or(DEFAULT_LIMIT)
}

fn parse_limit(text: &str) -> Option<u64> {
    let text = text.trim();
    let (digits, unit) = match text.chars().last()? {
        'K' | 'k' => (&text[..text.len() - 1], 1u64 << 10),
        'M' | 'm' => (&text[..text.len() - 1], 1 << 20),
        'G' | 'g' => (&text[..text.len() - 1], 1 << 30),
        _ => (text, 1),
    };
    digits.trim().parse::<u64>().ok()?.checked_mul(unit)
}

/// Counts for one compilation, printed by `--cache-stats`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Whether the program cache served the whole object, when this build
    /// consulted it.
    pub program: Option<bool>,
    pub hits: u64,
    pub misses: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    /// The compile time the hit entries recorded when they were written,
    /// less the time spent reading them: an estimate.
    pub saved: Duration,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("compile cache: ")?;
        match self.program {
            Some(true) => f.write_str("program hit, ")?,
            Some(false) => f.write_str("program miss, ")?,
            None => {}
        }
        write!(
            f,
            "{} hits, {} misses, {} read, {} written, about {:.0?} saved",
            self.hits,
            self.misses,
            Size(self.bytes_read),
            Size(self.bytes_written),
            self.saved
        )
    }
}

/// Bytes in a human unit.
struct Size(u64);

impl std::fmt::Display for Size {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = self.0;
        if bytes >= 1 << 30 {
            write!(f, "{:.2} GiB", bytes as f64 / (1u64 << 30) as f64)
        } else if bytes >= 1 << 20 {
            write!(f, "{:.1} MiB", bytes as f64 / (1u64 << 20) as f64)
        } else if bytes >= 1 << 10 {
            write!(f, "{:.1} KiB", bytes as f64 / 1024.0)
        } else {
            write!(f, "{bytes} B")
        }
    }
}

/// One cache directory, shared by the threads of one compilation.
pub(crate) struct Cache {
    compiled: compiled::Entries,
    dir: PathBuf,
    limit: u64,
    /// Fed to the key hash before Cranelift's key: schema and versions.
    key_prefix: Vec<u8>,
    writes_disabled: AtomicBool,
    wrote: AtomicBool,
    /// 0: the program cache was not consulted; 1: a hit; 2: a miss.
    program: std::sync::atomic::AtomicU8,
    hits: AtomicU64,
    misses: AtomicU64,
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    saved_ns: AtomicU64,
    temporaries: AtomicU64,
}

/// Printed at most once per process, whatever the cause.
static WARNED: AtomicBool = AtomicBool::new(false);

fn warn(message: &str) {
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!("warning: compile cache disabled: {message}");
    }
}

/// Everything outside the IR and the ISA that the cached code depends on.
pub(crate) fn key_prefix() -> Vec<u8> {
    format!(
        "plenty compiled-function cache {SCHEMA}\nplenty {}\ncranelift {}\n",
        env!("CARGO_PKG_VERSION"),
        cranelift_codegen::VERSION,
    )
    .into_bytes()
}

impl Cache {
    /// The cache `mode` names, or `None` when it is off or unusable.
    #[cfg_attr(feature = "profile", hotpath::measure)]
    pub(crate) fn open(mode: &CacheMode) -> Option<Arc<Cache>> {
        let dir = mode.directory()?;
        if let Err(error) = fs::create_dir_all(dir.join(ENTRIES)) {
            warn(&format!("cannot create {}: {error}", dir.display()));
            return None;
        }
        Some(Arc::new(Cache {
            compiled: compiled::Entries::default(),
            dir,
            limit: limit(),
            key_prefix: key_prefix(),
            writes_disabled: AtomicBool::new(false),
            wrote: AtomicBool::new(false),
            program: std::sync::atomic::AtomicU8::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
            saved_ns: AtomicU64::new(0),
            temporaries: AtomicU64::new(0),
        }))
    }

    /// A store for compiling one function on the current thread.
    pub(crate) fn store(&self) -> Store<'_> {
        Store {
            cache: self,
            started: Instant::now(),
            read: Cell::new(None),
        }
    }

    pub(crate) fn stats(&self) -> Stats {
        Stats {
            program: match self.program.load(Ordering::Relaxed) {
                0 => None,
                outcome => Some(outcome == 1),
            },
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            bytes_read: self.bytes_read.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            saved: Duration::from_nanos(self.saved_ns.load(Ordering::Relaxed)),
        }
    }

    /// Entries are sharded by the first byte of the hash so no directory
    /// grows to the point of slow lookups.
    fn path(&self, key: &[u8]) -> PathBuf {
        let mut hash = Sha256::new();
        hash.update(&self.key_prefix);
        hash.update(key);
        let hex = format!("{:x}", hash.finalize());
        self.dir.join(ENTRIES).join(&hex[..2]).join(hex)
    }

    #[cfg_attr(feature = "profile", hotpath::measure)]
    fn read(&self, key: &[u8]) -> Option<(Duration, Vec<u8>)> {
        read_entry(&self.path(key), MAGIC)
    }

    #[cfg_attr(feature = "profile", hotpath::measure)]
    fn write(&self, key: &[u8], compile_time: Duration, payload: &[u8]) {
        if self.writes_disabled.load(Ordering::Relaxed) {
            return;
        }
        let path = self.path(key);
        let number = self.temporaries.fetch_add(1, Ordering::Relaxed);
        if let Err(error) = write_entry(&path, MAGIC, number, compile_time, payload) {
            self.writes_disabled.store(true, Ordering::Relaxed);
            warn(&format!("cannot write {}: {error}", path.display()));
            return;
        }
        self.wrote.store(true, Ordering::Relaxed);
        self.bytes_written
            .fetch_add((HEADER + payload.len()) as u64, Ordering::Relaxed);
    }

    /// Called once a compilation is complete: trims the directory to its
    /// size limit when it is due.
    #[cfg_attr(feature = "profile", hotpath::measure)]
    pub(crate) fn finish(&self) {
        if !self.wrote.load(Ordering::Relaxed) {
            return;
        }
        let marker = self.dir.join("sweep");
        let recent = fs::metadata(&marker)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age < SWEEP_INTERVAL);
        if recent {
            return;
        }
        // Touching first keeps other compilers from sweeping at the same
        // time; a concurrent sweep is harmless, only wasted.
        let _ = fs::write(&marker, b"");
        let _ = trim(&self.dir, self.limit);
    }
}

/// [`CacheKvStore`] for one function. Cranelift asks for the entry, then
/// compiles and inserts on a miss; the store times the whole attempt so a
/// hit later can estimate what it saved.
pub(crate) struct Store<'a> {
    cache: &'a Cache,
    started: Instant,
    /// The compile time the entry read recorded, once one is read. `get`
    /// takes `&self`.
    read: Cell<Option<Duration>>,
}

impl Store<'_> {
    /// Record the outcome Cranelift reports. An entry that was read but could
    /// not be used (a Cranelift version change, say) was replaced: a miss.
    pub(crate) fn finish(self, hit: bool) {
        let elapsed = self.started.elapsed();
        if hit {
            self.cache.hits.fetch_add(1, Ordering::Relaxed);
            let saved = self.read.get().unwrap_or_default().saturating_sub(elapsed);
            self.cache
                .saved_ns
                .fetch_add(saved.as_nanos() as u64, Ordering::Relaxed);
        } else {
            self.cache.misses.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl CacheKvStore for Store<'_> {
    fn get(&self, key: &[u8]) -> Option<Cow<'_, [u8]>> {
        let (compile_time, payload) = self.cache.read(key)?;
        self.cache
            .bytes_read
            .fetch_add((HEADER + payload.len()) as u64, Ordering::Relaxed);
        self.read.set(Some(compile_time));
        Some(Cow::Owned(payload))
    }

    fn insert(&mut self, key: &[u8], val: Vec<u8>) {
        self.cache.write(key, self.started.elapsed(), &val);
    }
}

/// The entry at `path`, if it is complete and intact.
///
/// The header is read separately so the payload lands in its own buffer:
/// slicing it out of a whole-file read would copy it a second time on every
/// hit.
fn read_entry(path: &Path, magic: &[u8; 4]) -> Option<(Duration, Vec<u8>)> {
    let mut file = fs::File::open(path).ok()?;
    let mut header = [0; HEADER];
    file.read_exact(&mut header).ok()?;
    if &header[..4] != magic {
        return None;
    }
    let field = |at: usize| u64::from_le_bytes(header[at..at + 8].try_into().unwrap());
    let compile_ns = field(4);
    let length = field(12) as usize;
    // Size the buffer from the file, not the header, so a corrupt length
    // cannot request a huge allocation.
    let size = file.metadata().ok()?.len().saturating_sub(HEADER as u64);
    let mut payload = Vec::with_capacity(usize::try_from(size).ok()?);
    // Reading at most the known size skips `File::read_to_end`'s own size
    // query and its final empty read: a warm build reads an entry for nearly
    // every function (issue #315). Entries are replaced by rename, never
    // extended, so a longer file is corrupt and fails the length check.
    (&mut file).take(size).read_to_end(&mut payload).ok()?;
    if payload.len() != length || Sha256::digest(&payload).as_slice() != &header[20..] {
        return None;
    }
    Some((Duration::from_nanos(compile_ns), payload))
}

fn write_entry(
    path: &Path,
    magic: &[u8; 4],
    number: u64,
    compile_time: Duration,
    payload: &[u8],
) -> io::Result<()> {
    let shard = path.parent().unwrap();
    let name = path.file_name().unwrap().to_string_lossy();
    let temporary = shard.join(format!(".{name}.{}.{number}.tmp", std::process::id()));
    // A cold build writes an entry for nearly every function, so each costs
    // one create, one write and one rename; the shard directory is created
    // only when it is missing (issue #315).
    let mut entry = Vec::with_capacity(HEADER + payload.len());
    entry.extend_from_slice(magic);
    entry.extend_from_slice(&(compile_time.as_nanos() as u64).to_le_bytes());
    entry.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    entry.extend_from_slice(&Sha256::digest(payload));
    entry.extend_from_slice(payload);
    let written = (|| {
        let mut file = match fs::File::create(&temporary) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir_all(shard)?;
                fs::File::create(&temporary)?
            }
            file => file?,
        };
        file.write_all(&entry)?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// An entry on disk, for trimming and statistics.
struct Entry {
    path: PathBuf,
    bytes: u64,
    /// The later of the access and modification times. Linux updates access
    /// times lazily (`relatime`), so this is coarse but needs no write on a hit.
    used: SystemTime,
}

fn entries(dir: &Path) -> Vec<Entry> {
    let mut found = Vec::new();
    let shards = [ENTRIES, PROGRAMS]
        .into_iter()
        .filter_map(|kind| fs::read_dir(dir.join(kind)).ok())
        .flatten();
    for shard in shards.flatten() {
        let Ok(files) = fs::read_dir(shard.path()) else {
            continue;
        };
        for file in files.flatten() {
            let Ok(metadata) = file.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let path = file.path();
            if path.extension().is_some_and(|e| e == "tmp") {
                if modified.elapsed().is_ok_and(|age| age > STALE_TEMPORARY) {
                    let _ = fs::remove_file(&path);
                }
                continue;
            }
            let used = metadata.accessed().map_or(modified, |a| a.max(modified));
            found.push(Entry {
                path,
                bytes: metadata.len(),
                used,
            });
        }
    }
    found
}

/// Remove the least recently used entries until the directory holds at most
/// `limit` bytes.
fn trim(dir: &Path, limit: u64) -> io::Result<(u64, u64)> {
    let mut entries = entries(dir);
    let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
    entries.sort_by_key(|e| e.used);
    let (mut removed, mut freed) = (0, 0);
    for entry in entries {
        if total <= limit {
            break;
        }
        if fs::remove_file(&entry.path).is_ok() {
            total -= entry.bytes;
            removed += 1;
            freed += entry.bytes;
        }
    }
    Ok((removed, freed))
}

/// What `plenty cache stats` reports.
#[derive(Debug)]
pub struct Summary {
    pub directory: PathBuf,
    pub entries: u64,
    pub bytes: u64,
    pub limit: u64,
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "directory: {}", self.directory.display())?;
        writeln!(f, "entries:   {}", self.entries)?;
        writeln!(f, "size:      {}", Size(self.bytes))?;
        write!(f, "limit:     {}", Size(self.limit))
    }
}

/// The cache `mode` names, or `None` when it is off.
pub fn summary(mode: &CacheMode) -> Option<Summary> {
    let directory = mode.directory()?;
    let entries = entries(&directory);
    Some(Summary {
        entries: entries.len() as u64,
        bytes: entries.iter().map(|e| e.bytes).sum(),
        directory,
        limit: limit(),
    })
}

/// Remove every entry. Returns the count and bytes removed, or `None` when
/// the cache is off.
pub fn clean(mode: &CacheMode) -> io::Result<Option<(u64, u64)>> {
    let Some(directory) = mode.directory() else {
        return Ok(None);
    };
    if !directory.join(ENTRIES).exists() && !directory.join(PROGRAMS).exists() {
        return Ok(Some((0, 0)));
    }
    trim(&directory, 0).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(dir: &Path) -> Arc<Cache> {
        Cache::open(&CacheMode::Directory(dir.to_path_buf())).unwrap()
    }

    #[test]
    fn limits_parse_with_units() {
        assert_eq!(parse_limit("1024"), Some(1024));
        assert_eq!(parse_limit("4K"), Some(4096));
        assert_eq!(parse_limit("2M"), Some(2 << 20));
        assert_eq!(parse_limit(" 1g "), Some(1 << 30));
        assert_eq!(parse_limit("many"), None);
        assert_eq!(parse_limit(""), None);
    }

    #[test]
    fn entries_round_trip_and_record_compile_time() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let key = [7u8; 32];
        assert!(cache.read(&key).is_none());
        cache.write(&key, Duration::from_millis(3), b"code");
        assert_eq!(
            cache.read(&key),
            Some((Duration::from_millis(3), b"code".to_vec()))
        );
        assert_eq!(cache.stats().bytes_written, (HEADER + 4) as u64);
    }

    /// Any damage to an entry makes it a miss, never an error.
    #[test]
    fn damaged_entries_are_misses() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let key = [1u8; 32];
        cache.write(&key, Duration::ZERO, b"machine code");
        let path = cache.path(&key);
        let good = fs::read(&path).unwrap();

        fs::write(&path, &good[..good.len() - 1]).unwrap();
        assert!(cache.read(&key).is_none(), "truncated");
        let mut flipped = good.clone();
        *flipped.last_mut().unwrap() ^= 1;
        fs::write(&path, &flipped).unwrap();
        assert!(cache.read(&key).is_none(), "corrupt payload");
        let mut header = good.clone();
        header[0] = b'x';
        fs::write(&path, &header).unwrap();
        assert!(cache.read(&key).is_none(), "wrong magic");
        fs::write(&path, b"").unwrap();
        assert!(cache.read(&key).is_none(), "empty");
        fs::write(&path, &good[..HEADER]).unwrap();
        assert!(cache.read(&key).is_none(), "header only");
        let mut extended = good.clone();
        extended.push(0);
        fs::write(&path, &extended).unwrap();
        assert!(cache.read(&key).is_none(), "trailing byte");

        // A later write replaces the damaged entry.
        cache.write(&key, Duration::ZERO, b"machine code");
        assert!(cache.read(&key).is_some());
    }

    /// Every input outside the IR and ISA changes the entry's name: the
    /// schema and the versions of Plenty and Cranelift.
    #[test]
    fn key_prefix_names_schema_and_versions() {
        let prefix = String::from_utf8(key_prefix()).unwrap();
        assert!(prefix.contains(&format!("cache {SCHEMA}\n")));
        assert!(prefix.contains(&format!("plenty {}\n", env!("CARGO_PKG_VERSION"))));
        assert!(prefix.contains(&format!("cranelift {}\n", cranelift_codegen::VERSION)));

        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let key = [9u8; 32];
        let path = cache.path(&key);
        let mut other = Cache::open(&CacheMode::Directory(dir.path().to_path_buf())).unwrap();
        Arc::get_mut(&mut other).unwrap().key_prefix.push(b'!');
        assert_ne!(other.path(&key), path);
        assert_ne!(cache.path(&[8u8; 32]), path);
    }

    #[test]
    fn trimming_removes_the_least_recently_used_entries_first() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        for i in 0..4u8 {
            cache.write(&[i; 32], Duration::ZERO, &[i; 100]);
            // Distinct modification times, in key order.
            let path = cache.path(&[i; 32]);
            let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + u64::from(i));
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_accessed(when).set_modified(when))
                .unwrap();
        }
        let entry = (HEADER + 100) as u64;
        let (removed, freed) = trim(dir.path(), 2 * entry).unwrap();
        assert_eq!((removed, freed), (2, 2 * entry));
        assert!(cache.read(&[0; 32]).is_none());
        assert!(cache.read(&[1; 32]).is_none());
        assert!(cache.read(&[2; 32]).is_some());
        assert!(cache.read(&[3; 32]).is_some());

        let summary = summary(&CacheMode::Directory(dir.path().to_path_buf())).unwrap();
        assert_eq!((summary.entries, summary.bytes), (2, 2 * entry));
        assert_eq!(
            clean(&CacheMode::Directory(dir.path().to_path_buf())).unwrap(),
            Some((2, 2 * entry))
        );
        assert_eq!(clean(&CacheMode::Off).unwrap(), None);
    }

    /// An unwritable directory keeps reads working and warns once.
    #[cfg(unix)]
    #[test]
    fn unwritable_directory_disables_writes_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        cache.write(&[1; 32], Duration::ZERO, b"kept");
        let entries = dir.path().join(ENTRIES);
        fs::set_permissions(&entries, fs::Permissions::from_mode(0o555)).unwrap();
        let shard = cache.path(&[1; 32]).parent().unwrap().to_path_buf();
        fs::set_permissions(&shard, fs::Permissions::from_mode(0o555)).unwrap();
        let readable = cache.read(&[1; 32]).is_some();
        cache.write(&[1; 32], Duration::ZERO, b"replaced");
        cache.write(&[2; 32], Duration::ZERO, b"new");
        let disabled = cache.writes_disabled.load(Ordering::Relaxed);
        let unchanged = cache.read(&[1; 32]).map(|(_, p)| p);
        fs::set_permissions(&shard, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&entries, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(readable);
        // Root can write anywhere; the test is only meaningful otherwise.
        if unchanged.as_deref() == Some(b"kept") {
            assert!(disabled);
        }
    }
}
