//! Versioned, per-owner-edge pins. No mutable selector is a graph-wide pin.
use super::{
    Dependency, LOCKFILE, Manifest, PackageIdentity, PackageSource, Project, Result, Source,
    Update, manifest, toml,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Edge {
    pub target: String,
    pub requirement: Option<String>,
}
#[derive(Clone, Debug)]
pub(super) struct LockedPackage {
    pub name: String,
    pub version: String,
    pub source: String,
    pub subdir: PathBuf,
    pub commit: Option<String>,
    pub selectors: BTreeSet<String>,
    pub edges: BTreeMap<String, Edge>,
    pub member: bool,
}
#[derive(Default)]
pub(super) struct Pins {
    pub exists: bool,
    pub version: i64,
    pub packages: BTreeMap<String, LockedPackage>,
    pub root: PathBuf,
}

pub(super) fn requirement(dependency: &Dependency) -> String {
    let source = match &dependency.source {
        Source::Path(path) => {
            let (bytes, path) = super::identity::path_fields(Path::new(""), path);
            format!("path{}:{path}", if bytes { "-bytes" } else { "" })
        }
        Source::Git {
            url,
            subdir,
            selector,
        } => format!(
            "git:{}:{url}:{}:{}:{selector}",
            url.len(),
            subdir.as_deref().unwrap_or_default().len(),
            subdir.as_deref().unwrap_or_default()
        ),
    };
    format!(
        "{source};version:{}",
        dependency
            .version
            .map(|v| v.to_string())
            .unwrap_or_default()
    )
}
fn text(table: &toml::Table, key: &str) -> Option<String> {
    match table.get(key).map(|e| &e.value) {
        Some(toml::Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}
fn normalize_subdir(subdir: &str) -> PathBuf {
    Path::new(subdir).components().collect()
}
fn source_requirement(requirement: &str) -> &str {
    requirement
        .rsplit_once(";version:")
        .map_or(requirement, |(source, _)| source)
}
/// Decode the original edge context, which can differ from a canonical symlink target.
fn git_context(requirement: &str) -> Option<(String, PathBuf)> {
    fn field(input: &str) -> Option<(&str, &str)> {
        let (length, rest) = input.split_once(':')?;
        let length: usize = length.parse().ok()?;
        Some((rest.get(..length)?, rest.get(length..)?.strip_prefix(':')?))
    }
    let input = requirement.strip_prefix("git:")?;
    let (url, rest) = field(input)?;
    let (subdir, _) = field(rest)?;
    Some((url.to_owned(), normalize_subdir(subdir)))
}
fn full_commit(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn source_id(root: &Path, source: &str, subdir: &Path, commit: Option<&str>) -> Option<String> {
    if let Some(url) = source.strip_prefix("git+") {
        Some(
            PackageIdentity::Git {
                url: url.into(),
                subdir: subdir.to_path_buf(),
                commit: commit?.into(),
            }
            .serialized(root),
        )
    } else {
        let script = source.starts_with("script+") || source.starts_with("script-bytes+");
        let path = if source == "root" {
            root.to_path_buf()
        } else {
            let (kind, text) = source.split_once('+')?;
            if !["path", "path-bytes", "script", "script-bytes"].contains(&kind) {
                return None;
            }
            root.join(super::identity::decode_path(
                kind.ends_with("-bytes"),
                text,
            )?)
        };
        // Lexical normalization allows reading pins before a removed live path is diagnosed.
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                std::path::Component::CurDir => {}
                other => normalized.push(other.as_os_str()),
            }
        }
        Some(
            if script {
                PackageIdentity::Script(normalized)
            } else {
                PackageIdentity::Local(normalized)
            }
            .serialized(root),
        )
    }
}
pub(super) fn read_pins(path: &Path) -> Result<Pins> {
    read_pins_at(path, path.parent().unwrap_or(Path::new(".")))
}
pub(super) fn read_pins_at(path: &Path, root: &Path) -> Result<Pins> {
    let root = root.to_path_buf();
    let body = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Pins {
                root,
                ..Pins::default()
            });
        }
        Err(e) => return Err(e.into()),
    };
    let invalid = || {
        format!(
            "{} is malformed; restore a valid lockfile or delete it to resolve every dependency again",
            path.display()
        )
    };
    let document = toml::parse(&body, &path.display().to_string())?;
    let version = match document.get("lock-version").map(|e| &e.value) {
        Some(toml::Value::Integer(v @ (1 | 2))) => *v,
        _ => {
            return Err(format!(
                "{}: unsupported lock-version; this toolchain reads versions 1 and 2",
                path.display()
            )
            .into());
        }
    };
    let mut pins = Pins {
        exists: true,
        version,
        root: root.clone(),
        ..Pins::default()
    };
    let Some(toml::Value::Array(packages)) = document.get("package").map(|e| &e.value) else {
        return Err(invalid().into());
    };
    let mut old_names = BTreeMap::new();
    for item in packages {
        let toml::Value::Table(table) = item else {
            return Err(invalid().into());
        };
        let name = text(table, "name")
            .filter(|n| manifest::valid_name(n))
            .ok_or_else(invalid)?;
        let package_version = text(table, "version")
            .filter(|v| manifest::parse_version(v).is_some())
            .ok_or_else(invalid)?;
        let source = text(table, "source").ok_or_else(invalid)?;
        let subdir = match (text(table, "subdir"), text(table, "subdir-bytes")) {
            (Some(_), Some(_)) => return Err(invalid().into()),
            (Some(path), None) => PathBuf::from(path),
            (None, Some(bytes)) => {
                super::identity::decode_path(true, &bytes).ok_or_else(invalid)?
            }
            (None, None) => PathBuf::new(),
        };
        if !subdir
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        {
            return Err(invalid().into());
        }
        let commit = text(table, "commit");
        if source.starts_with("git+") && !commit.as_deref().is_some_and(full_commit) {
            return Err(invalid().into());
        }
        let id = source_id(&root, &source, &subdir, commit.as_deref()).ok_or_else(invalid)?;
        if version == 2 && text(table, "identity").as_ref() != Some(&id) {
            return Err(format!(
                "{}: malformed package identity: expected {id:?}",
                path.display()
            )
            .into());
        }
        let mut selectors = BTreeSet::new();
        if let Some(value) = table.get("selectors") {
            let toml::Value::Array(items) = &value.value else {
                return Err(invalid().into());
            };
            for item in items {
                let toml::Value::String(s) = item else {
                    return Err(invalid().into());
                };
                selectors.insert(s.clone());
            }
        }
        let Some(toml::Value::Array(items)) = table.get("dependencies").map(|e| &e.value) else {
            return Err(invalid().into());
        };
        let mut edges = BTreeMap::new();
        for item in items {
            let toml::Value::Table(edge) = item else {
                return Err(invalid().into());
            };
            let alias = text(edge, "alias")
                .filter(|a| manifest::valid_name(a))
                .ok_or_else(invalid)?;
            let target = text(edge, "package").ok_or_else(invalid)?;
            let req = text(edge, "requirement");
            if version == 2 && req.is_none() {
                return Err(invalid().into());
            }
            if edges
                .insert(
                    alias,
                    Edge {
                        target,
                        requirement: req,
                    },
                )
                .is_some()
            {
                return Err(invalid().into());
            }
        }
        let member = source == "root"
            || matches!(
                table.get("member").map(|e| &e.value),
                Some(toml::Value::Bool(true))
            );
        if version == 1 && old_names.insert(name.clone(), id.clone()).is_some() {
            return Err(invalid().into());
        }
        if pins
            .packages
            .insert(
                id,
                LockedPackage {
                    name,
                    version: package_version,
                    source,
                    subdir,
                    commit,
                    selectors,
                    edges,
                    member,
                },
            )
            .is_some()
        {
            return Err(invalid().into());
        }
    }
    let identities: BTreeSet<_> = pins.packages.keys().cloned().collect();
    for package in pins.packages.values_mut() {
        for edge in package.edges.values_mut() {
            if version == 1 {
                edge.target = old_names.get(&edge.target).ok_or_else(invalid)?.clone();
            }
            if !identities.contains(&edge.target) {
                return Err(invalid().into());
            }
        }
    }
    Ok(pins)
}
impl Pins {
    pub fn script_root(&mut self, manifest: &Manifest, directory: &Path) {
        if self.version != 1 {
            return;
        }
        let old = PackageIdentity::Local(directory.to_path_buf()).serialized(&self.root);
        let Some((_, path)) = manifest.binaries.first() else {
            return;
        };
        let new = PackageIdentity::Script(path.clone()).serialized(&self.root);
        if let Some(mut package) = self.packages.remove(&old) {
            package.source = PackageIdentity::Script(path.clone()).local_source(&self.root);
            self.packages.insert(new.clone(), package);
            for package in self.packages.values_mut() {
                for edge in package.edges.values_mut() {
                    if edge.target == old {
                        edge.target = new.clone();
                    }
                }
            }
        }
    }
    pub fn require_current_format(&self, locked: bool) -> Result<()> {
        if locked && self.exists && self.version == 1 {
            return Err("plenty.lock uses lock-version = 1; run `plenty fetch` without --locked to migrate its existing commits to version 2, then retry --locked".into());
        }
        Ok(())
    }
    pub fn prior_owner(&self, id: &str, previous: Option<&str>) -> Option<String> {
        if self.packages.contains_key(id) {
            Some(id.into())
        } else {
            previous
                .filter(|p| self.packages.contains_key(*p))
                .map(str::to_owned)
        }
    }
    pub fn edge_context(
        &self,
        owner: &str,
        alias: &str,
        dependency: &Dependency,
    ) -> Option<String> {
        let edge = self.packages.get(owner)?.edges.get(alias)?;
        let target = self.packages.get(&edge.target)?;
        let same_context = match &dependency.source {
            Source::Git { url, subdir, .. } => {
                let expected = (
                    url.clone(),
                    subdir.as_deref().map(normalize_subdir).unwrap_or_default(),
                );
                if let Some(req) = &edge.requirement {
                    git_context(req).as_ref() == Some(&expected)
                } else {
                    target.source == format!("git+{url}") && target.subdir == expected.1
                }
            }
            Source::Path(_) => edge.requirement.as_ref().is_none_or(|old| {
                source_requirement(old) == source_requirement(&requirement(dependency))
            }),
        };
        same_context.then(|| edge.target.clone())
    }
    pub fn edge_target(&self, owner: &str, alias: &str, dependency: &Dependency) -> Option<String> {
        let edge = self.packages.get(owner)?.edges.get(alias)?;
        let target = self.packages.get(&edge.target)?;
        let compatible = if let Some(req) = &edge.requirement {
            source_requirement(req) == source_requirement(&requirement(dependency))
                && self.edge_context(owner, alias, dependency).is_some()
        } else {
            match &dependency.source {
                Source::Git {
                    url,
                    subdir,
                    selector,
                } => {
                    target.source == format!("git+{url}")
                        && target.subdir
                            == subdir.as_deref().map(normalize_subdir).unwrap_or_default()
                        && target.selectors.contains(&selector.to_string())
                }
                Source::Path(_) => true,
            }
        };
        compatible.then(|| edge.target.clone())
    }
    pub fn preflight(&self, manifest: &Manifest, directory: &Path) -> Result<()> {
        if !self.exists && manifest.dependencies.is_empty() {
            return Ok(());
        }
        let fail = || format!("{LOCKFILE} is missing or stale; --locked forbids changing it");
        let id = PackageIdentity::Script(manifest.binaries.first().ok_or_else(fail)?.1.clone())
            .serialized(&self.root);
        let mut pending = vec![(manifest.clone(), directory.to_path_buf(), id)];
        let mut seen = BTreeSet::new();
        while let Some((manifest, base, owner)) = pending.pop() {
            if !seen.insert(owner.clone()) {
                continue;
            }
            let package = self.packages.get(&owner).ok_or_else(fail)?;
            if package.name != manifest.name
                || package.version != manifest.version
                || package.edges.len() != manifest.dependencies.len()
            {
                return Err(fail().into());
            }
            for (alias, dependency) in &manifest.dependencies {
                let target = self
                    .edge_target(&owner, alias, dependency)
                    .ok_or_else(fail)?;
                if let Source::Path(path) = &dependency.source {
                    let path = base.join(path).canonicalize()?;
                    let expected = PackageIdentity::Local(path.clone()).serialized(&self.root);
                    if target != expected {
                        return Err(fail().into());
                    }
                    pending.push((super::read_manifest(&path)?, path, target));
                }
            }
        }
        Ok(())
    }
    /// A bare root alias, a slash-delimited root-to-edge chain, or OWNER_ID::ALIAS.
    pub fn select_updates(
        &self,
        update: &Update,
        root: &Path,
        selected: &[PathBuf],
    ) -> Result<BTreeSet<(String, String)>> {
        let Update::Packages(selectors) = update else {
            return Ok(BTreeSet::new());
        };
        if self.version == 1 {
            return Err("plenty.lock uses lock-version = 1; run `plenty fetch` to migrate its existing commits before selecting an update edge".into());
        }
        let mut roots: Vec<_> = selected
            .iter()
            .map(|p| PackageIdentity::Local(p.clone()).serialized(root))
            .collect();
        if roots.is_empty() {
            roots.extend(
                self.packages
                    .iter()
                    .filter(|(_, p)| p.member)
                    .map(|(id, _)| id.clone()),
            );
        }
        let mut result = BTreeSet::new();
        for selector in selectors {
            let mut matches = BTreeSet::new();
            if let Some((owner, alias)) = selector.rsplit_once("::") {
                if self
                    .packages
                    .get(owner)
                    .is_some_and(|p| p.edges.contains_key(alias))
                {
                    matches.insert((owner.to_owned(), alias.to_owned()));
                }
            } else {
                for root in &roots {
                    let mut owner = root.clone();
                    let parts: Vec<_> = selector.split('/').collect();
                    for (i, alias) in parts.iter().enumerate() {
                        let Some(edge) =
                            self.packages.get(&owner).and_then(|p| p.edges.get(*alias))
                        else {
                            break;
                        };
                        if i + 1 == parts.len() {
                            matches.insert((owner.clone(), (*alias).to_owned()));
                        } else {
                            owner = edge.target.clone();
                        }
                    }
                }
                if matches.is_empty() && !selector.contains('/') {
                    let targets: Vec<_> = self
                        .packages
                        .iter()
                        .filter(|(_, p)| &p.name == selector)
                        .map(|(id, _)| id)
                        .collect();
                    if targets.len() == 1 {
                        for (owner, p) in &self.packages {
                            for (alias, edge) in &p.edges {
                                if &edge.target == targets[0] {
                                    matches.insert((owner.clone(), alias.clone()));
                                }
                            }
                        }
                    } else if targets.len() > 1 {
                        return Err(format!("ambiguous package `{selector}`; select an incoming root-to-edge path or OWNER_ID::ALIAS; candidates:\n{}", targets.iter().map(|id| format!("  {id}")).collect::<Vec<_>>().join("\n")).into());
                    }
                }
            }
            if matches.is_empty() {
                return Err(format!(
                    "no package named `{selector}` or dependency edge in the dependency graph"
                )
                .into());
            }
            for (owner, alias) in &matches {
                let package = &self.packages[owner];
                let edge = &package.edges[alias];
                if package.source.starts_with("git+")
                    && edge
                        .requirement
                        .as_deref()
                        .is_some_and(|r| r.starts_with("path:"))
                {
                    let mut boundary = owner.clone();
                    let mut visited = BTreeSet::new();
                    while visited.insert(boundary.clone()) {
                        let incoming = self.packages.iter().find_map(|(id, p)| {
                            p.edges
                                .iter()
                                .find(|(_, e)| {
                                    e.target == boundary
                                        && e.requirement
                                            .as_deref()
                                            .is_some_and(|r| r.starts_with("path:"))
                                })
                                .map(|(a, _)| (id.clone(), a.clone()))
                        });
                        if let Some((id, _)) = incoming {
                            boundary = id;
                        } else {
                            break;
                        }
                    }
                    let boundary_edges: Vec<_> = self
                        .packages
                        .iter()
                        .flat_map(|(id, p)| {
                            p.edges
                                .iter()
                                .filter(|(_, e)| e.target == boundary)
                                .map(move |(a, _)| format!("{id}::{a}"))
                        })
                        .collect();
                    return Err(format!("cannot independently update snapshot-relative path edge {owner}::{alias}; update its owning Git dependency {} (source boundary {boundary}) to move this snapshot; lockfile unchanged", boundary_edges.join(", ")).into());
                }
                let consumers: Vec<_> = self
                    .packages
                    .iter()
                    .flat_map(|(id, p)| {
                        p.edges
                            .iter()
                            .filter(|(_, e)| &e.target == owner)
                            .map(move |(a, _)| format!("{id}::{a}"))
                    })
                    .collect();
                if consumers.len() > 1 {
                    eprintln!(
                        "Updating shared outgoing edge {owner}::{alias} affects all consumers: {}",
                        consumers.join(", ")
                    );
                }
            }
            result.extend(matches);
        }
        Ok(result)
    }
}
pub(super) fn serialize(project: &Project) -> String {
    let mut order: Vec<_> = (0..project.packages.len()).collect();
    order.sort_by_key(|i| {
        (
            project.packages[*i].directory != project.root,
            project.packages[*i].identity.serialized(&project.root),
        )
    });
    let mut out = String::from(
        "# Generated by `plenty`. Commit this file; do not edit it by hand.\nlock-version = 2\n",
    );
    for index in order {
        let package = &project.packages[index];
        let id = package.identity.serialized(&project.root);
        out.push_str(&format!(
            "\n[[package]]\nidentity = {}\nname = {}\nversion = {}\nmember = {}\n",
            toml::quote(&id),
            toml::quote(&package.manifest.name),
            toml::quote(&package.manifest.version),
            project.members.contains(&index)
        ));
        match &package.source {
            PackageSource::Local => out.push_str(&format!(
                "source = {}\n",
                toml::quote(&if let PackageIdentity::Script(path) = &package.identity {
                    PackageIdentity::Script(path.clone()).local_source(&project.root)
                } else if package.directory == project.root {
                    "root".into()
                } else {
                    package.identity.local_source(&project.root)
                })
            )),
            PackageSource::Git {
                url,
                subdir,
                commit,
                selectors,
                ..
            } => {
                out.push_str(&format!(
                    "source = {}\ncommit = {}\n",
                    toml::quote(&format!("git+{url}")),
                    toml::quote(commit)
                ));
                if let Some(s) = subdir {
                    let (bytes, text) = super::identity::path_fields(Path::new(""), s);
                    out.push_str(&format!(
                        "{} = {}\n",
                        if bytes { "subdir-bytes" } else { "subdir" },
                        toml::quote(&text)
                    ));
                }
                if !selectors.is_empty() {
                    out.push_str(&format!(
                        "selectors = [{}]\n",
                        selectors
                            .iter()
                            .map(|s| toml::quote(&s.to_string()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        }
        let dependencies: Vec<_> = package
            .dependencies
            .iter()
            .map(|(alias, target)| {
                format!(
                    "{{ alias = {}, package = {}, requirement = {} }}",
                    toml::quote(alias),
                    toml::quote(&project.packages[*target].identity.serialized(&project.root)),
                    toml::quote(&requirement(&package.manifest.dependencies[alias]))
                )
            })
            .collect();
        out.push_str(&format!("dependencies = [{}]\n", dependencies.join(", ")));
    }
    out
}
