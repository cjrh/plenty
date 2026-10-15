//! `plenty.toml`: package metadata, targets, and direct dependencies.
use super::toml::{self, Entry, Table, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "plenty.toml";
/// The manifest format version, independent of package and toolchain versions.
pub const MANIFEST_VERSION: i64 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub license: Option<String>,
    /// The minimum toolchain version the package supports.
    pub plenty_version: Option<String>,
    /// Library entry relative to the package directory, if any.
    pub library: Option<PathBuf>,
    /// `(name, entry)` executables relative to the package directory.
    pub binaries: Vec<(String, PathBuf)>,
    /// Direct dependencies by import alias.
    pub dependencies: BTreeMap<String, Dependency>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dependency {
    /// A live local directory, relative to the declaring manifest.
    Path(PathBuf),
    /// A Git repository at an exact tag or revision.
    Git {
        url: String,
        subdir: Option<String>,
        selector: GitSelector,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum GitSelector {
    Tag(String),
    Rev(String),
}

/// Package names and dependency aliases are import roots, so they must be
/// identifiers. Lowercase keeps names stable on case-insensitive filesystems.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `MAJOR.MINOR.PATCH` with decimal components.
pub fn parse_version(text: &str) -> Option<[u64; 3]> {
    let mut parts = text.split('.');
    let mut out = [0; 3];
    for slot in &mut out {
        let part = parts.next()?;
        if part.is_empty()
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

struct Reader<'a> {
    name: &'a str,
}

impl Reader<'_> {
    fn error<T>(&self, entry: &Entry, message: impl std::fmt::Display) -> Result<T, String> {
        Err(format!("{}:{}: {message}", self.name, entry.line))
    }

    fn string(&self, key: &str, entry: &Entry) -> Result<String, String> {
        match &entry.value {
            Value::String(text) => Ok(text.clone()),
            other => self.error(
                entry,
                format!("`{key}` must be a string, not {}", other.kind()),
            ),
        }
    }

    fn table<'t>(&self, key: &str, entry: &'t Entry) -> Result<&'t Table, String> {
        match &entry.value {
            Value::Table(table) => Ok(table),
            other => self.error(
                entry,
                format!("`{key}` must be a table, not {}", other.kind()),
            ),
        }
    }

    /// Reject keys outside `allowed` so typos are errors, not ignored settings.
    fn known(&self, table: &Table, context: &str, allowed: &[&str]) -> Result<(), String> {
        for (key, entry) in table {
            if !allowed.contains(&key.as_str()) {
                return self.error(
                    entry,
                    format!(
                        "unknown key `{key}` in {context}; expected one of: {}",
                        allowed.join(", ")
                    ),
                );
            }
        }
        Ok(())
    }

    /// A target entry must stay inside the package's `src` directory.
    fn entry_path(&self, key: &str, entry: &Entry) -> Result<PathBuf, String> {
        let text = self.string(key, entry)?;
        let path = PathBuf::from(&text);
        let mut components = path.components();
        let inside = components.next() == Some(std::path::Component::Normal("src".as_ref()))
            && components.all(|c| matches!(c, std::path::Component::Normal(_)))
            && path.extension().is_some_and(|e| e == "plenty");
        if !inside {
            return self.error(
                entry,
                format!("`{key}` must be a relative .plenty path below src/, not {text:?}"),
            );
        }
        Ok(path)
    }
}

/// Parse a manifest. `name` labels diagnostics; `directory` locates default
/// targets, which exist only if their files do.
pub fn parse(text: &str, name: &str, directory: &Path) -> Result<Manifest, String> {
    let document = toml::parse(text, name)?;
    let reader = Reader { name };
    reader.known(
        &document,
        "the manifest",
        &["manifest-version", "package", "lib", "bin", "dependencies"],
    )?;
    match document.get("manifest-version") {
        None => {
            return Err(format!(
                "{name}: missing `manifest-version = {MANIFEST_VERSION}`"
            ))
        }
        Some(Entry {
            value: Value::Integer(MANIFEST_VERSION),
            ..
        }) => {}
        Some(entry) => {
            return reader.error(
                entry,
                format!(
                    "unsupported manifest-version; this toolchain reads version {MANIFEST_VERSION}"
                ),
            )
        }
    }
    let Some(package) = document.get("package") else {
        return Err(format!("{name}: missing [package] table"));
    };
    let package_entry = package;
    let package = reader.table("package", package)?;
    reader.known(
        package,
        "[package]",
        &[
            "name",
            "version",
            "description",
            "authors",
            "license",
            "plenty-version",
        ],
    )?;
    let required = |key: &str| -> Result<String, String> {
        match package.get(key) {
            Some(entry) => reader.string(key, entry),
            None => reader.error(package_entry, format!("[package] requires `{key}`")),
        }
    };
    let package_name = required("name")?;
    if !valid_name(&package_name) {
        return reader.error(
            &package["name"],
            format!(
                "package name {package_name:?} must be a lowercase identifier (letters, digits, `_`)"
            ),
        );
    }
    if package_name == "std" {
        return reader.error(
            &package["name"],
            "`std` is reserved for the standard library",
        );
    }
    let version = required("version")?;
    if parse_version(&version).is_none() {
        return reader.error(
            &package["version"],
            format!("version {version:?} must be MAJOR.MINOR.PATCH"),
        );
    }
    let optional = |key: &str| package.get(key).map(|e| reader.string(key, e)).transpose();
    let plenty_version = optional("plenty-version")?;
    if let Some(minimum) = &plenty_version {
        if parse_version(minimum).is_none() {
            return reader.error(
                &package["plenty-version"],
                format!("plenty-version {minimum:?} must be MAJOR.MINOR.PATCH"),
            );
        }
    }
    let authors = match package.get("authors") {
        None => vec![],
        Some(entry) => match &entry.value {
            Value::Array(items) if items.iter().all(|i| matches!(i, Value::String(_))) => items
                .iter()
                .map(|item| match item {
                    Value::String(text) => text.clone(),
                    _ => unreachable!(),
                })
                .collect(),
            _ => return reader.error(entry, "`authors` must be an array of strings"),
        },
    };

    let library = match document.get("lib") {
        Some(entry) => {
            let table = reader.table("lib", entry)?;
            reader.known(table, "[lib]", &["path"])?;
            Some(match table.get("path") {
                Some(path) => reader.entry_path("path", path)?,
                None => PathBuf::from("src/lib.plenty"),
            })
        }
        None => directory
            .join("src/lib.plenty")
            .is_file()
            .then(|| PathBuf::from("src/lib.plenty")),
    };
    let binaries = match document.get("bin") {
        Some(entry) => {
            let Value::Array(items) = &entry.value else {
                return reader.error(entry, "`bin` must be an array of tables: use [[bin]]");
            };
            let mut binaries: Vec<(String, PathBuf)> = vec![];
            for item in items {
                let Value::Table(table) = item else {
                    return reader.error(entry, "`bin` must be an array of tables: use [[bin]]");
                };
                reader.known(table, "[[bin]]", &["name", "path"])?;
                let (Some(name_entry), Some(path)) = (table.get("name"), table.get("path")) else {
                    return reader.error(entry, "each [[bin]] requires `name` and `path`");
                };
                let bin = reader.string("name", name_entry)?;
                if !valid_name(&bin) {
                    return reader.error(
                        name_entry,
                        format!("binary name {bin:?} must be a lowercase identifier"),
                    );
                }
                if binaries.iter().any(|(existing, _)| existing == &bin) {
                    return reader.error(name_entry, format!("binary `{bin}` is declared twice"));
                }
                binaries.push((bin, reader.entry_path("path", path)?));
            }
            binaries
        }
        None if directory.join("src/main.plenty").is_file() => {
            vec![(package_name.clone(), PathBuf::from("src/main.plenty"))]
        }
        None => vec![],
    };
    if library.is_none() && binaries.is_empty() {
        return Err(format!(
            "{name}: package `{package_name}` has no targets; add src/main.plenty, src/lib.plenty, [lib], or [[bin]]"
        ));
    }

    let mut dependencies = BTreeMap::new();
    if let Some(entry) = document.get("dependencies") {
        for (alias, entry) in reader.table("dependencies", entry)? {
            if !valid_name(alias) {
                return reader.error(
                    entry,
                    format!("dependency name `{alias}` must be a lowercase identifier"),
                );
            }
            if alias == "std" {
                return reader.error(
                    entry,
                    "`std` is reserved for the standard library and cannot name a dependency",
                );
            }
            let table = reader.table(alias, entry)?;
            reader.known(
                table,
                &format!("dependency `{alias}`"),
                &["path", "git", "subdir", "tag", "rev"],
            )?;
            let field = |key: &str| table.get(key).map(|e| reader.string(key, e)).transpose();
            let dependency = match (field("path")?, field("git")?) {
                (Some(path), None) => {
                    if let Some(key) = ["subdir", "tag", "rev"]
                        .into_iter()
                        .find(|k| table.contains_key(*k))
                    {
                        return reader.error(
                            &table[key],
                            format!(
                                "dependency `{alias}`: `{key}` applies only to git dependencies"
                            ),
                        );
                    }
                    if path.is_empty() {
                        return reader.error(entry, format!("dependency `{alias}`: empty path"));
                    }
                    Dependency::Path(PathBuf::from(path))
                }
                (None, Some(url)) => {
                    let selector = match (field("tag")?, field("rev")?) {
                        (Some(tag), None) => GitSelector::Tag(tag),
                        (None, Some(rev)) => GitSelector::Rev(rev),
                        _ => {
                            return reader.error(
                                entry,
                                format!(
                                "git dependency `{alias}` requires exactly one of `tag` or `rev`"
                            ),
                            )
                        }
                    };
                    Dependency::Git {
                        url,
                        subdir: field("subdir")?,
                        selector,
                    }
                }
                (Some(_), Some(_)) => {
                    return reader.error(
                        entry,
                        format!("dependency `{alias}` cannot have both `path` and `git`"),
                    )
                }
                (None, None) => {
                    return reader.error(
                        entry,
                        format!("dependency `{alias}` requires `path` or `git`"),
                    )
                }
            };
            dependencies.insert(alias.clone(), dependency);
        }
    }
    Ok(Manifest {
        name: package_name,
        version,
        description: optional("description")?,
        authors,
        license: optional("license")?,
        plenty_version,
        library,
        binaries,
        dependencies,
    })
}

/// The manifest that `plenty new` and `plenty init` write.
pub fn template(name: &str) -> String {
    format!(
        "manifest-version = {MANIFEST_VERSION}\n\n[package]\nname = {}\nversion = \"0.1.0\"\nplenty-version = {}\n\n[dependencies]\n",
        toml::quote(name),
        toml::quote(env!("CARGO_PKG_VERSION")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(body: &str) -> Result<Manifest, String> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.plenty"), "").unwrap();
        parse(
            &format!(
                "manifest-version = 1\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n{body}"
            ),
            "plenty.toml",
            dir.path(),
        )
    }

    #[test]
    fn defaults_and_dependencies() {
        let m = manifest(
            "[dependencies]\nutil = { path = \"../util\" }\n[dependencies.audio]\ngit = \"file:///r\"\nsubdir = \"packages/audio\"\ntag = \"v1\"\n",
        )
        .unwrap();
        assert_eq!(m.binaries, vec![("app".into(), "src/main.plenty".into())]);
        assert_eq!(m.library, None);
        assert_eq!(m.dependencies["util"], Dependency::Path("../util".into()));
        assert_eq!(
            m.dependencies["audio"],
            Dependency::Git {
                url: "file:///r".into(),
                subdir: Some("packages/audio".into()),
                selector: GitSelector::Tag("v1".into()),
            }
        );
    }

    #[test]
    fn invalid_manifests_are_reported() {
        for (body, message) in [
            (
                "edition = 1\n",
                "plenty.toml:5: unknown key `edition` in [package]",
            ),
            (
                "[dependencies]\nstd = { path = \"x\" }\n",
                "`std` is reserved",
            ),
            (
                "[dependencies]\nx = { path = \"a\", git = \"b\" }\n",
                "both `path` and `git`",
            ),
            (
                "[dependencies]\nx = { git = \"b\" }\n",
                "exactly one of `tag` or `rev`",
            ),
            (
                "[dependencies]\nx = { git = \"b\", tag = \"t\", rev = \"r\" }\n",
                "exactly one",
            ),
            (
                "[dependencies]\nx = { path = \"a\", tag = \"t\" }\n",
                "only to git",
            ),
            ("[dependencies]\nx = { }\n", "requires `path` or `git`"),
            (
                "[dependencies]\nBad = { path = \"a\" }\n",
                "lowercase identifier",
            ),
            ("[lib]\npath = \"../x.plenty\"\n", "below src/"),
            ("[[bin]]\nname = \"a\"\n", "requires `name` and `path`"),
            (
                "[dependencies]\nx = { path = \"a\", branch = \"main\" }\n",
                "unknown key `branch`",
            ),
        ] {
            let error = manifest(body).unwrap_err();
            assert!(error.contains(message), "{body:?}: {error}");
        }
        let dir = tempfile::tempdir().unwrap();
        for (text, message) in [
            (
                "[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
                "missing `manifest-version",
            ),
            ("manifest-version = 2\n", "unsupported manifest-version"),
            (
                "manifest-version = 1\n[package]\nname = \"a\"\nversion = \"1.0\"\n",
                "MAJOR.MINOR.PATCH",
            ),
            (
                "manifest-version = 1\n[package]\nname = \"a-b\"\nversion = \"1.0.0\"\n",
                "lowercase identifier",
            ),
            (
                "manifest-version = 1\n[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
                "has no targets",
            ),
            (
                "manifest-version = 1\nedition = 1\n",
                "plenty.toml:2: unknown key `edition` in the manifest",
            ),
        ] {
            let error = parse(text, "plenty.toml", dir.path()).unwrap_err();
            assert!(error.contains(message), "{text:?}: {error}");
        }
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("1.20.3"), Some([1, 20, 3]));
        for bad in ["1.2", "1.2.3.4", "01.2.3", "1.x.3", ""] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }
}
