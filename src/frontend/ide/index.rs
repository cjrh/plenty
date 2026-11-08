//! The editor analyses of a package and of the workspace members that depend
//! on it (issues #821 and #822). Each library, executable and test file is
//! checked as an entry, and then each `src/` file no entry reached, so every
//! written reference to a declaration of the package is found once. Find
//! references and moving a declaration read the same index.
use super::{Analysis, File, HashMap, Location, SymbolKind, analyze};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A reference from any analysis, keyed by where it is written.
#[derive(Clone, Debug)]
pub(crate) struct Use {
    pub(crate) path: PathBuf,
    pub(crate) location: Location,
    /// The declaration's position: `(file, line, column)`; None for `std`.
    pub(crate) target: Option<(PathBuf, u32, u32)>,
    /// The declaration's position in any file, `std` included.
    pub(crate) identity: Option<(File, u32, u32)>,
    pub(crate) kind: SymbolKind,
    /// See [`Reference::inferred`](super::Reference::inferred).
    pub(crate) inferred: bool,
}

/// The files each analysis read: each entry's, then each unreached file's.
#[derive(Default)]
pub(crate) struct Reach {
    pub(crate) entries: Vec<BTreeSet<PathBuf>>,
    pub(crate) files: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    /// The first error of each analysis that did not check, with its place.
    pub(crate) errors: Vec<String>,
    /// Each analysis's [`Analysis::unresolved`] places, in package files.
    pub(crate) unresolved: Vec<(PathBuf, Location)>,
}

impl Reach {
    /// The files the targets and tests reach.
    pub(crate) fn reached(&self) -> BTreeSet<PathBuf> {
        self.entries.iter().flatten().cloned().collect()
    }
}

/// Whether an analysis that does not check ends the index ([`Errors::Stop`])
/// or is kept with the references it recorded before the error.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Errors {
    Stop,
    Keep,
}

/// A workspace member and the entries that can refer to its declarations.
pub(crate) struct Package {
    pub(crate) project: crate::project::Project,
    pub(crate) member: usize,
    /// The targets and tests of the member and of the workspace members that
    /// depend on it, with their graphs.
    pub(crate) entries: Vec<(PathBuf, crate::project::SourceGraph)>,
    /// Every source file of those members, checked as its own entry when no
    /// target reaches it, so code no target imports is still read.
    pub(crate) files: Vec<(PathBuf, crate::project::SourceGraph)>,
    /// How many analyses have run.
    pub(crate) analyses: std::cell::Cell<usize>,
}

pub(crate) fn source_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            source_files(&path, files);
        } else if crate::source_path::is_source(&path)
            && let Ok(path) = path.canonicalize()
        {
            files.push(path);
        }
    }
}

impl Package {
    /// The workspace member whose directory holds `path`, a canonical path,
    /// loaded without fetching or changing `plenty.lock`.
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let directory = path
            .parent()
            .and_then(|p| crate::project::discover(p).ok())
            .ok_or("this file has no plenty.toml, so it is not in a package")?;
        let options = crate::project::ResolveOptions {
            locked: true,
            offline: true,
            ..Default::default()
        };
        let project = crate::project::Project::load(&directory, &options)
            .map_err(|e| format!("cannot load the package: {e}"))?;
        let member = member_of(&project, path).ok_or("this file is not in a workspace member")?;
        Ok(Self::for_member(project, member))
    }

    /// The workspace member whose directory holds `path`, the innermost
    /// when members are nested.
    pub(crate) fn member_of(&self, path: &Path) -> Option<usize> {
        member_of(&self.project, path)
    }

    /// The index of `member` of the same workspace.
    pub(crate) fn for_member(project: crate::project::Project, member: usize) -> Self {
        let mut entries = vec![];
        let mut all_files = vec![];
        for &m in &project.members {
            let package = &project.packages[m];
            if m != member && !package.dependencies.iter().any(|(_, d)| *d == member) {
                continue;
            }
            let graph = project.source_graph(m);
            let mut files = vec![];
            source_files(&package.directory.join("src"), &mut files);
            files.sort();
            for target in package
                .manifest
                .library
                .iter()
                .chain(package.manifest.binaries.iter().map(|(_, e)| e))
            {
                if let Ok(path) = package.directory.join(target).canonicalize() {
                    entries.push((path, graph.clone()));
                }
            }
            all_files.extend(files.into_iter().map(|f| (f, graph.clone())));
            let mut tests = vec![];
            source_files(&package.directory.join("tests"), &mut tests);
            tests.sort();
            if !tests.is_empty() {
                let graph = project.test_graph(m);
                entries.extend(tests.into_iter().map(|f| (f, graph.clone())));
            }
        }
        Package {
            project,
            member,
            entries,
            files: all_files,
            analyses: std::cell::Cell::new(0),
        }
    }

    /// `path` relative to the workspace root, for messages.
    pub(crate) fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.project.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    /// Analyze the targets and then each file they do not reach, with
    /// `overlay`; the references found and what each analysis read. Each
    /// file is read by as few analyses as its imports allow. With
    /// `previous`, an analysis that read none of the `edited` files is not
    /// repeated: its files, and so its result, are unchanged.
    pub(crate) fn analyze(
        &self,
        overlay: &HashMap<PathBuf, String>,
        previous: Option<(&Reach, &BTreeSet<PathBuf>)>,
        errors: Errors,
    ) -> Result<(Vec<Use>, Reach), String> {
        // A generic body can resolve one written name to a different
        // declaration in each instantiation, so a place keeps each.
        let mut uses: BTreeMap<(PathBuf, u32, u32, u32), Vec<Use>> = BTreeMap::new();
        let mut reach = Reach::default();
        let unchanged = |read: Option<&BTreeSet<PathBuf>>| {
            let (_, edited) = previous?;
            read.filter(|read| read.is_disjoint(edited)).cloned()
        };
        let one = |root: &Path,
                   graph: &crate::project::SourceGraph,
                   reach: &mut Reach,
                   uses: &mut BTreeMap<_, _>|
         -> Result<BTreeSet<PathBuf>, String> {
            let (read, error) = self.analyze_one(root, graph, overlay, uses, &mut reach.unresolved);
            if let Some(error) = error {
                if errors == Errors::Stop {
                    return Err(error);
                }
                reach.errors.push(error);
            }
            Ok(read)
        };
        for (i, (root, graph)) in self.entries.iter().enumerate() {
            let read = match unchanged(previous.and_then(|(p, _)| p.entries.get(i))) {
                Some(read) => read,
                None => one(root, graph, &mut reach, &mut uses)?,
            };
            reach.entries.push(read);
        }
        let mut seen = reach.reached();
        for (root, graph) in &self.files {
            if seen.contains(root) {
                continue;
            }
            let read = match unchanged(previous.and_then(|(p, _)| p.files.get(root))) {
                Some(read) => read,
                None => one(root, graph, &mut reach, &mut uses)?,
            };
            seen.extend(read.iter().cloned());
            reach.files.insert(root.clone(), read);
        }
        Ok((uses.into_values().flatten().collect(), reach))
    }

    /// Analyze one entry, adding its references to `uses`; the files it read
    /// and its first error, with its place.
    fn analyze_one(
        &self,
        root: &Path,
        graph: &crate::project::SourceGraph,
        overlay: &HashMap<PathBuf, String>,
        uses: &mut BTreeMap<(PathBuf, u32, u32, u32), Vec<Use>>,
        unresolved: &mut Vec<(PathBuf, Location)>,
    ) -> (BTreeSet<PathBuf>, Option<String>) {
        self.analyses.set(self.analyses.get() + 1);
        let analysis: Analysis = analyze(root, Some(graph), overlay.clone());
        for location in &analysis.unresolved {
            if let File::Path(path) = &location.file {
                let place = (path.clone(), location.clone());
                if !unresolved.contains(&place) {
                    unresolved.push(place);
                }
            }
        }
        let error = analysis.diagnostics.first().map(|error| {
            let at = match &error.file {
                Some(File::Path(path)) => {
                    format!("{}:{}:{}: ", self.relative(path), error.line, error.column)
                }
                _ => String::new(),
            };
            format!("{at}{}", error.message)
        });
        for reference in &analysis.references {
            let File::Path(path) = &reference.location.file else {
                continue;
            };
            let symbol = &analysis.symbols[reference.symbol];
            // Format strings record calls to `std.fmt` methods at their
            // fields, such as `p.x}` or `gree` for the methods that write
            // `p.x` and `greet()`. A name is written as a name or a dotted
            // name, and a member, which no import renames, as its own name.
            let l = &reference.location;
            let written: String = analysis
                .sources
                .get(&l.file)
                .and_then(|text| text.lines().nth(l.line as usize - 1))
                .map(|line| {
                    line.chars()
                        .skip(l.column as usize - 1)
                        .take(l.length as usize)
                        .collect()
                })
                .unwrap_or_default();
            let dotted = written.split('.').all(|part| {
                !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_')
            });
            let member = symbol.name.rsplit('.').next().unwrap_or_default();
            let named = matches!(
                symbol.kind,
                SymbolKind::Method | SymbolKind::Field | SymbolKind::Variant
            )
            .then(|| written.rsplit('.').next() == Some(member));
            if !dotted || named == Some(false) {
                continue;
            }
            let target = match &symbol.location {
                Some(Location {
                    file: File::Path(file),
                    line,
                    column,
                    ..
                }) => Some((file.clone(), *line, *column)),
                _ => None,
            };
            let identity = symbol
                .location
                .as_ref()
                .map(|l| (l.file.clone(), l.line, l.column));
            let place = uses
                .entry((path.clone(), l.line, l.column, l.length))
                .or_default();
            if !place.iter().any(|u| u.identity == identity) {
                // A checked use comes before an inferred one, so the first
                // use at a place does not depend on which analysis read
                // the file first.
                let at = match reference.inferred {
                    true => place.len(),
                    false => place.partition_point(|u| !u.inferred),
                };
                place.insert(
                    at,
                    Use {
                        path: path.clone(),
                        location: l.clone(),
                        target,
                        identity,
                        kind: symbol.kind,
                        inferred: reference.inferred,
                    },
                );
            }
        }
        let read = analysis
            .sources
            .keys()
            .filter_map(|f| match f {
                File::Path(path) => Some(path.clone()),
                File::Standard(_) => None,
            })
            .collect();
        (read, error)
    }
}

/// The first use found at each place, for a caller that rewrites a place
/// once. `uses` is in [`Package::analyze`]'s order, grouped by place.
pub(crate) fn one_per_place(mut uses: Vec<Use>) -> Vec<Use> {
    uses.dedup_by(|b, a| a.path == b.path && a.location == b.location);
    uses
}

fn member_of(project: &crate::project::Project, path: &Path) -> Option<usize> {
    project
        .members
        .iter()
        .copied()
        .filter_map(|m| {
            let directory = project.packages[m].directory.canonicalize().ok()?;
            path.starts_with(&directory)
                .then(|| (directory.components().count(), m))
        })
        .max()
        .map(|(_, m)| m)
}

/// Every written reference to the declaration at `identity` (its name's
/// file, line and column) that the package of `path` and the workspace
/// members depending on it can contain, the declaration's own name
/// included. A declaration in another workspace member is searched from
/// that member. `overlay` holds unsaved text by canonical path.
pub fn references(
    path: &Path,
    identity: &(File, u32, u32),
    overlay: &std::collections::HashMap<PathBuf, String, impl std::hash::BuildHasher>,
) -> Result<Found, String> {
    let overlay: HashMap<PathBuf, String> = overlay
        .iter()
        .map(|(p, t)| (p.clone(), t.clone()))
        .collect();
    let path = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut package = Package::open(&path)?;
    if let File::Path(declared) = &identity.0
        && let Some(member) = package.member_of(declared)
        && member != package.member
    {
        package = Package::for_member(package.project, member);
    }
    let (uses, reach) = package.analyze(&overlay, None, Errors::Keep)?;
    let mut found: Vec<Location> = uses
        .into_iter()
        .filter(|u| u.identity.as_ref() == Some(identity))
        .map(|u| u.location)
        .collect();
    found.sort_by(|a, b| {
        let key = |l: &Location| match &l.file {
            File::Path(p) => (p.clone(), l.line, l.column),
            File::Standard(s) => (PathBuf::from(s), l.line, l.column),
        };
        key(a).cmp(&key(b))
    });
    found.dedup();
    // A file outside `src/`, `tests/` and the manifest's entries, such as an
    // example, is read by no analysis.
    let read = reach.reached().contains(&path) || reach.files.values().any(|f| f.contains(&path));
    Ok(Found {
        locations: found,
        errors: reach.errors,
        read,
    })
}

/// The result of [`references`].
pub struct Found {
    pub locations: Vec<Location>,
    /// The first error of each entry that did not check, whose references
    /// may be incomplete.
    pub errors: Vec<String>,
    /// Whether any analysis read the file the search started from.
    pub read: bool,
}
