//! Packages: `plenty.toml` manifests, workspaces, path and Git dependency
//! graphs, `plenty.lock`, build reuse, and the project commands behind
//! `plenty build`, `run`, `check`, `test`, `add`, `fetch`, and `update`.
//!
//! The compiler sees a project only as a [`SourceGraph`]: one source root per
//! package and each package's direct dependency aliases. Dependency resolution
//! and fetching never happen while evaluating imports.
mod fingerprint;
mod identity;
mod inspection;
mod lock;
pub use identity::PackageIdentity;
pub use inspection::inspect;
use lock::{Pins, read_pins};
mod git;
pub mod manifest;
mod toml;

use fingerprint::Fingerprint;
use manifest::{Dependency, GitSelector, MANIFEST, Manifest, Source};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub const LOCKFILE: &str = "plenty.lock";
/// Generated dependency sources, beside the lockfile.
pub const PACKAGE_CACHE: &str = ".pkg";

/// Readable provenance, separate from the compiler's nominal package key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageProvenance {
    pub name: String,
    pub version: String,
    pub source: String,
    /// A dependency chain from a selected root, using import aliases.
    pub reached: String,
}

/// One package's source tree as the compiler sees it.
#[derive(Clone, Debug)]
pub struct SourcePackage {
    /// Stable declaration qualifier, independent of the importing alias.
    pub identity: String,
    /// Full source identity. Digests are checked against this value.
    pub source_identity: String,
    pub provenance: Option<PackageProvenance>,
    /// Explicit owner for a synthetic integration-test package.
    pub test_of: Option<String>,
    pub root: PathBuf,
    /// Bounds imports/includes; absent for single-file programs and scripts.
    pub directory: Option<PathBuf>,
    pub library: Option<PathBuf>,
    /// `(alias, index into SourceGraph::packages)` for direct dependencies.
    pub dependencies: Vec<(String, usize)>,
}

impl SourcePackage {
    pub(crate) fn display_name(&self) -> &str {
        self.provenance.as_ref().map_or(&self.identity, |p| &p.name)
    }

    /// Include the complete digest, and retain its full preimage in the graph.
    pub fn resolved_identity(name: &str, version: &str, source_identity: &str) -> String {
        use sha2::{Digest, Sha256};
        let digest = crate::hash_hex::encode(&Sha256::digest(source_identity.as_bytes()));
        format!("{name}@{version}#{digest}")
    }
}

/// The packages of one compilation; package zero contains the entry file.
#[derive(Clone, Debug)]
pub struct SourceGraph {
    pub packages: Vec<SourcePackage>,
}

const GRAPH_VERSION: i64 = 2;

impl SourceGraph {
    /// Reject duplicate identities, digest/preimage mismatches, invalid edges,
    /// and forged synthetic-test ownership before loading any source files.
    pub(crate) fn validate(&self) -> Result<()> {
        let mut identities = BTreeMap::new();
        let mut sources = BTreeMap::new();
        if self.packages.is_empty() {
            return Err("package graph requires at least one package".into());
        }
        for package in &self.packages {
            if package.provenance.is_some() && package.source_identity.is_empty() {
                return Err(format!(
                    "package `{}` declares provenance without a full source identity",
                    package.identity
                )
                .into());
            }
            if let Some(previous) = identities.insert(&package.identity, &package.source_identity) {
                return Err(if previous != &package.source_identity {
                    format!("package identity digest collision for `{}`: distinct full source identities", package.identity)
                } else {
                    format!("identity `{}` appears twice", package.identity)
                }.into());
            }
            if !package.source_identity.is_empty()
                && let Some(previous) = sources.insert(&package.source_identity, &package.identity)
                && previous != &package.identity
            {
                return Err("one source identity has multiple compiler identities".into());
            }
            if !package.source_identity.is_empty() {
                use sha2::{Digest, Sha256};
                let digest =
                    crate::hash_hex::encode(&Sha256::digest(package.source_identity.as_bytes()));
                if package.identity.rsplit_once('#').map(|(_, suffix)| suffix)
                    != Some(digest.as_str())
                {
                    return Err(format!("package `{}` has an identity digest that does not match its full source identity", package.identity).into());
                }
                if let Some(provenance) = &package.provenance
                    && (package.source_identity.is_empty()
                        || package.identity
                            != SourcePackage::resolved_identity(
                                &provenance.name,
                                &provenance.version,
                                &package.source_identity,
                            ))
                {
                    return Err(format!("package `{}` has an identity digest that does not match its full source identity", package.identity).into());
                }
            }
            let mut aliases = BTreeSet::new();
            for (alias, target) in &package.dependencies {
                if !manifest::valid_name(alias) || alias == "std" || !aliases.insert(alias) {
                    return Err(format!("invalid or repeated dependency alias `{alias}`").into());
                }
                if *target >= self.packages.len() {
                    return Err(format!("dependency `{alias}` has an invalid package index").into());
                }
            }
        }
        for package in &self.packages {
            if let Some(owner) = &package.test_of {
                let Some(owner) = self.packages.iter().find(|p| &p.identity == owner) else {
                    return Err(format!(
                        "test package `{}` names an unknown owner",
                        package.identity
                    )
                    .into());
                };
                if owner.test_of.is_some()
                    || owner.identity == package.identity
                    || package.directory.is_none()
                    || package.directory != owner.directory
                    || package.root != owner.directory.as_ref().unwrap().join("tests")
                {
                    return Err(format!(
                        "test package `{}` has invalid ownership",
                        package.identity
                    )
                    .into());
                }
            }
        }
        Ok(())
    }

    /// Origins of repeated names, shared by command-line and IDE diagnostics.
    pub(crate) fn origin_notes(&self) -> String {
        let mut names = BTreeMap::<&str, usize>::new();
        for package in &self.packages {
            if let Some(p) = &package.provenance {
                *names.entry(&p.name).or_default() += 1;
            }
        }
        let mut notes = String::new();
        for package in &self.packages {
            if let Some(p) = &package.provenance
                && names.get(p.name.as_str()).copied().unwrap_or(0) > 1
            {
                notes.push_str(&format!(
                    "\n  package `{}`: {} {}, source {}, reached through {} (full identity {})",
                    package.identity,
                    p.name,
                    p.version,
                    p.source,
                    p.reached,
                    package.source_identity,
                ));
            }
        }
        notes
    }

    /// Keep structured source positions and attribution when adding origins.
    pub(crate) fn explain_error(&self, error: Box<dyn Error>) -> Box<dyn Error> {
        let notes = self.origin_notes();
        if notes.is_empty() {
            return error;
        }
        let mut diagnostic = crate::diagnostic::Diagnostic::from_boxed(error);
        diagnostic.message += &notes;
        diagnostic
    }

    /// Versioned compiler input, retaining full source and display metadata.
    pub fn to_toml(&self) -> String {
        let mut out = format!(
            "# Generated by `plenty graph` for `plentyc --package-graph`.\ngraph-version = {GRAPH_VERSION}\n"
        );
        for package in &self.packages {
            out.push_str(&format!(
                "\n[[package]]\nidentity = {}\nsource-identity = {}\n",
                toml::quote(&package.identity),
                toml::quote(&package.source_identity)
            ));
            let mut path = |key: &str, path: &Path| {
                let (bytes, text) = identity::encode_path(path);
                out.push_str(&format!(
                    "{key}{} = {}\n",
                    if bytes { "-bytes" } else { "" },
                    toml::quote(&text)
                ));
            };
            path("root", &package.root);
            if let Some(directory) = &package.directory {
                path("directory", directory);
            }
            if let Some(library) = &package.library {
                path("library", library);
            }
            if let Some(p) = &package.provenance {
                for (key, value) in [
                    ("name", &p.name),
                    ("version", &p.version),
                    ("source", &p.source),
                    ("reached", &p.reached),
                ] {
                    out.push_str(&format!("{key} = {}\n", toml::quote(value)));
                }
            }
            if let Some(owner) = &package.test_of {
                out.push_str(&format!("test-of = {}\n", toml::quote(owner)));
            }
            let mut edges: Vec<_> = package.dependencies.iter().collect();
            edges.sort_by(|(left, _), (right, _)| left.cmp(right));
            let dependencies = edges
                .into_iter()
                .map(|(alias, target)| {
                    format!(
                        "{alias} = {}",
                        toml::quote(&self.packages[*target].identity)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            if dependencies.is_empty() {
                out.push_str("dependencies = {}\n");
            } else {
                out.push_str(&format!("dependencies = {{ {dependencies} }}\n"));
            }
        }
        out
    }

    /// Version one is adapted as a graph with caller-supplied opaque identities
    /// and no provenance or synthetic ownership. Version two checks metadata.
    pub fn read(path: &Path) -> Result<SourceGraph> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let name = path.display().to_string();
        let base = path.parent().unwrap_or(Path::new("."));
        let document = toml::parse(&text, &name)?;
        let error = |line: usize, message: &str| format!("{name}:{line}: {message}");
        for (key, entry) in &document {
            if key != "graph-version" && key != "package" {
                return Err(error(entry.line, &format!("unknown key `{key}`")).into());
            }
        }
        let version = match document.get("graph-version").map(|e| &e.value) {
            Some(toml::Value::Integer(version @ (1 | GRAPH_VERSION))) => *version,
            _ => return Err(format!("{name}: unsupported graph-version; this compiler reads versions 1 and {GRAPH_VERSION}").into()),
        };
        let tables = match document.get("package").map(|e| &e.value) {
            Some(toml::Value::Array(items)) if !items.is_empty() => items,
            _ => return Err(format!("{name}: requires at least one [[package]]").into()),
        };
        let mut packages = vec![];
        let mut edges = vec![];
        for item in tables {
            let toml::Value::Table(table) = item else {
                return Err(format!("{name}: `package` must be an array of tables").into());
            };
            let line = table.values().map(|e| e.line).min().unwrap_or(0);
            let string = |key: &str| match table.get(key).map(|e| &e.value) {
                Some(toml::Value::String(text)) => Ok(Some(text.clone())),
                None => Ok(None),
                Some(_) => Err(error(line, &format!("`{key}` must be a string"))),
            };
            let required = |key| {
                string(key)?.ok_or_else(|| error(line, &format!("[[package]] requires `{key}`")))
            };
            for key in table.keys() {
                let legacy = ["identity", "root", "library", "dependencies"];
                let current = [
                    "source-identity",
                    "root-bytes",
                    "directory-bytes",
                    "library-bytes",
                    "directory",
                    "name",
                    "version",
                    "source",
                    "reached",
                    "test-of",
                ];
                if !legacy.contains(&key.as_str())
                    && !(version == GRAPH_VERSION && current.contains(&key.as_str()))
                {
                    return Err(error(line, &format!("unknown key `{key}` in [[package]]")).into());
                }
            }
            let path_field = |key: &str| -> Result<Option<PathBuf>> {
                match (string(key)?, string(&format!("{key}-bytes"))?) {
                    (Some(_), Some(_)) => Err(error(
                        line,
                        &format!("choose `{key}` or `{key}-bytes`, not both"),
                    )
                    .into()),
                    (Some(path), None) => Ok(Some(base.join(path))),
                    (None, Some(bytes)) => Ok(Some(base.join(
                        identity::decode_path(true, &bytes).ok_or_else(|| {
                            error(
                                line,
                                &format!("invalid native path encoding in `{key}-bytes`"),
                            )
                        })?,
                    ))),
                    (None, None) => Ok(None),
                }
            };
            let identity = required("identity")?;
            let root = path_field("root")?
                .ok_or_else(|| error(line, "[[package]] requires `root` or `root-bytes`"))?;
            let directory = if version == 1 {
                Some(
                    match root.parent() {
                        Some(parent) if parent.as_os_str().is_empty() => Path::new("."),
                        Some(parent) => parent,
                        None => &root,
                    }
                    .to_path_buf(),
                )
            } else {
                path_field("directory")?
            };
            let source_identity = if version == 1 {
                String::new()
            } else {
                required("source-identity")?
            };
            let provenance = if table.contains_key("name")
                || table.contains_key("version")
                || table.contains_key("source")
                || table.contains_key("reached")
            {
                Some(PackageProvenance {
                    name: required("name")?,
                    version: required("version")?,
                    source: required("source")?,
                    reached: required("reached")?,
                })
            } else {
                None
            };
            let mut dependencies = vec![];
            match table.get("dependencies").map(|e| &e.value) {
                None => {}
                Some(toml::Value::Table(aliases)) => {
                    for (alias, target) in aliases {
                        let toml::Value::String(target) = &target.value else {
                            return Err(error(
                                target.line,
                                "dependency targets are identity strings",
                            )
                            .into());
                        };
                        dependencies.push((alias.clone(), target.clone()));
                    }
                }
                Some(_) => return Err(error(line, "`dependencies` must be a table").into()),
            }
            packages.push(SourcePackage {
                identity,
                source_identity,
                provenance,
                test_of: string("test-of")?,
                directory,
                root,
                library: path_field("library")?,
                dependencies: vec![],
            });
            edges.push(dependencies);
        }
        for (index, dependencies) in edges.into_iter().enumerate() {
            for (alias, target) in dependencies {
                let Some(target) = packages.iter().position(|p| p.identity == target) else {
                    return Err(format!(
                        "{name}: package `{}` depends on unknown identity `{target}`",
                        packages[index].identity
                    )
                    .into());
                };
                if packages[target].library.is_none() {
                    return Err(format!(
                        "{name}: dependency `{alias}` names package `{}`, which has no `library`",
                        packages[target].identity
                    )
                    .into());
                }
                packages[index].dependencies.push((alias, target));
            }
        }
        let graph = SourceGraph { packages };
        graph.validate().map_err(|e| format!("{name}: {e}"))?;
        Ok(graph)
    }
}

/// Selects the generated code configuration, not the language rules: bounds,
/// overflow, and ownership checks are the same in both profiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    FastCompile,
    Optimized,
}

impl Profile {
    pub fn name(self) -> &'static str {
        match self {
            Profile::FastCompile => "fast-compile",
            Profile::Optimized => "optimized",
        }
    }
}

/// Where a resolved package's files come from.
#[derive(Clone, Debug, PartialEq)]
pub enum PackageSource {
    /// A workspace member, the root package, or a live local directory.
    Local,
    /// A snapshot of one commit of a Git repository.
    Git {
        url: String,
        /// The package's directory inside the repository, if not its root.
        subdir: Option<PathBuf>,
        commit: String,
        /// The tags and revisions that dependents requested; all name `commit`.
        selectors: BTreeSet<GitSelector>,
        /// The snapshot's root directory, which in-repository path
        /// dependencies cannot leave.
        snapshot: PathBuf,
    },
}

/// A resolved package and its direct dependencies.
#[derive(Clone, Debug)]
pub struct Package {
    pub identity: PackageIdentity,
    pub manifest: Manifest,
    /// Canonical package directory, containing `plenty.toml`.
    pub directory: PathBuf,
    pub dependencies: Vec<(String, usize)>,
    pub source: PackageSource,
    /// The dependency chain that first reached this package, for conflicts.
    reached: String,
}

/// A resolved dependency graph and the packages a command acts on.
#[derive(Debug)]
pub struct Project {
    pub packages: Vec<Package>,
    /// The workspace directory, or the root package's: it holds
    /// `plenty.lock`, `.pkg/`, and `target/`.
    pub root: PathBuf,
    /// Workspace members (or just the root package), in manifest order.
    pub members: Vec<usize>,
    /// The members the command was run for: all of them at the workspace
    /// root, otherwise the package containing the current directory.
    pub selected: Vec<usize>,
}

/// How resolution may change pinned Git commits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Update {
    /// Keep every commit that `plenty.lock` pins.
    #[default]
    None,
    /// Resolve every tag and revision again.
    All,
    /// Resolve the named packages again and keep the other pins.
    Packages(Vec<String>),
}

/// Options that govern dependency resolution.
#[derive(Clone, Debug, Default)]
pub struct ResolveOptions {
    /// Refuse to create or change `plenty.lock`.
    pub locked: bool,
    /// Never run `git fetch`; use only the package cache.
    pub offline: bool,
    pub update: Update,
}

/// The nearest directory at or above `start` that contains `plenty.toml`.
pub fn discover(start: &Path) -> Result<PathBuf> {
    let start = start.canonicalize()?;
    start
        .ancestors()
        .find(|dir| dir.join(MANIFEST).is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "could not find {MANIFEST} in {} or any parent directory",
                start.display()
            )
            .into()
        })
}

fn read_document(directory: &Path) -> Result<manifest::Document> {
    let path = directory.join(MANIFEST);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    Ok(manifest::parse_document(
        &text,
        &path.display().to_string(),
        directory,
    )?)
}

fn read_manifest(directory: &Path) -> Result<Manifest> {
    let document = read_document(directory)?;
    let Some(manifest) = document.package else {
        return Err(format!(
            "{}: a workspace manifest is not a package",
            directory.join(MANIFEST).display()
        )
        .into());
    };
    if let Some(minimum) = &manifest.plenty_version {
        let required = manifest::parse_version(minimum).unwrap();
        let current = manifest::parse_version(env!("CARGO_PKG_VERSION")).unwrap();
        if required > current {
            return Err(format!(
                "package `{}` requires Plenty {minimum} or newer; this toolchain is {}",
                manifest.name,
                env!("CARGO_PKG_VERSION")
            )
            .into());
        }
    }
    Ok(manifest)
}

/// The project directory (workspace or package), its member package
/// directories, and the members selected from `start`.
struct Layout {
    root: PathBuf,
    members: Vec<PathBuf>,
    selected: Vec<PathBuf>,
}

fn layout(start: &Path) -> Result<Layout> {
    let package = discover(start)?;
    for directory in package.ancestors() {
        let path = directory.join(MANIFEST);
        // Only a manifest that can declare a workspace needs parsing here,
        // so an unrelated broken manifest above a package is no obstacle.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !text.contains("workspace") {
            continue;
        }
        let document = read_document(directory)?;
        let Some(members) = document.members else {
            continue;
        };
        let mut member_directories = vec![];
        if document.package.is_some() {
            member_directories.push(directory.to_path_buf());
        }
        for member in members {
            let member_directory = directory.join(&member);
            if !member_directory.join(MANIFEST).is_file() {
                return Err(format!(
                    "workspace {}: member {} has no {MANIFEST}",
                    path.display(),
                    member.display()
                )
                .into());
            }
            let member_directory = member_directory.canonicalize()?;
            if !member_directories.contains(&member_directory) {
                member_directories.push(member_directory);
            }
        }
        if directory == package {
            return Ok(Layout {
                root: package,
                selected: member_directories.clone(),
                members: member_directories,
            });
        }
        if member_directories.contains(&package) {
            return Ok(Layout {
                root: directory.to_path_buf(),
                members: member_directories,
                selected: vec![package],
            });
        }
    }
    Ok(Layout {
        root: package.clone(),
        members: vec![package.clone()],
        selected: vec![package],
    })
}

/// Validate locally knowable lock changes before any Git checkout can fetch.
/// Git manifests at pinned immutable commits are checked during resolution.
fn script_lock_preflight(manifest: &Manifest, directory: &Path, lockfile: &Path) -> Result<()> {
    let mut pins = lock::read_pins_at(lockfile, directory)?;
    pins.script_root(manifest, directory);
    pins.require_current_format(true)?;
    pins.preflight(manifest, directory)
}

/// The resolution state of one `Project::load`.
struct Resolver {
    options: ResolveOptions,
    pins: Pins,
    cache: git::Cache,
    identities: BTreeMap<PackageIdentity, usize>,
    visiting: BTreeSet<PackageIdentity>,
    fresh: BTreeMap<(String, String, String), String>,
    selected_edges: BTreeSet<(String, String)>,
    used_updates: BTreeSet<(String, String)>,
}

impl Project {
    /// Resolve the workspace or package containing `start`: read every
    /// manifest, and fetch and snapshot Git dependencies as needed.
    pub fn load(start: &Path, options: &ResolveOptions) -> Result<Project> {
        Self::load_with(start, options, None)
    }

    /// Resolve one script without workspace discovery or adjacent generated files.
    pub(crate) fn load_script(
        manifest: Manifest,
        directory: &Path,
        storage: &Path,
        lockfile: &Path,
        options: &ResolveOptions,
    ) -> Result<Project> {
        if options.locked {
            script_lock_preflight(&manifest, directory, lockfile)?;
        }
        let mut pins = lock::read_pins_at(lockfile, directory)?;
        pins.script_root(&manifest, directory);
        pins.require_current_format(options.locked)?;
        let mut resolver = Resolver::new(
            options,
            pins,
            git::Cache::new(storage.join(PACKAGE_CACHE), options.offline),
        );
        let mut project = Project {
            packages: vec![],
            root: directory.to_path_buf(),
            members: vec![0],
            selected: vec![0],
        };
        project.visit(
            Some(manifest),
            directory.to_path_buf(),
            PackageSource::Local,
            &mut vec![],
            &mut resolver,
            None,
        )?;
        project.validate_resources(0)?;
        Ok(project)
    }

    /// `load`, reusing a cache that has already fetched some repositories
    /// during this command.
    fn load_with(
        start: &Path,
        options: &ResolveOptions,
        cache: Option<git::Cache>,
    ) -> Result<Project> {
        let start = start
            .canonicalize()
            .map_err(|e| format!("{}: {e}", start.display()))?;
        let layout = layout(&start)?;
        let pins = read_pins(&layout.root.join(LOCKFILE))?;
        pins.require_current_format(options.locked)?;
        let selected_edges =
            pins.select_updates(&options.update, &layout.root, &layout.selected)?;
        let mut resolver = Resolver::new(
            options,
            pins,
            cache.unwrap_or_else(|| {
                git::Cache::new(layout.root.join(PACKAGE_CACHE), options.offline)
            }),
        );
        resolver.selected_edges = selected_edges;
        let mut project = Project {
            packages: vec![],
            root: layout.root.clone(),
            members: vec![],
            selected: vec![],
        };
        for member in &layout.members {
            let index = project.visit(
                None,
                member.clone(),
                PackageSource::Local,
                &mut vec![],
                &mut resolver,
                None,
            )?;
            if project.members.iter().any(|i: &usize| {
                project.packages[*i].manifest.name == project.packages[index].manifest.name
            }) {
                return Err(format!(
                    "workspace members have duplicate name `{}`",
                    project.packages[index].manifest.name
                )
                .into());
            }
            project.members.push(index);
        }
        project.selected = layout
            .selected
            .iter()
            .map(|directory| {
                project
                    .packages
                    .iter()
                    .position(|p| &p.directory == directory)
                    .unwrap()
            })
            .collect();
        for &member in &project.selected {
            project.validate_resources(member)?;
        }
        if let Some((owner, alias)) = resolver
            .selected_edges
            .difference(&resolver.used_updates)
            .next()
        {
            return Err(format!("selected dependency edge {owner}::{alias} is no longer declared in this graph; run `plenty fetch` to reconcile the lock before choosing an update").into());
        }
        Ok(project)
    }

    /// Depth-first resolution. `chain` holds the dependency path from a
    /// member, so cycles and conflicts can name how each package was reached.
    fn visit(
        &mut self,
        supplied: Option<Manifest>,
        directory: PathBuf,
        source: PackageSource,
        chain: &mut Vec<String>,
        resolver: &mut Resolver,
        previous: Option<String>,
    ) -> Result<usize> {
        let identity = supplied
            .as_ref()
            .and_then(|m| m.binaries.first())
            .map(|(_, path)| PackageIdentity::Script(path.clone()))
            .unwrap_or_else(|| PackageIdentity::new(&directory, &source));
        if resolver.visiting.contains(&identity) {
            let name = &self.packages[resolver.identities[&identity]].manifest.name;
            return Err(format!(
                "dependency cycle: {} -> {name} ({})",
                chain.join(" -> "),
                describe_location(&directory, &source)
            )
            .into());
        }
        if let Some(&index) = resolver.identities.get(&identity) {
            if let (
                PackageSource::Git { selectors, .. },
                PackageSource::Git { selectors: old, .. },
            ) = (&source, &mut self.packages[index].source)
            {
                old.extend(selectors.iter().cloned());
            }
            return Ok(index);
        }
        let manifest = match supplied {
            Some(manifest) => manifest,
            None => read_manifest(&directory)?,
        };
        resolver.visiting.insert(identity.clone());
        resolver
            .identities
            .insert(identity.clone(), self.packages.len());
        let id = identity.serialized(&self.root);
        let previous = resolver.pins.prior_owner(&id, previous.as_deref());
        let index = self.packages.len();
        self.packages.push(Package {
            identity: identity.clone(),
            manifest: manifest.clone(),
            directory: directory.clone(),
            dependencies: vec![],
            source: source.clone(),
            reached: if chain.is_empty() {
                manifest.name.clone()
            } else {
                format!("{} -> {}", chain.join(" -> "), manifest.name)
            },
        });
        chain.push(manifest.name.clone());
        let mut dependencies = vec![];
        let mut direct_names = BTreeMap::new();
        for (alias, dependency) in &manifest.dependencies {
            let target = self.dependency(
                index,
                alias,
                dependency,
                chain,
                resolver,
                previous.as_deref(),
            )?;
            let package = &self.packages[target];
            if let Some((other, old_alias)) =
                direct_names.insert(package.manifest.name.clone(), (target, alias.clone()))
                && other != target
            {
                return Err(format!("package `{}`: two different packages are named `{}` in its direct dependencies `{old_alias}` and `{alias}`: {} and {}; each manifest may directly use one resolved instance per declared package name", manifest.name, package.manifest.name, describe_source(&self.packages[other]), describe_source(package)).into());
            }
            if package.manifest.library.is_none() {
                return Err(format!(
                    "package `{}`: dependency `{alias}` (package `{}`) has no library target",
                    manifest.name, package.manifest.name
                )
                .into());
            }
            if let Some(requirement) = &dependency.version {
                let version = manifest::parse_version(&package.manifest.version).unwrap();
                if !requirement.matches(version) {
                    return Err(format!(
                        "package `{}` requires `{alias}` {requirement}, but {} is version {}",
                        manifest.name,
                        describe_source(package),
                        package.manifest.version
                    )
                    .into());
                }
            }
            dependencies.push((alias.clone(), target));
        }
        chain.pop();
        self.packages[index].dependencies = dependencies;
        resolver.visiting.remove(&identity);
        Ok(index)
    }

    /// Locate one dependency, then visit it.
    fn dependency(
        &mut self,
        owner_index: usize,
        alias: &str,
        dependency: &Dependency,
        chain: &mut Vec<String>,
        resolver: &mut Resolver,
        previous: Option<&str>,
    ) -> Result<usize> {
        let owner = chain.last().unwrap().clone();
        let directory = self.packages[owner_index].directory.clone();
        let source = self.packages[owner_index].source.clone();
        let owner_id = self.packages[owner_index].identity.serialized(&self.root);
        let old_target =
            resolver
                .pins
                .edge_target(previous.unwrap_or(&owner_id), alias, dependency);
        let old_context =
            resolver
                .pins
                .edge_context(previous.unwrap_or(&owner_id), alias, dependency);
        let selected = (previous.unwrap_or(&owner_id).to_owned(), alias.to_owned());
        let explicitly_updated = resolver.selected_edges.contains(&selected);
        if explicitly_updated {
            resolver.used_updates.insert(selected);
        }
        let update = resolver.options.update == Update::All || explicitly_updated;
        let (path, mut target_source) = match (&dependency.source, &source) {
            (Source::Path(path), PackageSource::Local) => {
                (directory.join(path), PackageSource::Local)
            }
            // A path inside a Git package names another package of the same
            // repository snapshot.
            (
                Source::Path(path),
                PackageSource::Git {
                    url,
                    commit,
                    snapshot,
                    ..
                },
            ) => {
                let path = directory.join(path);
                let inside = path
                    .canonicalize()
                    .ok()
                    .and_then(|p| p.strip_prefix(snapshot).ok().map(Path::to_path_buf));
                let Some(relative) = inside else {
                    return Err(format!(
                        "package `{owner}`: path dependency `{alias}` leaves its repository {url}"
                    )
                    .into());
                };
                let subdir = relative;
                (
                    path,
                    PackageSource::Git {
                        url: url.clone(),
                        subdir: (!subdir.as_os_str().is_empty()).then_some(subdir),
                        commit: commit.clone(),
                        selectors: BTreeSet::new(),
                        snapshot: snapshot.clone(),
                    },
                )
            }
            (
                Source::Git {
                    url,
                    subdir,
                    selector,
                },
                _,
            ) => {
                let commit =
                    resolver.commit(dependency, &owner, alias, old_target.as_deref(), update)?;
                let snapshot = resolver.cache.checkout(url, &commit)?;
                let path = match subdir {
                    Some(subdir) => snapshot.join(subdir),
                    None => snapshot.clone(),
                };
                let mut selectors = BTreeSet::new();
                selectors.insert(selector.clone());
                (
                    path,
                    PackageSource::Git {
                        url: url.clone(),
                        subdir: subdir.as_ref().map(PathBuf::from),
                        commit,
                        selectors,
                        snapshot,
                    },
                )
            }
        };
        if !path.join(MANIFEST).is_file() {
            return Err(format!(
                "package `{owner}`: dependency `{alias}` at {} has no {MANIFEST}",
                describe_location(&path, &target_source)
            )
            .into());
        }
        let path = path.canonicalize()?;
        if let PackageSource::Git { snapshot, .. } = &target_source
            && !path.starts_with(snapshot)
        {
            return Err(format!(
                "package `{owner}`: dependency `{alias}` leaves its repository snapshot"
            )
            .into());
        }
        if let PackageSource::Git {
            subdir, snapshot, ..
        } = &mut target_source
        {
            let relative = path.strip_prefix(snapshot)?;
            *subdir = (!relative.as_os_str().is_empty()).then(|| relative.to_path_buf());
        }
        self.visit(None, path, target_source, chain, resolver, old_context)
    }

    pub fn member(&self, index: usize) -> &Package {
        &self.packages[index]
    }

    fn source_package(&self, index: usize) -> SourcePackage {
        let package = &self.packages[index];
        let source_identity = package.identity.serialized(&self.root);
        SourcePackage {
            identity: SourcePackage::resolved_identity(
                &package.manifest.name,
                &package.manifest.version,
                &source_identity,
            ),
            source_identity,
            provenance: Some(PackageProvenance {
                name: package.manifest.name.clone(),
                version: package.manifest.version.clone(),
                source: describe_source(package),
                reached: package.reached.clone(),
            }),
            test_of: None,
            root: package.directory.join("src"),
            directory: Some(package.directory.clone()),
            library: package
                .manifest
                .library
                .as_ref()
                .map(|library| package.directory.join(library)),
            dependencies: package.dependencies.clone(),
        }
    }

    /// The compiler's view, with package `entry` first.
    pub fn source_graph(&self, entry: usize) -> SourceGraph {
        let mut packages: Vec<_> = (0..self.packages.len())
            .map(|i| self.source_package(i))
            .collect();
        packages.swap(0, entry);
        let remap = |i: usize| {
            if i == 0 {
                entry
            } else if i == entry {
                0
            } else {
                i
            }
        };
        for package in &mut packages {
            for (_, target) in &mut package.dependencies {
                *target = remap(*target);
            }
        }
        SourceGraph { packages }
    }

    /// Integration tests of `member` form their own entry package rooted at
    /// its `tests/`. They see the member's dependencies and the member
    /// itself, under its package name.
    pub fn test_graph(&self, member: usize) -> SourceGraph {
        let mut graph = self.source_graph(member);
        let package = &self.packages[member];
        let mut dependencies: Vec<_> = graph.packages[0]
            .dependencies
            .iter()
            .map(|(alias, target)| (alias.clone(), target + 1))
            .collect();
        if package.manifest.library.is_some() {
            dependencies.push((package.manifest.name.clone(), 1));
        }
        for package in &mut graph.packages {
            for (_, target) in &mut package.dependencies {
                *target += 1;
            }
        }
        let owner = graph.packages[0].identity.clone();
        let source_identity = format!("tests:{}", package.identity.serialized(&self.root));
        graph.packages.insert(
            0,
            SourcePackage {
                identity: SourcePackage::resolved_identity(
                    &format!("{}_tests", package.manifest.name),
                    &package.manifest.version,
                    &source_identity,
                ),
                source_identity,
                provenance: None,
                test_of: Some(owner),
                root: package.directory.join("tests"),
                directory: Some(package.directory.clone()),
                library: None,
                dependencies,
            },
        );
        graph
    }

    /// The lockfile text for this graph: deterministic, and independent of
    /// where the project is checked out.
    pub fn lockfile(&self) -> String {
        lock::serialize(self)
    }

    /// Bring `plenty.lock` up to date, or with `locked` require that it is.
    pub fn sync_lockfile(&self, locked: bool) -> Result<()> {
        let path = self.root.join(LOCKFILE);
        let expected = self.lockfile();
        let current = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("reading {}: {e}", path.display()).into()),
        };
        if current.as_deref() == Some(expected.as_str()) {
            return Ok(());
        }
        if locked {
            return Err(match current {
                None => format!("{LOCKFILE} is missing and --locked forbids creating it"),
                Some(_) => format!(
                    "{LOCKFILE} does not match the dependencies in {MANIFEST}, and --locked forbids updating it"
                ),
            }
            .into());
        }
        write_atomically(&path, expected.as_bytes())
    }
}

impl Resolver {
    fn new(options: &ResolveOptions, pins: Pins, cache: git::Cache) -> Self {
        Self {
            options: options.clone(),
            pins,
            cache,
            identities: BTreeMap::new(),
            visiting: BTreeSet::new(),
            fresh: BTreeMap::new(),
            selected_edges: BTreeSet::new(),
            used_updates: BTreeSet::new(),
        }
    }

    fn commit(
        &mut self,
        dependency: &Dependency,
        owner: &str,
        alias: &str,
        old_target: Option<&str>,
        update: bool,
    ) -> Result<String> {
        let Source::Git {
            url,
            subdir,
            selector,
        } = &dependency.source
        else {
            unreachable!()
        };
        if !update
            && let Some(commit) = old_target
                .and_then(|id| self.pins.packages.get(id))
                .and_then(|p| p.commit.as_ref())
        {
            return Ok(commit.clone());
        }
        let key = (
            url.to_owned(),
            subdir.clone().unwrap_or_default(),
            selector.to_string(),
        );
        if let Some(commit) = self.fresh.get(&key) {
            return Ok(commit.clone());
        }
        if self.options.locked && !self.pins.exists {
            return Err(format!("{LOCKFILE} is missing and --locked forbids creating it").into());
        }
        if self.options.locked {
            return Err(format!("{LOCKFILE} pins no commit for dependency `{alias}` of `{owner}` ({url} {selector}), and --locked forbids resolving it").into());
        }
        let commit = self
            .cache
            .resolve(url, selector)
            .map_err(|e| format!("dependency `{alias}` of `{owner}`: {e}"))?;
        self.fresh.insert(key, commit.clone());
        Ok(commit)
    }
}

fn describe_location(directory: &Path, source: &PackageSource) -> String {
    match source {
        PackageSource::Local => directory.display().to_string(),
        PackageSource::Git {
            url,
            subdir,
            commit,
            ..
        } => format!(
            "{url}{} at {}",
            subdir
                .as_ref()
                .map(|s| format!(" ({})", s.display()))
                .unwrap_or_default(),
            &commit[..12]
        ),
    }
}

fn describe_source(package: &Package) -> String {
    describe_location(&package.directory, &package.source)
}

/// `to` relative to `from`, both canonical, with `/` separators.
fn relative_path(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// Write beside the destination, then rename, so an interruption never
/// leaves a partially written file.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path.parent().unwrap();
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| format!("writing {}: {}", path.display(), e.error))?;
    Ok(())
}

/// Options shared by the commands that compile a project.
#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub profile: Profile,
    /// Refuse to create or change `plenty.lock`.
    pub locked: bool,
    /// Never fetch; use only `.pkg/`.
    pub offline: bool,
    pub compile: crate::CompileOptions,
    /// `--cpu`, which overrides each package's `[build] cpu`.
    pub cpu: Option<crate::CpuLevel>,
    /// Used only by `plenty check`, which never reuses an earlier result.
    pub check: crate::CheckOptions,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            profile: Profile::Optimized,
            locked: false,
            offline: false,
            compile: crate::CompileOptions::default(),
            cpu: None,
            check: crate::CheckOptions::default(),
        }
    }
}

fn open(directory: &Path, options: &BuildOptions) -> Result<Project> {
    let project = Project::load(
        directory,
        &ResolveOptions {
            locked: options.locked,
            offline: options.offline,
            update: Update::None,
        },
    )?;
    project.sync_lockfile(options.locked)?;
    Ok(project)
}

/// Type-check `entry`, a file of `graph.packages[0]`, as an application
/// (`require_main`) or a library module.
pub fn check_entry(
    entry: &Path,
    graph: &SourceGraph,
    require_main: bool,
    options: &crate::CheckOptions,
) -> Result<Vec<crate::Warning>> {
    check_sources(entry, graph, require_main, options).map(|(warnings, ..)| warnings)
}

/// [`check_entry`], also returning what the check read, in all packages of
/// `graph`.
fn check_sources(
    entry: &Path,
    graph: &SourceGraph,
    require_main: bool,
    options: &crate::CheckOptions,
) -> Result<(Vec<crate::Warning>, crate::frontend::Inputs)> {
    check_program(options, |heap| {
        crate::frontend::compile_graph(entry, graph, require_main, heap, *options)
    })
}

/// Check the operations of the program `compile` lowers, returning its lint
/// warnings and what it read.
fn check_program(
    options: &crate::CheckOptions,
    compile: impl FnOnce(&mut crate::value::Heap) -> Result<crate::frontend::Program>,
) -> Result<(Vec<crate::Warning>, crate::frontend::Inputs)> {
    let mut heap = crate::value::Heap::default();
    let mut program = compile(&mut heap)?;
    let checked = crate::op::check(&program.ops);
    let warnings = std::mem::take(&mut program.warnings);
    let inputs = std::mem::take(&mut program.inputs);
    crate::teardown((program, heap), options);
    checked.map(|()| (warnings, inputs))
}

fn status(verb: &str, message: impl std::fmt::Display) {
    eprintln!("{verb:>10} {message}");
}

fn title(package: &Package) -> String {
    format!("{} v{}", package.manifest.name, package.manifest.version)
}

/// Check every target of the selected packages, printing each package's lint
/// warnings once on stderr after its targets pass. A target's check covers the
/// files its import graph reaches, so afterwards each `.plenty` or `.p` file below the
/// package's `src/` that no target reached is reported as unchecked. With
/// `tests`, each package's test files are also checked as `plenty test` would
/// compile them, without linking or running them.
pub fn check(directory: &Path, options: &BuildOptions, tests: bool) -> Result<()> {
    let project = open(directory, options)?;
    let mut test_files = vec![];
    if tests {
        // Discovery reads every selected package's tests before any check,
        // as `plenty test` does, so a malformed test file fails early.
        for &member in &project.selected {
            discover_tests(&project, member, false, &mut test_files, &mut vec![])?;
        }
    }
    for &member in &project.selected {
        let package = project.member(member);
        status("Checking", title(package));
        let graph = project.source_graph(member);
        // Targets share modules, so the same warning can come from each.
        let mut warnings = Vec::new();
        let mut reached = BTreeSet::new();
        let entries = package
            .manifest
            .library
            .iter()
            .map(|library| (library, false))
            .chain(
                package
                    .manifest
                    .binaries
                    .iter()
                    .map(|(_, entry)| (entry, true)),
            );
        for (entry, require_main) in entries {
            let (found, inputs) = check_sources(
                &package.directory.join(entry),
                &graph,
                require_main,
                &options.check,
            )?;
            warnings.extend(found);
            reached.extend(inputs.sources().map(Path::to_path_buf));
        }
        let member_tests: Vec<_> = test_files.iter().filter(|t| t.member == member).collect();
        if !member_tests.is_empty() {
            let graph = project.test_graph(member);
            for TestSource { path, kind, .. } in &member_tests {
                let appendix = match kind {
                    crate::frontend::TestFile::Program => None,
                    crate::frontend::TestFile::Functions(functions) => {
                        Some(crate::frontend::test_appendix(functions))
                    }
                };
                let (found, _) = check_program(&options.check, |heap| {
                    crate::frontend::compile_test_graph(
                        path,
                        &graph,
                        appendix.as_deref(),
                        heap,
                        options.check,
                    )
                })?;
                warnings.extend(found);
            }
        }
        crate::diagnostic::settle(&mut warnings);
        for warning in &warnings {
            eprintln!("{warning}");
        }
        let mut sources = vec![];
        source_files(&package.directory.join("src"), &mut sources)?;
        sources.sort();
        let unreached: Vec<_> = sources.iter().filter(|s| !reached.contains(*s)).collect();
        for path in &unreached {
            eprintln!(
                "warning: {} is not imported by any target of `{}`; it was not checked",
                relative_path(&project.root, path),
                package.manifest.name
            );
        }
        status(
            "Checked",
            format!(
                "{}: {}{}; {} of {} files in src/",
                title(package),
                describe_targets(&package.manifest),
                match (tests, member_tests.len()) {
                    (false, _) => String::new(),
                    (true, 1) => ", 1 test file".into(),
                    (true, n) => format!(", {n} test files"),
                },
                sources.len() - unreached.len(),
                sources.len()
            ),
        );
    }
    Ok(())
}

/// The targets `plenty check` checks, such as `library and 2 executables`.
fn describe_targets(manifest: &Manifest) -> String {
    let executables = match manifest.binaries.len() {
        0 => None,
        1 => Some("1 executable".to_string()),
        n => Some(format!("{n} executables")),
    };
    match (manifest.library.is_some(), executables) {
        (true, Some(executables)) => format!("library and {executables}"),
        (true, None) => "library".into(),
        (false, Some(executables)) => executables,
        (false, None) => unreachable!("a manifest declares at least one target"),
    }
}

/// The canonical paths of the `.plenty`/`.p` files below `directory`.
fn source_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            source_files(&path, files)?;
        } else if path.is_file() && crate::source_path::is_source(&path) {
            files.push(path.canonicalize()?);
        }
    }
    Ok(())
}

fn output_directory(project: &Project, profile: Profile) -> PathBuf {
    project.root.join("target").join(profile.name())
}

/// The options for compiling `package`'s executables and tests. Only the
/// package being built chooses its CPU level; dependencies are compiled into
/// its program at that level.
fn compile_options(options: &BuildOptions, package: &Package) -> crate::CompileOptions {
    let mut compile = options.compile.clone();
    compile.optimize = options.profile == Profile::Optimized;
    compile.cpu = options.cpu.or(package.manifest.cpu).unwrap_or_default();
    compile
}

/// Prints the package's status line before its first stale target, or
/// `Fresh` at the end if every target was reused.
struct Progress<'a> {
    package: &'a Package,
    profile: Profile,
    started: bool,
}

impl Progress<'_> {
    fn building(&mut self) {
        if !self.started {
            self.started = true;
            status(
                "Compiling",
                format!("{} ({})", title(self.package), self.profile.name()),
            );
        }
    }

    fn finish(self) {
        if !self.started {
            status(
                "Fresh",
                format!("{} ({})", title(self.package), self.profile.name()),
            );
        }
    }
}

/// Run `build` unless the fingerprint says `output` is current.
fn reuse_or_build(
    fingerprint: Fingerprint,
    output: Option<&Path>,
    graph: &SourceGraph,
    progress: &mut Progress,
    build: impl FnOnce() -> Result<crate::frontend::Inputs>,
) -> Result<()> {
    if fingerprint.fresh(output, graph) {
        return Ok(());
    }
    progress.building();
    fingerprint.invalidate()?;
    let inputs = build()?;
    fingerprint.record(&inputs)
}

/// Check the library and build each executable of `member`, reusing
/// up-to-date outputs. Returns `(name, path)` for each executable.
fn build_member(
    project: &Project,
    member: usize,
    only: Option<&str>,
    options: &BuildOptions,
) -> Result<Vec<(String, PathBuf)>> {
    let package = project.member(member);
    let directory = output_directory(project, options.profile);
    std::fs::create_dir_all(&directory)?;
    let graph = project.source_graph(member);
    let compile = compile_options(options, package);
    let mut progress = Progress {
        package,
        profile: options.profile,
        started: false,
    };
    if only.is_none()
        && let Some(library) = &package.manifest.library
    {
        let entry = package.directory.join(library);
        let digest = fingerprint::digest(&graph, &entry, "library", &compile)?;
        reuse_or_build(
            Fingerprint::new(
                &directory,
                &format!("lib-{}", package.manifest.name),
                digest,
            ),
            None,
            &graph,
            &mut progress,
            || {
                check_sources(&entry, &graph, false, &crate::CheckOptions::default())
                    .map(|(_, inputs)| inputs)
            },
        )?;
    }
    let mut outputs = vec![];
    for (name, entry) in &package.manifest.binaries {
        if only.is_some_and(|only| only != name) {
            continue;
        }
        let entry = package.directory.join(entry);
        let output = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        let digest = fingerprint::digest(&graph, &entry, "executable", &compile)?;
        reuse_or_build(
            Fingerprint::new(&directory, &format!("bin-{name}"), digest),
            Some(&output),
            &graph,
            &mut progress,
            || crate::codegen::compile_graph_to_executable(&entry, &graph, &output, &compile),
        )?;
        outputs.push((name.clone(), output));
    }
    progress.finish();
    Ok(outputs)
}

/// Executables share `target/PROFILE/`, so their names must be unique
/// across the selected packages.
fn executables(project: &Project) -> Result<Vec<(usize, String)>> {
    let mut all: Vec<(usize, String)> = vec![];
    for &member in &project.selected {
        for (name, _) in &project.member(member).manifest.binaries {
            if let Some((other, _)) = all.iter().find(|(_, n)| n == name) {
                return Err(format!(
                    "packages `{}` and `{}` both define an executable named `{name}`",
                    project.member(*other).manifest.name,
                    project.member(member).manifest.name
                )
                .into());
            }
            all.push((member, name.clone()));
        }
    }
    Ok(all)
}

/// Build every executable target of the selected packages into
/// `target/PROFILE/`, and check their libraries. Returns the executables'
/// paths.
pub fn build(directory: &Path, options: &BuildOptions) -> Result<Vec<PathBuf>> {
    let project = open(directory, options)?;
    executables(&project)?;
    let mut outputs = vec![];
    for &member in &project.selected {
        outputs.extend(
            build_member(&project, member, None, options)?
                .into_iter()
                .map(|(_, path)| path),
        );
    }
    Ok(outputs)
}

/// Build one executable target and run it with `arguments`, inheriting
/// standard streams, environment, and working directory. Returns the child's
/// exit status.
pub fn run(
    directory: &Path,
    binary: Option<&str>,
    arguments: &[String],
    options: &BuildOptions,
) -> Result<ExitCode> {
    let project = open(directory, options)?;
    let all = executables(&project)?;
    let names = || {
        all.iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let packages = || {
        project
            .selected
            .iter()
            .map(|&m| format!("`{}`", project.member(m).manifest.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (member, name) = match binary {
        Some(wanted) => all
            .iter()
            .find(|(_, name)| name == wanted)
            .ok_or_else(|| format!("no executable target `{wanted}`; available: {}", names()))?,
        None => match all.as_slice() {
            [only] => only,
            [] => {
                let noun = if project.selected.len() == 1 {
                    "package"
                } else {
                    "packages"
                };
                return Err(
                    format!("{noun} {} has no executable target to run", packages()).into(),
                );
            }
            _ => {
                return Err(format!(
                    "{} has several executables; choose one with --bin NAME: {}",
                    packages(),
                    names()
                )
                .into());
            }
        },
    };
    let (_, executable) = build_member(&project, *member, Some(name), options)?
        .pop()
        .unwrap();
    status("Running", relative_path(&project.root, &executable));
    let status = Command::new(&executable)
        .args(arguments)
        .status()
        .map_err(|e| format!("running {}: {e}", executable.display()))?;
    Ok(exit_code(status))
}

/// A child's exit status as this process's: Unix signal termination becomes
/// 128 plus the signal number.
pub fn exit_code(status: std::process::ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return ExitCode::from(code as u8);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return ExitCode::from((128 + signal) as u8);
        }
    }
    ExitCode::FAILURE
}

/// The outcome of [`test`].
#[derive(Debug, Default)]
pub struct TestSummary {
    pub passed: Vec<String>,
    pub failed: Vec<String>,
    /// Tests that a filter excluded.
    pub filtered: usize,
}

/// Options for [`test`].
#[derive(Clone, Debug, Default)]
pub struct TestOptions {
    pub build: BuildOptions,
    /// Run only tests whose name contains one of these; all when empty.
    pub filters: Vec<String>,
    /// Let tests write straight to the terminal instead of capturing their
    /// output and showing it only when they fail.
    pub no_capture: bool,
    /// Passed to every test as `PLENTY_TEST_SEED`, which `std.test.seed`
    /// reads.
    pub seed: Option<u64>,
    /// Print the selected tests' names instead of running them.
    pub list: bool,
    /// Most tests running at once; `None` uses the available parallelism.
    pub jobs: Option<std::num::NonZeroUsize>,
    /// Kill a test that runs longer than this and fail it.
    pub timeout: Option<std::time::Duration>,
}

/// One test process that has not been reaped yet.
struct Running {
    case: usize,
    child: std::process::Child,
    log: Option<std::fs::File>,
    started: std::time::Instant,
}

/// How a test process ended.
enum Ending {
    Exited(std::process::ExitStatus),
    TimedOut(std::time::Duration),
}

/// One test: a whole program, or one function of a file of test functions.
struct TestCase {
    name: String,
    member: usize,
    file: usize,
    function: Option<String>,
}

/// A test file, what it defines, and its package.
struct TestSource {
    path: PathBuf,
    kind: crate::frontend::TestFile,
    member: usize,
}

/// Collect the tests in `member`'s `tests/`. A file that defines `main` is
/// one test named after the file; otherwise each `test_*` function is a test
/// named `FILE::FUNCTION`. Files in subdirectories are helper modules. In a
/// command that tests several packages, names start with `PACKAGE/`.
fn discover_tests(
    project: &Project,
    member: usize,
    prefix: bool,
    files: &mut Vec<TestSource>,
    cases: &mut Vec<TestCase>,
) -> Result<()> {
    let package = project.member(member);
    let tests_directory = package.directory.join("tests");
    let mut paths = vec![];
    if tests_directory.is_dir() {
        for entry in std::fs::read_dir(&tests_directory)? {
            let path = entry?.path();
            if path.is_file() && crate::source_path::is_source(&path) {
                paths.push(path);
            }
        }
    }
    paths.sort();
    let mut stems = BTreeMap::new();
    for path in &paths {
        if let Some(previous) = stems.insert(path.file_stem().unwrap(), path) {
            return Err(format!(
                "ambiguous test module: {} and {}",
                previous.display(),
                path.display()
            )
            .into());
        }
    }
    for path in paths {
        let kind = crate::frontend::discover_tests(&path)?;
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let stem = if prefix {
            format!("{}/{stem}", package.manifest.name)
        } else {
            stem
        };
        let file = files.len();
        match &kind {
            crate::frontend::TestFile::Program => cases.push(TestCase {
                name: stem,
                member,
                file,
                function: None,
            }),
            crate::frontend::TestFile::Functions(functions) => {
                for (function, _) in functions {
                    cases.push(TestCase {
                        name: format!("{stem}::{function}"),
                        member,
                        file,
                        function: Some(function.clone()),
                    });
                }
            }
        }
        files.push(TestSource { path, kind, member });
    }
    Ok(())
}

/// Compile the selected tests and run each in its own process from its
/// package directory. A test passes when its process exits with status zero.
pub fn test(directory: &Path, options: &TestOptions) -> Result<TestSummary> {
    let project = open(directory, &options.build)?;
    let mut files = vec![];
    let mut cases = vec![];
    let prefix = project.selected.len() > 1;
    for &member in &project.selected {
        discover_tests(&project, member, prefix, &mut files, &mut cases)?;
    }
    let total = cases.len();
    let cases: Vec<_> = cases
        .into_iter()
        .filter(|case| {
            options.filters.is_empty() || options.filters.iter().any(|f| case.name.contains(f))
        })
        .collect();
    let mut summary = TestSummary {
        filtered: total - cases.len(),
        ..TestSummary::default()
    };
    if options.list {
        for case in &cases {
            println!("{}", case.name);
        }
        return Ok(summary);
    }
    let output = output_directory(&project, options.build.profile);
    let mut executables = vec![None; files.len()];
    for &member in &project.selected {
        let package = project.member(member);
        let compile = compile_options(&options.build, package);
        let mut progress = Progress {
            package,
            profile: options.build.profile,
            started: false,
        };
        let graph = project.test_graph(member);
        let directory = output.join("tests").join(&package.manifest.name);
        std::fs::create_dir_all(&directory)?;
        for case in cases.iter().filter(|c| c.member == member) {
            if executables[case.file].is_some() {
                continue;
            }
            let TestSource { path, kind, .. } = &files[case.file];
            let stem = path.file_stem().unwrap().to_string_lossy();
            let executable = directory.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX));
            let digest = fingerprint::digest(&graph, path, "test", &compile)?;
            reuse_or_build(
                Fingerprint::new(&directory, &stem, digest),
                Some(&executable),
                &graph,
                &mut progress,
                || match kind {
                    crate::frontend::TestFile::Program => {
                        crate::codegen::compile_test_program_to_executable(
                            path,
                            &graph,
                            &executable,
                            &compile,
                        )
                    }
                    crate::frontend::TestFile::Functions(functions) => {
                        crate::codegen::compile_tests_to_executable(
                            path,
                            &graph,
                            functions,
                            &executable,
                            &compile,
                        )
                    }
                },
            )?;
            executables[case.file] = Some(executable);
        }
        progress.finish();
    }
    let plural = if cases.len() == 1 { "" } else { "s" };
    println!("running {} test{plural}", cases.len());
    let jobs = options
        .jobs
        .or_else(|| std::thread::available_parallelism().ok())
        .map_or(1, std::num::NonZeroUsize::get);
    // Each test gets its own TMPDIR inside this directory, removed when the
    // test ends, so a test that traps or times out leaves no scratch files.
    let temporary = tempfile::Builder::new()
        .prefix("plenty-test-run-")
        .tempdir()?;
    let mut endings: Vec<Option<(Ending, Vec<u8>)>> = (0..cases.len()).map(|_| None).collect();
    let mut running: Vec<Running> = vec![];
    let mut started = 0;
    let mut reported = 0;
    let mut failures = vec![];
    while reported < cases.len() {
        while running.len() < jobs && started < cases.len() {
            let case = &cases[started];
            let executable = executables[case.file].as_ref().unwrap();
            let scratch = temporary.path().join(started.to_string());
            std::fs::create_dir(&scratch)?;
            let mut command = Command::new(executable);
            command
                .current_dir(&project.member(files[case.file].member).directory)
                .env("TMPDIR", &scratch)
                .stdin(std::process::Stdio::null());
            if let Some(function) = &case.function {
                command.arg(function);
            }
            if let Some(seed) = options.seed {
                command.env("PLENTY_TEST_SEED", seed.to_string());
            }
            let log = if options.no_capture {
                None
            } else {
                // One file for both streams keeps their interleaving, and
                // unlike a pipe it cannot fill up and stall the test.
                let log = tempfile::tempfile()?;
                command.stdout(log.try_clone()?).stderr(log.try_clone()?);
                Some(log)
            };
            let child = command
                .spawn()
                .map_err(|e| format!("running {}: {e}", executable.display()))?;
            running.push(Running {
                case: started,
                child,
                log,
                started: std::time::Instant::now(),
            });
            started += 1;
        }
        let mut index = 0;
        while index < running.len() {
            let test = &mut running[index];
            let ending = match test.child.try_wait()? {
                Some(status) => Ending::Exited(status),
                None => match options.timeout {
                    Some(limit) if test.started.elapsed() >= limit => {
                        // Descendants the test started are not killed.
                        drop(test.child.kill());
                        test.child.wait()?;
                        Ending::TimedOut(limit)
                    }
                    _ => {
                        index += 1;
                        continue;
                    }
                },
            };
            let mut test = running.swap_remove(index);
            let mut captured = vec![];
            if let Some(log) = &mut test.log {
                std::io::Seek::rewind(log)?;
                std::io::Read::read_to_end(log, &mut captured)?;
            }
            drop(std::fs::remove_dir_all(
                temporary.path().join(test.case.to_string()),
            ));
            endings[test.case] = Some((ending, captured));
        }
        // Report in discovery order, so output does not depend on timing.
        while let Some(Some((ending, captured))) = endings.get_mut(reported).map(Option::take) {
            let case = &cases[reported];
            let reason = match ending {
                Ending::Exited(status) if status.success() => None,
                Ending::Exited(status)
                    if case.function.is_some()
                        && status.code() == Some(crate::frontend::RETURNED_ERR) =>
                {
                    Some("returned Err".to_owned())
                }
                Ending::Exited(status) => Some(describe(status)),
                Ending::TimedOut(limit) => Some(format!("timed out after {}s", limit.as_secs())),
            };
            match reason {
                None => {
                    println!("test {} ... ok", case.name);
                    summary.passed.push(case.name.clone());
                }
                Some(reason) => {
                    println!("test {} ... FAILED ({reason})", case.name);
                    summary.failed.push(case.name.clone());
                    failures.push((case.name.clone(), captured));
                }
            }
            reported += 1;
        }
        if reported < cases.len() && !running.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    if !options.no_capture && failures.iter().any(|(_, output)| !output.is_empty()) {
        println!("\nfailures:");
        for (name, captured) in &failures {
            if captured.is_empty() {
                continue;
            }
            println!("\n---- {name} output ----");
            print!("{}", String::from_utf8_lossy(captured));
            if !captured.ends_with(b"\n") {
                println!();
            }
        }
    }
    let filtered = if options.filters.is_empty() {
        String::new()
    } else {
        format!("; {} filtered out", summary.filtered)
    };
    println!(
        "\ntest result: {}. {} passed; {} failed{filtered}",
        if summary.failed.is_empty() {
            "ok"
        } else {
            "FAILED"
        },
        summary.passed.len(),
        summary.failed.len()
    );
    Ok(summary)
}

fn describe(status: std::process::ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit status {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("terminated by signal {signal}");
        }
    }
    "terminated abnormally".into()
}

/// Remove `target/` from the workspace or package at or above `directory`.
pub fn clean(directory: &Path) -> Result<()> {
    let root = layout(directory)?.root;
    let target = root.join("target");
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .map_err(|e| format!("removing {}: {e}", target.display()))?;
    }
    Ok(())
}

/// Resolve and snapshot every dependency, writing `plenty.lock` if needed,
/// without compiling.
pub fn fetch(directory: &Path, options: &BuildOptions) -> Result<()> {
    open(directory, options).map(|_| ())
}

/// Resolve Git tags and revisions again, for every package or only the
/// named ones, and rewrite `plenty.lock`. Returns `(name, old, new)` commit
/// changes, abbreviated.
pub fn update(
    directory: &Path,
    packages: &[String],
    offline: bool,
) -> Result<Vec<(String, String, String)>> {
    let before = read_pins(&layout(directory)?.root.join(LOCKFILE))?;
    let project = Project::load(
        directory,
        &ResolveOptions {
            locked: false,
            offline,
            update: if packages.is_empty() {
                Update::All
            } else {
                Update::Packages(packages.to_vec())
            },
        },
    )?;
    project.sync_lockfile(false)?;
    let mut changes = vec![];
    for package in &project.packages {
        let owner = package.identity.serialized(&project.root);
        for (alias, target) in &package.dependencies {
            let target_package = &project.packages[*target];
            if let PackageSource::Git { commit, .. } = &target_package.source {
                let old = before
                    .packages
                    .get(&owner)
                    .and_then(|p| p.edges.get(alias))
                    .and_then(|e| before.packages.get(&e.target))
                    .and_then(|p| p.commit.as_ref());
                if old != Some(commit) {
                    eprintln!(
                        "Edge {owner}::{alias} now uses {} {} ({})",
                        target_package.manifest.name,
                        target_package.manifest.version,
                        target_package.identity.serialized(&project.root)
                    );
                    changes.push((
                        target_package.manifest.name.clone(),
                        old.map(|c| c[..12].to_owned()).unwrap_or_default(),
                        commit[..12].to_owned(),
                    ));
                }
            }
        }
    }
    for (id, package) in &before.packages {
        if !project
            .packages
            .iter()
            .any(|p| p.identity.serialized(&project.root) == *id)
        {
            eprintln!("Removing unreachable {} ({id})", package.name);
        }
    }
    Ok(changes)
}

/// The `plentyc --package-graph` file for the selected package.
pub fn graph(directory: &Path, options: &BuildOptions) -> Result<String> {
    let project = open(directory, options)?;
    let [member] = project.selected.as_slice() else {
        return Err(
            "a workspace root selects several packages; run `plenty graph` in one member's directory"
                .into(),
        );
    };
    Ok(project.source_graph(*member).to_toml())
}

/// Add a dependency to the manifest of the package containing `directory`,
/// as the last entry of its `[dependencies]` table, then resolve and lock
/// the graph. `alias` defaults to the dependency's package name. On any
/// error the manifest is left unchanged. Returns the alias.
pub fn add(
    directory: &Path,
    alias: Option<&str>,
    mut dependency: Dependency,
    offline: bool,
) -> Result<String> {
    let package_directory = discover(directory)?;
    let manifest_path = package_directory.join(MANIFEST);
    let original = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
    let manifest = read_manifest(&package_directory)?;
    if let Source::Path(path) = &mut dependency.source {
        // The command line's path is relative to where the command runs;
        // the manifest's is relative to the manifest.
        let absolute = directory.join(&*path);
        let absolute = absolute
            .canonicalize()
            .map_err(|e| format!("{}: {e}", absolute.display()))?;
        *path = PathBuf::from(relative_path(&package_directory, &absolute));
    }
    let mut cache = git::Cache::new(layout(directory)?.root.join(PACKAGE_CACHE), offline);
    let alias = match alias {
        Some(alias) => alias.to_owned(),
        None => match &dependency.source {
            Source::Path(path) => read_manifest(&package_directory.join(path))?.name,
            Source::Git {
                url,
                subdir,
                selector,
            } => {
                let commit = cache.resolve(url, selector)?;
                let snapshot = cache.checkout(url, &commit)?;
                let path = match subdir {
                    Some(subdir) => snapshot.join(subdir),
                    None => snapshot,
                };
                if !path.join(MANIFEST).is_file() {
                    return Err(format!(
                        "{url} {selector}{} has no {MANIFEST}",
                        subdir
                            .as_ref()
                            .map(|s| format!(" ({s})"))
                            .unwrap_or_default()
                    )
                    .into());
                }
                read_manifest(&path)?.name
            }
        },
    };
    if manifest.dependencies.contains_key(&alias) {
        return Err(format!(
            "package `{}` already has a dependency named `{alias}`",
            manifest.name
        )
        .into());
    }
    let text =
        manifest::insert_dependency(&original, &manifest::dependency_line(&alias, &dependency));
    manifest::parse(
        &text,
        &manifest_path.display().to_string(),
        &package_directory,
    )?;
    write_atomically(&manifest_path, text.as_bytes())?;
    let resolved = Project::load_with(
        directory,
        &ResolveOptions {
            locked: false,
            offline,
            update: Update::None,
        },
        Some(cache),
    )
    .and_then(|project| project.sync_lockfile(false));
    if let Err(error) = resolved {
        write_atomically(&manifest_path, original.as_bytes())?;
        return Err(error);
    }
    Ok(alias)
}

/// The kind of package `plenty new` and `plenty init` create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    Application,
    Library,
}

/// Create a package in the existing, manifest-free `directory`.
pub fn init(directory: &Path, name: Option<&str>, template: Template) -> Result<()> {
    let name = match name {
        Some(name) => name.to_owned(),
        None => directory
            .canonicalize()?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    if !manifest::valid_name(&name) || name == "std" {
        return Err(format!(
            "package name {name:?} must be a lowercase identifier other than `std`; choose one with --name NAME"
        )
        .into());
    }
    let manifest_path = directory.join(MANIFEST);
    if manifest_path.exists() {
        return Err(format!("{} already exists", manifest_path.display()).into());
    }
    let (file, source) = match template {
        Template::Application => (
            "src/main.plenty",
            format!(
                "def main() -> Result[(), IoError]:\n    print(\"Hello from {name}!\")?\n    Ok(())\n"
            ),
        ),
        Template::Library => (
            "src/lib.plenty",
            "pub def add(left: i64, right: i64) -> Result[i64, OverflowError[i64]]:\n    Ok(left + right)\n".to_string(),
        ),
    };
    let source_path = directory.join(file);
    for extension in ["plenty", "p"] {
        let existing = source_path.with_extension(extension);
        if existing.exists() {
            return Err(format!("{} already exists", existing.display()).into());
        }
    }
    std::fs::create_dir_all(directory.join("src"))?;
    std::fs::write(&source_path, source)?;
    std::fs::write(&manifest_path, manifest::template(&name))?;
    let ignore = directory.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, "/target\n/.pkg\n")?;
    }
    Ok(())
}

/// Create `directory` and a package inside it.
pub fn new(directory: &Path, name: Option<&str>, template: Template) -> Result<()> {
    if directory.exists() {
        return Err(format!("{} already exists", directory.display()).into());
    }
    std::fs::create_dir_all(directory)?;
    init(directory, name, template)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/a/c/d")),
            "../c/d"
        );
        assert_eq!(relative_path(Path::new("/a"), Path::new("/a/b")), "b");
        assert_eq!(relative_path(Path::new("/a"), Path::new("/a")), ".");
    }

    #[test]
    fn graph_files_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let graph = SourceGraph {
            packages: vec![
                SourcePackage {
                    identity: "app".into(),
                    source_identity: String::new(),
                    provenance: None,
                    test_of: None,
                    root: dir.path().join("app/src"),
                    directory: Some(dir.path().join("app")),
                    library: None,
                    dependencies: vec![("geo".into(), 1)],
                },
                SourcePackage {
                    identity: "shapes".into(),
                    source_identity: String::new(),
                    provenance: None,
                    test_of: None,
                    root: dir.path().join("shapes/src"),
                    directory: Some(dir.path().join("shapes")),
                    library: Some(dir.path().join("shapes/src/lib.plenty")),
                    dependencies: vec![],
                },
            ],
        };
        let path = dir.path().join("graph.toml");
        std::fs::write(&path, graph.to_toml()).unwrap();
        let read = SourceGraph::read(&path).unwrap();
        assert_eq!(read.to_toml(), graph.to_toml());

        std::fs::write(
            &path,
            "graph-version = 1\n[[package]]\nidentity = \"app\"\nroot = \"src\"\nlibrary = \"src/lib.plenty\"\n",
        )
        .unwrap();
        let relative = SourceGraph::read(&path).unwrap();
        assert_eq!(relative.packages[0].root, dir.path().join("src"));
        assert_eq!(
            relative.packages[0].library,
            Some(dir.path().join("src/lib.plenty"))
        );

        std::fs::write(
            &path,
            "graph-version = 1\n[[package]]\nidentity = \"app\"\nroot = \"src\"\ndependencies = { x = \"nope\" }\n",
        )
        .unwrap();
        let error = SourceGraph::read(&path).unwrap_err().to_string();
        assert!(error.contains("unknown identity `nope`"), "{error}");
    }
}
