//! Linked outputs for single-file runs. Linker depfiles record the native
//! inputs; file identities and search-directory identities invalidate them.
//! Only a default Linux GCC driver is supported: wrappers, Clang's implicit
//! configuration files and explicit arguments can read untracked inputs.
//! The selected linker must support depfiles.
use super::{Cache, EXECUTABLES, compiler_identity, read_entry, write_entry};
use crate::CompileOptions;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

const MAGIC: &[u8; 4] = b"PLEX";
type Manifest = Vec<(String, String)>;

pub(crate) struct Executable {
    cache: Arc<Cache>,
    path: PathBuf,
    dependencies: PathBuf,
    temporary_inputs: [PathBuf; 2],
    watched: Vec<PathBuf>,
    started: Instant,
    linker: PathBuf,
}

impl Executable {
    pub(crate) fn new(
        options: &CompileOptions,
        command: &Command,
        object: &Path,
        runtime: &Path,
        engines: &[OsString],
    ) -> Option<Self> {
        if !cfg!(target_os = "linux")
            || !options.executable_cache
            || !options.program_cache
            || options.distribution
            || options.fuzz.is_some()
            || options.test_hooks
            || !options.link_args.is_empty()
            || options.linker != Path::new("cc")
            || !options.auto_linker
        {
            return None;
        }
        // These can redirect the driver's tools or inject arbitrary behavior.
        if [
            "COMPILER_PATH",
            "GCC_EXEC_PREFIX",
            "LIBRARY_PATH",
            "LD_PRELOAD",
            "LD_AUDIT",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
        {
            return None;
        }
        let linker = options.automatic_linker();
        let cache = Cache::open(&options.cache)?;
        let path = std::env::var_os("PATH")?;
        let find = |name: &str| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(name))
                .find(|file| {
                    file.metadata().is_ok_and(|m| {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            m.is_file() && m.permissions().mode() & 0o111 != 0
                        }
                        #[cfg(not(unix))]
                        {
                            m.is_file()
                        }
                    })
                })
        };
        let driver = find("cc")?;
        if !gcc_driver(&driver) {
            return None;
        }
        // Ask GCC, not just PATH: collect2 can select a private linker first.
        let tool = linker.map_or_else(|| "ld".to_owned(), |name| format!("ld.{name}"));
        let output = Command::new(&driver)
            .arg(format!("-print-prog-name={tool}"))
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let name = std::str::from_utf8(&output.stdout).ok()?.trim();
        let selected = if Path::new(name).is_absolute() {
            PathBuf::from(name)
        } else {
            find(name)?
        };
        // Eligibility must precede load(), not only the depfile-capability
        // probe on a miss. Wrapper configuration is not a linker dependency.
        if !native_elf(&selected) {
            return None;
        }
        let mut hash = Sha256::new();
        let mut field = |bytes: &[u8]| {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        };
        field(b"plenty executable cache 2");
        field(compiler_identity().as_bytes()); // Includes the embedded runtime.
        field(&std::fs::read(object).ok()?);
        field(identity(&driver)?.as_bytes());
        field(identity(&selected)?.as_bytes());
        field(std::env::current_dir().ok()?.as_os_str().as_encoded_bytes());
        // Conservatively include the entire environment, not a guessed subset
        // of driver variables. Nothing here captures the program's output.
        let mut environment: Vec<_> = std::env::vars_os().collect();
        environment.sort();
        for (name, value) in environment {
            field(name.as_encoded_bytes());
            field(value.as_encoded_bytes());
        }
        let mut skip_output = false;
        for arg in command.get_args() {
            if skip_output {
                skip_output = false;
                continue;
            }
            if arg == "-o" {
                skip_output = true;
                continue;
            }
            if arg == object.as_os_str() || arg == runtime.as_os_str() {
                continue;
            }
            field(arg.as_encoded_bytes());
        }
        // Track absent directories too: creating a previously absent search
        // directory or adding a higher-priority library must invalidate a hit.
        let search = Command::new(&driver)
            .arg("-print-search-dirs")
            .output()
            .ok()?;
        if !search.status.success() {
            return None;
        }
        let search = std::str::from_utf8(&search.stdout).ok()?;
        let mut watched = vec![driver, selected.clone(), std::env::current_dir().ok()?];
        for line in search.lines() {
            if let Some((kind, dirs)) = line.split_once(": =")
                && (kind == "libraries" || kind == "programs")
            {
                for dir in std::env::split_paths(dirs) {
                    // GCC can find a linker in its private program directories
                    // before PATH; watching only PATH would miss those updates.
                    let tools = private_tools(&dir)?;
                    watched.extend(tools);
                    watched.push(dir.join("specs"));
                    watched.push(dir);
                }
            }
        }
        if !search.lines().any(|line| line.starts_with("libraries: =")) {
            return None;
        }
        watched.extend(
            ["/lib", "/lib64", "/usr/lib", "/usr/lib64", "/usr/local/lib"].map(PathBuf::from),
        );
        watched.extend(engines.iter().filter_map(|arg| {
            let file = Path::new(arg);
            file.is_absolute().then(|| file.to_path_buf())
        }));
        watched.sort();
        watched.dedup();
        let key = format!("{:x}", hash.finalize());
        Some(Self {
            path: cache.dir.join(EXECUTABLES).join(&key[..2]).join(key),
            cache,
            dependencies: object.with_extension("link.d"),
            temporary_inputs: [object.to_path_buf(), runtime.to_path_buf()],
            watched,
            started: Instant::now(),
            linker: selected,
        })
    }

    /// Probe only on a miss. Old or unknown linkers still run normally.
    pub(crate) fn supported(&self) -> bool {
        Command::new(&self.linker)
            .arg("--help")
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("--dependency-file")
            })
    }

    pub(crate) fn dependencies(&self) -> &Path {
        &self.dependencies
    }

    pub(crate) fn load(&self, output: &Path) -> bool {
        let Some((_, payload)) = read_entry(&self.path, MAGIC) else {
            return false;
        };
        let Some((manifest, executable)) = decode(&payload) else {
            return false;
        };
        if executable.is_empty()
            || manifest.is_empty()
            || !manifest
                .iter()
                .all(|(path, before)| snapshot(Path::new(path)).as_ref() == Some(before))
        {
            return false;
        }
        // Materialize privately: eviction, concurrent publication and a cache
        // mounted noexec cannot race execution or mutate a running file.
        std::fs::write(output, executable)
            .and_then(|()| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(output, std::fs::Permissions::from_mode(0o700))
                }
                #[cfg(not(unix))]
                {
                    Ok(())
                }
            })
            .is_ok()
    }

    pub(crate) fn store(&self, output: &Path) {
        let _ = self.try_store(output);
    }

    fn try_store(&self, output: &Path) -> Option<()> {
        let text = std::fs::read_to_string(&self.dependencies).ok()?;
        let mut files = dependencies(&text)?;
        files.retain(|file| !self.temporary_inputs.contains(file));
        if files.is_empty() {
            return None;
        }
        // A missing recorded dependency is never a usable cache entry.
        for file in &files {
            identity(file)?;
        }
        let parents: Vec<_> = files
            .iter()
            .filter_map(|file| file.parent().map(Path::to_path_buf))
            .collect();
        files.extend(parents);
        files.extend(self.watched.iter().cloned());
        files.sort();
        files.dedup();
        let manifest: Manifest = files
            .iter()
            .map(|file| Some((file.to_str()?.to_owned(), snapshot(file)?)))
            .collect::<Option<_>>()?;
        let manifest = serde_json::to_vec(&manifest).ok()?;
        let code = std::fs::read(output).ok()?;
        let mut payload = Vec::with_capacity(8 + manifest.len() + code.len());
        payload.extend_from_slice(&(manifest.len() as u64).to_le_bytes());
        payload.extend_from_slice(&manifest);
        payload.extend_from_slice(&code);
        let number = self.cache.temporaries.fetch_add(1, Ordering::Relaxed);
        write_entry(&self.path, MAGIC, number, self.started.elapsed(), &payload).ok()?;
        self.cache.wrote.store(true, Ordering::Relaxed);
        self.cache.finish();
        Some(())
    }
}

fn gcc_driver(path: &Path) -> bool {
    let Some(name) = path.canonicalize().ok().and_then(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    }) else {
        return false;
    };
    if !name.split('-').any(|part| part == "gcc") {
        return false;
    }
    native_elf(path)
}

/// Private linker overrides and collect2 can themselves be wrappers. Check
/// every candidate before cache lookup; retain absent paths to notice additions.
fn private_tools(directory: &Path) -> Option<Vec<PathBuf>> {
    let mut tools = Vec::new();
    for name in [
        "collect2",
        "ld",
        "ld.mold",
        "ld.lld",
        "ld.bfd",
        "ld.gold",
        "real-ld",
        "collect-ld",
    ] {
        let file = directory.join(name);
        match file.try_exists() {
            Ok(true) if !native_elf(&file) => return None,
            Err(_) => return None,
            _ => tools.push(file),
        }
    }
    Some(tools)
}

fn native_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok()
        && magic == *b"\x7fELF"
}

fn decode(payload: &[u8]) -> Option<(Manifest, &[u8])> {
    let length = usize::try_from(u64::from_le_bytes(payload.get(..8)?.try_into().ok()?)).ok()?;
    let end = 8usize.checked_add(length)?;
    Some((
        serde_json::from_slice(payload.get(8..end)?).ok()?,
        payload.get(end..)?,
    ))
}

#[cfg(unix)]
fn identity(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let m = path.metadata().ok()?;
    Some(format!(
        "{:?} {} {} {} {} {} {} {} {}",
        path.canonicalize().ok()?,
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
        m.mode()
    ))
}

#[cfg(not(unix))]
fn identity(path: &Path) -> Option<String> {
    let metadata = path.metadata().ok()?;
    Some(format!(
        "{:?} {} {:?}",
        path.canonicalize().ok()?,
        metadata.len(),
        metadata.modified().ok()?
    ))
}

fn snapshot(path: &Path) -> Option<String> {
    match std::fs::metadata(path) {
        Ok(_) => identity(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some("absent".into()),
        Err(_) => None,
    }
}

/// Read just the first make rule; subsequent rules are empty phony targets.
/// Refuse ambiguous/non-UTF-8 syntax rather than store incomplete inputs.
fn dependencies(text: &str) -> Option<Vec<PathBuf>> {
    let text = text.replace("\\\n", "");
    let rule = text.lines().next()?;
    let (_, words) = rule.split_once(": ")?;
    let mut paths = Vec::new();
    let mut word = String::new();
    let mut chars = words.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => word.push(chars.next()?),
            '$' => {
                if chars.next()? != '$' {
                    return None;
                }
                word.push('$');
            }
            '#' => return None,
            c if c.is_whitespace() => {
                if !word.is_empty() {
                    paths.push(PathBuf::from(std::mem::take(&mut word)));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        paths.push(PathBuf::from(word));
    }
    (!paths.is_empty()).then_some(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_dependencies_preserve_escaped_paths() {
        assert_eq!(
            dependencies("out: /a\\ b/lib.a \\\n /lib/libc.so /a$$b/lib.a\n/lib/libc.so:\n"),
            Some(vec![
                "/a b/lib.a".into(),
                "/lib/libc.so".into(),
                "/a$b/lib.a".into()
            ])
        );
        assert!(dependencies("out: /unescaped#comment").is_none());
        assert!(dependencies("out: /trailing\\").is_none());
        assert!(decode(&[0; 7]).is_none());
        assert!(decode(&u64::MAX.to_le_bytes()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn executable_entries_validate_native_files_search_directories_and_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(&super::super::CacheMode::Directory(
            dir.path().join("cache"),
        ))
        .unwrap();
        let object = dir.path().join("program.o");
        let runtime = dir.path().join("runtime.a");
        let native = dir.path().join("libnative.a");
        let alias = dir.path().join("alias.a");
        let other = dir.path().join("other.a");
        let output = dir.path().join("program");
        let restored = dir.path().join("restored");
        let search = dir.path().join("search");
        std::fs::create_dir(&search).unwrap();
        std::fs::write(&native, b"native code").unwrap();
        std::fs::write(&other, b"other code").unwrap();
        symlink(&native, &alias).unwrap();
        std::fs::write(&output, b"executable bytes").unwrap();
        let exe = Executable {
            path: cache.dir.join(EXECUTABLES).join("00").join("entry"),
            cache,
            dependencies: dir.path().join("link.d"),
            temporary_inputs: [object, runtime],
            watched: vec![search.clone()],
            started: Instant::now(),
            linker: PathBuf::new(),
        };
        std::fs::write(&exe.dependencies, format!("out: {}\n", alias.display())).unwrap();
        exe.store(&output);
        assert!(exe.load(&restored));
        assert_eq!(std::fs::read(&restored).unwrap(), b"executable bytes");
        assert_eq!(
            restored.metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
        exe.store(&output);
        std::fs::write(search.join("libnew.a"), b"new").unwrap();
        assert!(!exe.load(&restored));
        exe.store(&output);
        std::fs::write(&native, b"changed").unwrap();
        assert!(!exe.load(&restored));
        exe.store(&output);
        std::fs::remove_file(&alias).unwrap();
        symlink(&other, &alias).unwrap();
        assert!(!exe.load(&restored));
        exe.store(&output);
        std::fs::remove_file(&other).unwrap();
        assert!(!exe.load(&restored));
    }

    #[test]
    fn untracked_driver_configurations_and_wrappers_bypass_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let gcc = dir.path().join("gcc");
        let clang = dir.path().join("clang");
        std::fs::write(&gcc, b"#!/bin/sh\nexec /usr/bin/gcc \"$@\"\n").unwrap();
        assert!(!gcc_driver(&gcc));
        std::fs::write(&clang, b"\x7fELF").unwrap();
        assert!(!gcc_driver(&clang));
        std::fs::write(&gcc, b"\x7fELF").unwrap();
        assert!(gcc_driver(&gcc));
    }

    #[test]
    fn private_linker_and_collect2_wrappers_are_ineligible_before_lookup() {
        let dir = tempfile::tempdir().unwrap();
        assert!(private_tools(dir.path()).is_some());
        for name in [
            "ld",
            "ld.mold",
            "ld.lld",
            "real-ld",
            "collect-ld",
            "collect2",
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"#!/bin/sh\nexec /bin/ld \"$@\"\n").unwrap();
            assert!(!native_elf(&path));
            assert!(
                private_tools(dir.path()).is_none(),
                "accepted {name} wrapper"
            );
            std::fs::remove_file(&path).unwrap();
        }
        let path = dir.path().join("ld.mold");
        std::fs::write(&path, b"\x7fELF").unwrap();
        assert!(private_tools(dir.path()).is_some());
    }

    #[test]
    fn identities_notice_replacement_and_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("input");
        let absent = snapshot(&file).unwrap();
        std::fs::write(&file, "a").unwrap();
        let before = snapshot(&file).unwrap();
        assert_ne!(absent, before);
        std::fs::write(&file, "b").unwrap();
        assert_ne!(before, snapshot(&file).unwrap());
    }
}
