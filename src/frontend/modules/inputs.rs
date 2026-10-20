//! What a program build read from outside the compiler, so the program cache
//! can tell whether a stored object is still the one a build would produce
//! (issue #317). See `book/src/design/toolchain/02-architecture/cache.md`.
//!
//! A source file's text decides its imports, and the loader's resolution of
//! an import depends only on the import's module path, the importing package
//! and the filesystem. So the inputs are current when every recorded file
//! still has its text and every recorded import still resolves where it did:
//! resolving it again repeats each existence check the loader made, so a new
//! file that would shadow, conflict with, or replace a module is noticed.
//! Standard modules are part of the compiler: the cache key covers the
//! compiler's identity and [`standard_digest`].
use super::{prepare_packages, Import, Kind, Loader, ModuleSource, Token, STANDARD_MODULES};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A digest of every standard module's name and text.
pub(crate) fn standard_digest() -> &'static [u8; 32] {
    static DIGEST: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    DIGEST.get_or_init(|| {
        let mut hash = Sha256::new();
        for (name, text) in STANDARD_MODULES {
            for part in [name.as_bytes(), text.as_bytes()] {
                hash.update((part.len() as u64).to_le_bytes());
                hash.update(part);
            }
        }
        hash.finalize().into()
    })
}

/// One source file read by a build.
#[derive(Debug, PartialEq)]
pub(crate) struct File {
    path: PathBuf,
    /// The package whose root its imports resolve against.
    package: usize,
    digest: [u8; 32],
    imports: Vec<Resolution>,
}

/// Where one import of a file resolved.
#[derive(Debug, PartialEq)]
struct Resolution {
    module: String,
    source: ModuleSource,
    owner: usize,
}

impl File {
    pub(super) fn new(path: &Path, package: usize, text: &str) -> File {
        File {
            path: path.to_path_buf(),
            package,
            digest: Sha256::digest(text.as_bytes()).into(),
            imports: vec![],
        }
    }

    pub(super) fn import(&mut self, module: &str, source: &ModuleSource, owner: usize) {
        self.imports.push(Resolution {
            module: module.to_owned(),
            source: source.clone(),
            owner,
        });
    }
}

/// Every source file a build read, in the order the loader read them.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Inputs {
    files: Vec<File>,
}

impl Inputs {
    pub(super) fn new(files: Vec<File>) -> Inputs {
        Inputs { files }
    }

    /// Whether a build of `graph` now would read the same files with the same
    /// text and resolve every import the same way. Any error is `false`: the
    /// build then runs and reports it.
    pub(crate) fn current(&self, graph: &crate::project::SourceGraph) -> bool {
        let Ok(packages) = prepare_packages(graph) else {
            return false;
        };
        let loader = Loader {
            packages,
            active: vec![],
            loaded: HashMap::new(),
            modules: vec![],
            dependencies: vec![],
            appendix: None,
            inputs: vec![],
        };
        self.files.iter().all(|file| {
            std::fs::read(&file.path).is_ok_and(|text| *Sha256::digest(text) == file.digest)
                && file.imports.iter().all(|import| {
                    let probe = Import {
                        at: Token {
                            kind: Kind::Eof,
                            line: 0,
                            column: 0,
                            source: None,
                        },
                        module: import.module.clone(),
                        item: None,
                        binding: String::new(),
                    };
                    loader
                        .import_path(&probe, file.package)
                        .is_ok_and(|(source, owner)| {
                            source == import.source && owner == import.owner
                        })
                })
        })
    }

    /// The inputs as bytes, or `None` when a path is not UTF-8 and so has no
    /// portable spelling; such a build is not cached.
    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        put_count(&mut out, self.files.len());
        for file in &self.files {
            put_text(&mut out, file.path.to_str()?);
            put_count(&mut out, file.package);
            out.extend_from_slice(&file.digest);
            put_count(&mut out, file.imports.len());
            for import in &file.imports {
                put_text(&mut out, &import.module);
                match &import.source {
                    ModuleSource::Standard(name) => {
                        out.push(0);
                        put_text(&mut out, name);
                    }
                    ModuleSource::File(path) => {
                        out.push(1);
                        put_text(&mut out, path.to_str()?);
                    }
                }
                put_count(&mut out, import.owner);
            }
        }
        Some(out)
    }

    /// [`Inputs::encode`]'s bytes back, or `None` if they are not exactly an
    /// encoding.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Inputs> {
        let mut reader = Reader(bytes);
        let mut files = Vec::new();
        for _ in 0..reader.count()? {
            let path = PathBuf::from(reader.text()?);
            let package = reader.count()?;
            let digest = reader.take(32)?.try_into().ok()?;
            let mut imports = Vec::new();
            for _ in 0..reader.count()? {
                let module = reader.text()?.to_owned();
                let source = match reader.take(1)?[0] {
                    0 => {
                        let name = reader.text()?;
                        let &(name, _) = STANDARD_MODULES.iter().find(|(n, _)| *n == name)?;
                        ModuleSource::Standard(name)
                    }
                    1 => ModuleSource::File(PathBuf::from(reader.text()?)),
                    _ => return None,
                };
                let owner = reader.count()?;
                imports.push(Resolution {
                    module,
                    source,
                    owner,
                });
            }
            files.push(File {
                path,
                package,
                digest,
                imports,
            });
        }
        reader.0.is_empty().then_some(Inputs { files })
    }
}

fn put_count(out: &mut Vec<u8>, count: usize) {
    out.extend_from_slice(&(count as u64).to_le_bytes());
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    put_count(out, text.len());
    out.extend_from_slice(text.as_bytes());
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }

    fn count(&mut self) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(self.take(8)?.try_into().ok()?)).ok()
    }

    fn text(&mut self) -> Option<&'a str> {
        let n = self.count()?;
        std::str::from_utf8(self.take(n)?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_round_trip_and_reject_damage() {
        let mut file = File::new(Path::new("/src/main.plenty"), 0, "import a\n");
        file.import("a", &ModuleSource::File("/src/a.plenty".into()), 0);
        file.import("std.json", &ModuleSource::Standard("std.json"), 0);
        let inputs = Inputs::new(vec![file]);
        let bytes = inputs.encode().unwrap();
        assert_eq!(Inputs::decode(&bytes), Some(inputs));
        assert_eq!(Inputs::decode(&bytes[..bytes.len() - 1]), None);
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(Inputs::decode(&longer), None);
        // A huge count runs out of bytes instead of allocating.
        assert_eq!(Inputs::decode(&u64::MAX.to_le_bytes()), None);
    }
}
