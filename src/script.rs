//! Native single-file applications with optional embedded package metadata.
//! Resolution is independent of neighboring manifests; generated state is private
//! and keyed by the physical script path. Applications still require `main`.
use crate::project::{self, Project, ResolveOptions};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::ffi::OsString;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const USAGE: &str = "Usage: plenty script [OPTIONS] [--] FILE [ARGS...]\n\
Options precede FILE; everything after FILE is passed unchanged to the program.\n\
  --check              Type-check without running\n\
  --compile -o OUT     Write a native executable without running\n\
  --fast-compile       Skip optimization for faster compilation\n\
  --unchecked MODE     `trap` (default) or `assume` for plain `unchecked:` blocks\n\
  --offline            Use only cached dependency sources\n\
  --locked             Refuse to create or change the dependency lock\n\
  --lockfile PATH      Use a durable lock (relative to the caller)\n\
  --verbose            Report linker commands\n\
  --no-cache           Disable compiled artifact reuse\n\
  --cache-stats        Report compiled artifact reuse\n\
  --no-strip           Keep executable symbols\n\
  -h, --help           Show this help\n";

pub(crate) struct Header {
    pub(crate) manifest: Option<String>,
    pub(crate) source: Option<String>,
}

/// Blank metadata bytes rather than deleting them: both scalar and byte source
/// locations after the header remain unchanged, including with CRLF input.
pub(crate) fn header(text: &str, name: &str) -> Result<Header> {
    if !text.contains("---") {
        return Ok(Header {
            manifest: None,
            source: None,
        });
    }
    let mut manifest = None;
    let mut open = None;
    let mut begin = 0;
    let mut body = false;
    let mut offset = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut masked = text.as_bytes().to_vec();
    for (index, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.trim_end_matches(['\r', '\n']);
        let marker = quote.is_none() && (line.trim() == "---plenty" || line.trim() == "---");
        if marker {
            if line != line.trim() {
                return Err(format!(
                    "{name}:{}: script header delimiters must be standalone, unindented lines",
                    index + 1
                )
                .into());
            }
            if line == "---plenty" {
                if open.is_some() || manifest.is_some() || body {
                    return Err(format!(
                        "{name}:{}: duplicate or misplaced script manifest",
                        index + 1
                    )
                    .into());
                }
                open = Some(index + 1);
                begin = offset + raw.len();
            } else if open.take().is_some() {
                let mut padded = "\n".repeat(text[..begin].bytes().filter(|&b| b == b'\n').count());
                padded.push_str(&text[begin..offset]);
                manifest = Some(padded);
            } else {
                return Err(format!(
                    "{name}:{}: script manifest closer without ---plenty",
                    index + 1
                )
                .into());
            }
        } else if open.is_none() {
            if !(line.trim().is_empty() || (index == 0 && line.starts_with("#!"))) {
                body = true;
            }
            // Delimiter-looking lines inside Plenty string literals are text.
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                if escaped {
                    escaped = false;
                    continue;
                }
                if quote.is_some() && c == '\\' {
                    escaped = true;
                    continue;
                }
                let triple = || {
                    let mut rest = chars.clone();
                    rest.next() == Some(c) && rest.next() == Some(c)
                };
                match quote {
                    Some((q, true)) if q == c && triple() => {
                        chars.next();
                        chars.next();
                        quote = None;
                    }
                    Some((q, false)) if q == c => quote = None,
                    None if c == '#' => break,
                    None if c == '"' || c == '\'' => {
                        let long = triple();
                        if long {
                            chars.next();
                            chars.next();
                        }
                        quote = Some((c, long));
                    }
                    _ => {}
                }
            }
            escaped = false;
        }
        if marker || open.is_some() {
            for byte in &mut masked[offset..offset + raw.len()] {
                if !matches!(*byte, b'\r' | b'\n') {
                    *byte = b' ';
                }
            }
        }
        offset += raw.len();
    }
    if let Some(line) = open {
        return Err(format!("{name}:{line}: unterminated script manifest; expected ---").into());
    }
    let source = if manifest.is_some() {
        Some(String::from_utf8(masked)?)
    } else {
        None
    };
    Ok(Header { manifest, source })
}

#[derive(Default)]
struct Options {
    check: bool,
    compile: bool,
    output: Option<PathBuf>,
    lockfile: Option<PathBuf>,
    resolve: ResolveOptions,
    build: crate::CompileOptions,
}

/// Run the script CLI with native, lossless OS arguments.
pub fn run(args: Vec<OsString>) -> Result<ExitCode> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    let file = loop {
        let Some(arg) = args.next() else {
            return Err(USAGE.into());
        };
        if arg == "--" {
            break args.next().ok_or("-- requires a script filename")?;
        }
        let Some(flag) = arg.to_str() else {
            break arg;
        };
        match flag {
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(ExitCode::SUCCESS);
            }
            "--check" => options.check = true,
            "--compile" => options.compile = true,
            "--release" => {}
            "--fast-compile" => options.build.optimize = false,
            "--unchecked" => {
                let mode = args
                    .next()
                    .ok_or("--unchecked requires `trap` or `assume`")?;
                let mode = mode
                    .to_str()
                    .ok_or("--unchecked takes `trap` or `assume`")?;
                options.build.unchecked = crate::UncheckedMode::parse(mode)?;
            }
            "--offline" => options.resolve.offline = true,
            "--locked" => options.resolve.locked = true,
            "--verbose" => options.build.verbose = true,
            "--no-strip" => options.build.strip = false,
            "--cache-stats" => options.build.cache_stats = true,
            "--no-cache" => options.build.cache = crate::cache::CacheMode::Off,
            "-o" | "--output" | "--lockfile" => {
                let value = PathBuf::from(
                    args.next()
                        .ok_or_else(|| format!("{flag} requires a path"))?,
                );
                let slot = if flag == "--lockfile" {
                    &mut options.lockfile
                } else {
                    &mut options.output
                };
                if slot.replace(value).is_some() {
                    return Err(format!("{flag} may be specified only once").into());
                }
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown script option {flag:?}\n{USAGE}").into());
            }
            _ => break arg,
        }
    };
    let arguments: Vec<_> = args.collect();
    if options.check && options.compile {
        return Err("--check and --compile are mutually exclusive".into());
    }
    if options.compile != options.output.is_some() {
        return Err("--compile requires -o OUT; -o requires --compile".into());
    }
    if (options.check || options.compile) && !arguments.is_empty() {
        return Err("--check and --compile reject program arguments".into());
    }
    let path = PathBuf::from(&file)
        .canonicalize()
        .map_err(|e| format!("reading {}: {e}", Path::new(&file).display()))?;
    let text = fs::read_to_string(&path)?;
    let header = header(&text, &path.display().to_string())?;
    let manifest = project::manifest::parse_script(
        header.manifest.as_deref().unwrap_or(""),
        &path.display().to_string(),
        &path,
    )?;
    let directory = path.parent().unwrap();
    // Even with artifact reuse disabled, dependency pins/sources need storage.
    let cache = storage_root()?;
    let key = format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes()));
    let scripts = cache.join("scripts");
    private_directory(&scripts)?;
    let _guard = lock(&scripts, &key)?;
    let storage = scripts.join("state").join(&key);
    private_directory(&storage)?;
    let lockfile = options
        .lockfile
        .clone()
        .unwrap_or_else(|| storage.join(project::LOCKFILE));
    if lockfile.canonicalize().ok().as_ref() == Some(&path) {
        return Err("--lockfile must not name the script source".into());
    }
    let _durable_guard = if options.lockfile.is_some() {
        let parent = lockfile
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let destination = parent.canonicalize()?.join(
            lockfile
                .file_name()
                .ok_or("--lockfile requires a filename")?,
        );
        let cache = cache.canonicalize()?;
        if destination.starts_with(&cache)
            || lockfile.canonicalize().is_ok_and(|p| p.starts_with(&cache))
        {
            return Err(
                "durable --lockfile must be outside the Plenty cache so cleanup preserves it"
                    .into(),
            );
        }
        let key = format!(
            "durable-{:x}",
            Sha256::digest(destination.as_os_str().as_encoded_bytes())
        );
        Some(lock(&scripts, &key)?)
    } else {
        None
    };
    let project = Project::load_script(manifest, directory, &storage, &lockfile, &options.resolve)?;
    if project.packages.len() > 1
        || (options.lockfile.is_some() && (!options.resolve.locked || lockfile.exists()))
    {
        sync_lock(&lockfile, &project.lockfile(), options.resolve.locked)?;
    }
    let mut graph = project.source_graph(0);
    graph.packages[0].root = directory.to_path_buf();
    graph.packages[0].directory = None;
    if options.check {
        eprintln!("  Checking {}", path.display());
        let mut heap = crate::value::Heap::default();
        let program = crate::frontend::compile_script(&path, &graph, &mut heap)?;
        crate::op::check(&program.ops)?;
        return Ok(ExitCode::SUCCESS);
    }
    let workspace = tempfile::tempdir()?;
    let output = options.output.clone().unwrap_or_else(|| {
        workspace
            .path()
            .join(format!("script{}", std::env::consts::EXE_SUFFIX))
    });
    if output.canonicalize().ok().as_ref() == Some(&path) {
        return Err("output must not overwrite the script source".into());
    }
    options.build.executable_cache = !options.compile;
    options.build.temporary_output = !options.compile;
    if options.compile {
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let staged = tempfile::Builder::new()
            .prefix(".plenty-script-")
            .suffix(std::env::consts::EXE_SUFFIX)
            .tempfile_in(parent)?
            .into_temp_path();
        crate::codegen::compile_script_to_executable(&path, &graph, &staged, &options.build)?;
        staged.persist(&output)?;
        return Ok(ExitCode::SUCCESS);
    }
    crate::codegen::compile_script_to_executable(&path, &graph, &output, &options.build)?;
    drop(_durable_guard);
    drop(_guard);
    let mut command = Command::new(&output);
    command.args(arguments);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.arg0(&file);
    }
    Ok(project::exit_code(command.status()?))
}

fn sync_lock(path: &Path, expected: &str, locked: bool) -> Result<()> {
    let current = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if current.as_deref() == Some(expected) {
        return Ok(());
    }
    if locked {
        return Err(format!(
            "{} is missing or stale; --locked forbids changing it",
            path.display()
        )
        .into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut file, expected.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

fn storage_root() -> Result<PathBuf> {
    // Ignore PLENTY_CACHE=off only for dependency state, not build artifacts.
    if let Some(path) = std::env::var_os("PLENTY_CACHE_DIR") {
        return Ok(path.into());
    }
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME").filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(path).join("plenty"));
    }
    Ok(PathBuf::from(
        std::env::var_os("HOME")
            .filter(|s| !s.is_empty())
            .ok_or("script storage requires PLENTY_CACHE_DIR, XDG_CACHE_HOME or HOME")?,
    )
    .join(".cache/plenty"))
}

fn private_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

fn lock(scripts: &Path, key: &str) -> Result<File> {
    let locks = scripts.join("locks");
    private_directory(&locks)?;
    let file = File::options()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(locks.join(format!("{key}.lock")))?;
    file.lock()?;
    Ok(file)
}

/// Editor resolution never fetches or discovers surrounding workspaces. Missing
/// cached dependencies fall back to ordinary file diagnostics in the caller.
pub(crate) fn editor_graph(path: &Path, text: &str) -> Option<project::SourceGraph> {
    let path = path.canonicalize().ok()?;
    let header = header(text, &path.display().to_string()).ok()?;
    let manifest = project::manifest::parse_script(
        header.manifest.as_deref().unwrap_or(""),
        &path.display().to_string(),
        &path,
    )
    .ok()?;
    let key = format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes()));
    let storage = storage_root().ok()?.join("scripts/state").join(key);
    let lockfile = storage.join(project::LOCKFILE);
    let directory = path.parent()?;
    let project = Project::load_script(
        manifest,
        directory,
        &storage,
        &lockfile,
        &ResolveOptions {
            locked: true,
            offline: true,
            ..Default::default()
        },
    )
    .ok()?;
    let mut graph = project.source_graph(0);
    graph.packages[0].root = directory.to_path_buf();
    graph.packages[0].directory = None;
    Some(graph)
}

pub(crate) fn cache_files(cache: &Path) -> Vec<PathBuf> {
    fn walk(path: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if let Ok(kind) = entry.file_type() {
                    if kind.is_dir() {
                        walk(&entry.path(), files);
                    } else if kind.is_file() {
                        files.push(entry.path());
                    }
                }
            }
        }
    }
    let mut files = vec![];
    walk(&cache.join("scripts/state"), &mut files);
    files
}

pub(crate) fn clean_cache(cache: &Path) -> Result<(u64, u64)> {
    let scripts = cache.join("scripts");
    let mut count = 0;
    let mut bytes = 0;
    if let Ok(entries) = fs::read_dir(scripts.join("state")) {
        for entry in entries {
            let entry = entry?;
            let Some(key) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let _guard = lock(&scripts, &key)?;
            // Inspect only this namespace while its resolver is excluded.
            fn size(path: &Path) -> std::io::Result<(u64, u64)> {
                let mut sum = (0, 0);
                for entry in fs::read_dir(path)? {
                    let entry = entry?;
                    let kind = entry.file_type()?;
                    if kind.is_dir() {
                        let child = size(&entry.path())?;
                        sum.0 += child.0;
                        sum.1 += child.1;
                    } else if kind.is_file() {
                        sum.0 += 1;
                        sum.1 += entry.metadata()?.len();
                    }
                }
                Ok(sum)
            }
            if !entry.path().exists() {
                continue;
            }
            let removed = size(&entry.path())?;
            fs::remove_dir_all(entry.path())?;
            count += removed.0;
            bytes += removed.1;
        }
    }
    Ok((count, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_preserves_body_bytes_and_original_manifest_lines() {
        let text = "#!/usr/bin/env -S plenty script\r\n\r\n---plenty\r\n[package]\r\ndescription='é'\r\n---\r\ndef main() -> ():\r\n    pass\r\n";
        let header = header(text, "test.plenty").unwrap();
        let source = header.source.unwrap();
        assert_eq!(source.len(), text.len());
        let body = text.find("def main").unwrap();
        assert_eq!(&source[body..], &text[body..]);
        assert_eq!(source.lines().count(), text.lines().count());
        assert_eq!(
            header.manifest.unwrap().lines().nth(4),
            Some("description='é'")
        );
    }

    #[test]
    fn delimiters_inside_multiline_strings_are_not_manifests() {
        let text =
            "def main() -> str:\n    \"\"\"has an internal \" quote\n---plenty\n---\n\"\"\"\n";
        let header = header(text, "test.plenty").unwrap();
        assert!(header.manifest.is_none());
        assert!(header.source.is_none());
    }

    #[test]
    fn defaults_and_strict_package_validation_remain_separate() {
        let manifest =
            project::manifest::parse_script("", "std.plenty", Path::new("std.plenty")).unwrap();
        assert_eq!(manifest.name, "script_std");
        assert_eq!(manifest.version, "0.0.0");
        assert!(manifest.library.is_none());
        assert_eq!(manifest.binaries.len(), 1);
        assert!(
            project::manifest::parse(
                "[package]\nname='hello'\nversion='0.1.0'\n",
                "plenty.toml",
                Path::new(".")
            )
            .unwrap_err()
            .contains("manifest-version")
        );
    }
}
