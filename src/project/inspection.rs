//! Human-readable resolution provenance and graph-local resource contracts.
use super::{BuildOptions, Package, PackageSource, Project, Result, open};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn source(package: &Package) -> String {
    match &package.source {
        PackageSource::Local => format!("path {}", package.directory.display()),
        PackageSource::Git {
            url,
            subdir,
            commit,
            ..
        } => format!(
            "git {url} subdir {} commit {commit}",
            subdir.as_deref().unwrap_or(Path::new(".")).display()
        ),
    }
}

impl Project {
    /// Every root-to-instance alias path. The resolver has already rejected
    /// cycles, but keep a visiting set so this diagnostic helper stays bounded
    /// when used with an incomplete graph in a caller's own tests.
    fn dependency_paths(&self, roots: &[usize]) -> BTreeMap<usize, BTreeSet<String>> {
        fn visit(
            project: &Project,
            index: usize,
            path: String,
            visiting: &mut BTreeSet<usize>,
            paths: &mut BTreeMap<usize, BTreeSet<String>>,
        ) {
            paths.entry(index).or_default().insert(path.clone());
            if !visiting.insert(index) {
                return;
            }
            for (alias, target) in &project.packages[index].dependencies {
                visit(project, *target, format!("{path}/{alias}"), visiting, paths);
            }
            visiting.remove(&index);
        }
        let mut paths = BTreeMap::new();
        for root in roots {
            visit(
                self,
                *root,
                self.packages[*root].manifest.name.clone(),
                &mut BTreeSet::new(),
                &mut paths,
            );
        }
        paths
    }

    /// Check one executable or library's linked dependency closure. A shared
    /// workspace lock is intentionally not the unit of resource exclusivity.
    pub(super) fn validate_resources(&self, entry: usize) -> Result<()> {
        let mut resources: BTreeMap<&str, usize> = BTreeMap::new();
        let mut pending = vec![entry];
        let mut seen = BTreeSet::new();
        while let Some(index) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            let package = &self.packages[index];
            pending.extend(package.dependencies.iter().map(|(_, target)| *target));
            for resource in &package.manifest.shared_resources {
                if let Some(other) = resources.insert(resource, index) {
                    let previous = &self.packages[other];
                    let paths = self.dependency_paths(&[entry]);
                    let describe = |index: usize, package: &Package| {
                        format!(
                            "{} {}\n    identity: {}\n    source: {}\n    reached through: {}",
                            package.manifest.name,
                            package.manifest.version,
                            package.identity.serialized(&self.root),
                            source(package),
                            paths[&index].iter().cloned().collect::<Vec<_>>().join(", ")
                        )
                    };
                    return Err(format!(
                        "shared-resource conflict for `{resource}` in linked package `{}`: two resolved instances claim the same resource\n  {}\n  {}\ncoordinate the declared dependencies; resolution does not choose a winner",
                        self.packages[entry].manifest.name,
                        describe(other, previous),
                        describe(index, package),
                    ).into());
                }
            }
        }
        Ok(())
    }
}

/// Inspect reachable duplicate package names, or explain matching resolved
/// instances. The default `graph` command remains the machine-readable graph.
pub fn inspect(
    directory: &Path,
    options: &BuildOptions,
    duplicates: bool,
    why: Option<&str>,
) -> Result<String> {
    let project = open(directory, options)?;
    let paths = project.dependency_paths(&project.selected);
    let mut names: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for index in paths.keys() {
        names
            .entry(&project.packages[*index].manifest.name)
            .or_default()
            .push(*index);
    }
    let mut selected = Vec::new();
    for (name, indices) in names {
        if duplicates && indices.len() < 2 {
            continue;
        }
        for index in indices {
            let package = &project.packages[index];
            if why.is_some_and(|selector| {
                selector != name
                    && selector != package.identity.serialized(&project.root)
                    && !paths[&index].contains(selector)
                    && !paths[&index].iter().any(|path| {
                        path.split_once('/')
                            .is_some_and(|(_, tail)| tail == selector)
                    })
            }) {
                continue;
            }
            selected.push(index);
        }
    }
    selected.sort_by_key(|index| {
        let package = &project.packages[*index];
        (
            package.manifest.name.clone(),
            package.identity.serialized(&project.root),
        )
    });
    if selected.is_empty() {
        return if let Some(selector) = why {
            Err(format!("no package matches `{selector}` in the selected dependency graph; use a declared package name, full identity or root-to-package alias path").into())
        } else {
            Ok("No duplicate package names in the selected dependency graph.\n".into())
        };
    }
    let mut output = String::new();
    for index in selected {
        let package = &project.packages[index];
        output.push_str(&format!(
            "{} {}\n  identity: {}\n  source: {}\n",
            package.manifest.name,
            package.manifest.version,
            package.identity.serialized(&project.root),
            source(package)
        ));
        for path in &paths[&index] {
            output.push_str(&format!("  via: {path}\n"));
        }
    }
    Ok(output)
}
