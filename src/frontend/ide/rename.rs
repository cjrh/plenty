//! Rename a module-level declaration, a field, method or enum variant, an
//! import alias or a local (issues #821 and #876). A plan is computed from
//! editor analyses and returned as text edits; nothing is written. A
//! declaration or member is renamed across its package and the workspace
//! members that depend on it; an alias or a local only in its file. Every unsupported case is refused before any
//! edit is returned, and the edited files are checked again before the
//! plan is accepted. Strings, comments and docstrings are not edited.
use super::index::{Errors, Package, Use};
use super::relocate::{
    FileEdits, Form, TextEdit, apply, conditionals, form, is_import, map_point, mentions, replace,
    top_level, word,
};
use super::{Analysis, File, HashMap, HashSet, Location, SymbolKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A checked rename, ready to preview and apply.
#[derive(Clone, Debug)]
pub struct Rename {
    pub name: String,
    pub new_name: String,
    pub files: Vec<FileEdits>,
    /// Public API changes the rename makes, one sentence each.
    pub api_changes: Vec<String>,
}

/// The name a rename at a position would change: where it is written there
/// and its spelling.
#[derive(Clone, Debug)]
pub struct Target {
    pub location: Location,
    pub name: String,
}

type Identity = Option<(File, u32, u32)>;

const NO_PACKAGE: &str =
    "renaming a module-level declaration needs a package: this file has no plenty.toml";

fn in_dependency(name: &str) -> String {
    format!("`{name}` is declared in a dependency, which a rename does not edit")
}

/// What the cursor names.
enum Subject {
    /// A module-level declaration, or a member of a class, enum or
    /// protocol, from any of its references.
    Declaration {
        identity: (File, u32, u32),
        name: String,
        qualified: String,
        /// The kind of a member; None for a module-level declaration.
        member: Option<SymbolKind>,
    },
    /// The alias of a `from` import, `h` in `from m import f as h`.
    Alias {
        identity: (File, u32, u32),
        alias: String,
        /// The alias in the import, from 1.
        line: u32,
        column: u32,
    },
    /// A local variable of the file's analysis.
    Local { symbol: usize, name: String },
}

/// What a rename at (`line`, `column`), from 1 and in characters, of
/// `path` would change, or why it is refused. `analysis` is a current check
/// of `text`, the file's text; this reads no other file.
pub fn prepare(
    analysis: &Analysis,
    path: &Path,
    text: &str,
    line: u32,
    column: u32,
) -> Result<Target, String> {
    let (subject, location) = subject(analysis, path, text, line, column)?;
    if let Subject::Declaration {
        identity: (File::Path(declared), ..),
        name,
        ..
    } = &subject
    {
        path.parent()
            .and_then(|p| crate::project::discover(p).ok())
            .ok_or(NO_PACKAGE)?;
        if Package::open(path)?.member_of(declared).is_none() {
            return Err(in_dependency(name));
        }
    }
    let name = written(text, &location);
    Ok(Target { location, name })
}

/// Plan renaming what (`line`, `column`) of `path` names to `new_name`.
/// `analysis` is a current check of the file with `overlay`, which holds
/// unsaved text by canonical path; `recheck` checks the file again with
/// another overlay. An `Err` explains why the rename is refused.
pub fn plan(
    analysis: &Analysis,
    path: &Path,
    line: u32,
    column: u32,
    new_name: &str,
    overlay: &std::collections::HashMap<PathBuf, String, impl std::hash::BuildHasher>,
    recheck: &dyn Fn(&std::collections::HashMap<PathBuf, String>) -> Analysis,
) -> Result<Rename, String> {
    let overlay: HashMap<PathBuf, String> = overlay
        .iter()
        .map(|(p, t)| (p.clone(), t.clone()))
        .collect();
    let path = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let text = read(&overlay, &path)?;
    if analysis
        .sources
        .get(&File::Path(path.clone()))
        .is_none_or(|checked| **checked != *text)
    {
        return Err("the file has changed since its last check; try the rename again".into());
    }
    let (subject, location) = subject(analysis, &path, &text, line, column)?;
    let old = written(&text, &location);
    valid_name(new_name, &old)?;
    match subject {
        Subject::Declaration {
            identity,
            name,
            qualified,
            member,
        } => {
            if member == Some(SymbolKind::Method) && implicit(new_name) {
                return Err(format!(
                    "`{new_name}` is a method name the language calls implicitly, so a method cannot be renamed to it"
                ));
            }
            declaration(
                analysis,
                (&path, &location),
                &identity,
                (&name, &qualified, member),
                new_name,
                &overlay,
            )
        }
        Subject::Alias {
            identity,
            alias,
            line,
            column,
        } => {
            let refs = places(analysis)
                .into_iter()
                .filter(|p| p.path == path && p.identity.as_ref() == Some(&identity))
                .filter(|p| written(&text, &p.location) == alias)
                .map(|p| p.location)
                .collect();
            let alias_at = Location {
                file: File::Path(path.clone()),
                line,
                column,
                length: alias.chars().count() as u32,
            };
            in_file(
                analysis, &path, &text, &alias, new_name, refs, alias_at, &overlay, recheck,
            )
        }
        Subject::Local { symbol, name } => {
            let mut refs: Vec<Location> = analysis
                .references
                .iter()
                .filter(|r| r.symbol == symbol)
                .map(|r| r.location.clone())
                .collect();
            if refs.iter().any(|l| l.file != File::Path(path.clone())) {
                return Err(format!("`{name}` is written in another file"));
            }
            let first = refs.remove(0);
            in_file(
                analysis, &path, &text, &name, new_name, refs, first, &overlay, recheck,
            )
        }
    }
}

/// What the cursor names and where that name is written, or why it cannot
/// be renamed.
fn subject(
    analysis: &Analysis,
    path: &Path,
    text: &str,
    line: u32,
    column: u32,
) -> Result<(Subject, Location), String> {
    let file = File::Path(path.to_owned());
    let Some(reference) = analysis.reference_at(&file, line, column) else {
        if let Some(found) = alias_at(analysis, path, text, line, column)? {
            return Ok(found);
        }
        // The analysis records no reference for a module alias's uses
        // (`u` in `u.f()`).
        let spelled = word_at(text, line, column);
        if !spelled.is_empty()
            && aliases(text).is_ok_and(|all| all.iter().any(|a| a.module && a.alias == spelled))
        {
            return Err(MODULE_ALIAS.into());
        }
        return Err("there is no name to rename here".into());
    };
    let symbol = &analysis.symbols[reference.symbol];
    let short = symbol.name.rsplit('.').next().unwrap_or(&symbol.name);
    let not_yet = |what: &str, reason: &str| {
        Err(format!(
            "renaming {what} is not supported yet: {reason} (issue #876)"
        ))
    };
    match symbol.kind {
        SymbolKind::Parameter => {
            return not_yet(
                "a parameter",
                "rename does not yet edit named arguments, though find references lists them",
            );
        }
        SymbolKind::Module => {
            let spelled = written(text, &name_at(text, &reference.location));
            if spelled != short {
                return Err(MODULE_ALIAS.into());
            }
            return Err("renaming a module is not supported: it means renaming its file".into());
        }
        _ => {}
    }
    let Some(declared) = &symbol.location else {
        return Err(format!("`{short}` is built in, so it cannot be renamed"));
    };
    if let File::Standard(module) = &declared.file {
        return Err(format!(
            "`{short}` is declared in the standard library ({module}), which a rename does not edit"
        ));
    }
    let location = name_at(text, &reference.location);
    if symbol.kind == SymbolKind::Local {
        let subject = Subject::Local {
            symbol: reference.symbol,
            name: short.to_owned(),
        };
        return Ok((subject, location));
    }
    let identity = (declared.file.clone(), declared.line, declared.column);
    let spelled = written(text, &location);
    let member = matches!(
        symbol.kind,
        SymbolKind::Field | SymbolKind::Method | SymbolKind::Variant
    )
    .then_some(symbol.kind);
    if let Some(kind) = member {
        if spelled != short {
            return Err(format!("cannot rename `{spelled}`, written for `{short}`"));
        }
        if kind == SymbolKind::Method {
            if implicit(short) {
                return Err(format!(
                    "`{short}` is a method name the language calls implicitly, so it cannot be renamed"
                ));
            }
            let owner = symbol.name.rsplit_once('.').map_or("", |(o, _)| o);
            if analysis
                .symbols
                .iter()
                .any(|s| s.kind == SymbolKind::Protocol && s.name == owner)
            {
                return Err("renaming a protocol requirement is not supported: a class or enum satisfies it by declaring a method of that name, which a rename does not edit".into());
            }
        }
        let subject = Subject::Declaration {
            identity,
            name: short.to_owned(),
            qualified: symbol.name.clone(),
            member,
        };
        return Ok((subject, location));
    }
    if spelled != short {
        // A use of an alias: the alias binding is renamed, not the
        // declaration.
        let binding = aliases(text)?
            .into_iter()
            .find(|a| !a.module && a.name == short && a.alias == spelled)
            .ok_or_else(|| format!("cannot rename `{spelled}`, written for `{short}`"))?;
        let subject = Subject::Alias {
            identity,
            alias: spelled,
            line: binding.line,
            column: binding.column,
        };
        return Ok((subject, location));
    }
    let subject = Subject::Declaration {
        identity,
        name: short.to_owned(),
        qualified: symbol.name.clone(),
        member: None,
    };
    Ok((subject, location))
}

/// A method name with a meaning to the language: a `__name__` hook, or a
/// formatting method that `print` and f-strings call.
fn implicit(name: &str) -> bool {
    (name.len() > 4 && name.starts_with("__") && name.ends_with("__"))
        || matches!(name, "display" | "debug")
}

/// An alias of a `from` import at the left margin.
struct Alias {
    name: String,
    /// The item's name, from 1.
    name_line: u32,
    name_column: u32,
    alias: String,
    /// The alias, from 1.
    line: u32,
    column: u32,
    /// `import a.b as c`, which names a module.
    module: bool,
}

fn aliases(text: &str) -> Result<Vec<Alias>, String> {
    let mut out = vec![];
    for tokens in top_level(text)? {
        if !is_import(&tokens) {
            continue;
        }
        let from = tokens[0].is("from");
        let Some(import) = tokens.iter().position(|t| t.is("import")) else {
            continue;
        };
        for (i, t) in tokens.iter().enumerate().skip(import + 1) {
            if !t.is("as") {
                continue;
            }
            let (Some(name), Some(alias)) = (tokens.get(i.wrapping_sub(1)), tokens.get(i + 1))
            else {
                continue;
            };
            let (Some(name_text), Some(alias_text)) = (word(name), word(alias)) else {
                continue;
            };
            out.push(Alias {
                name: name_text.to_owned(),
                name_line: name.line,
                name_column: name.column,
                alias: alias_text.to_owned(),
                line: alias.line,
                column: alias.column,
                module: !from,
            });
        }
    }
    Ok(out)
}

/// The import alias at the cursor, with the declaration it names.
fn alias_at(
    analysis: &Analysis,
    path: &Path,
    text: &str,
    line: u32,
    column: u32,
) -> Result<Option<(Subject, Location)>, String> {
    let Ok(all) = aliases(text) else {
        return Ok(None);
    };
    let Some(alias) = all.into_iter().find(|a| {
        a.line == line && a.column <= column && column < a.column + a.alias.chars().count() as u32
    }) else {
        return Ok(None);
    };
    if alias.module {
        return Err(MODULE_ALIAS.into());
    }
    let file = File::Path(path.to_owned());
    let item = analysis
        .reference_at(&file, alias.name_line, alias.name_column)
        .ok_or_else(|| format!("cannot tell what `{}` names", alias.name))?;
    let symbol = &analysis.symbols[item.symbol];
    if symbol.kind == SymbolKind::Module {
        return Err(MODULE_ALIAS.into());
    }
    let declared = symbol
        .location
        .as_ref()
        .ok_or_else(|| format!("cannot tell what `{}` names", alias.name))?;
    let location = Location {
        file,
        line: alias.line,
        column: alias.column,
        length: alias.alias.chars().count() as u32,
    };
    let subject = Subject::Alias {
        identity: (declared.file.clone(), declared.line, declared.column),
        alias: alias.alias,
        line: alias.line,
        column: alias.column,
    };
    Ok(Some((subject, location)))
}

const MODULE_ALIAS: &str = "renaming a module alias (`import m as a`) is not supported";

/// The identifier containing (`line`, `column`), from 1, or "".
fn word_at(text: &str, line: u32, column: u32) -> String {
    let Some(chars) = text
        .lines()
        .nth(line as usize - 1)
        .map(|l| l.chars().collect::<Vec<_>>())
    else {
        return String::new();
    };
    let part = |c: &char| c.is_ascii_alphanumeric() || *c == '_';
    let at = column as usize - 1;
    if !chars.get(at).is_some_and(part) {
        return String::new();
    }
    let start = chars[..at]
        .iter()
        .rposition(|c| !part(c))
        .map_or(0, |i| i + 1);
    let end = chars[at..]
        .iter()
        .position(|c| !part(c))
        .map_or(chars.len(), |i| at + i);
    chars[start..end].iter().collect()
}

/// The last name of a dotted reference, which is the part a rename edits.
fn name_at(text: &str, location: &Location) -> Location {
    let spelled = written(text, location);
    let last = spelled.rsplit('.').next().unwrap_or(&spelled);
    let skipped = spelled.chars().count() - last.chars().count();
    Location {
        column: location.column + skipped as u32,
        length: last.chars().count() as u32,
        ..location.clone()
    }
}

fn written(text: &str, location: &Location) -> String {
    text.lines()
        .nth(location.line as usize - 1)
        .map(|line| {
            line.chars()
                .skip(location.column as usize - 1)
                .take(location.length as usize)
                .collect()
        })
        .unwrap_or_default()
}

fn valid_name(new_name: &str, old: &str) -> Result<(), String> {
    let mut chars = new_name.chars();
    let identifier = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !identifier {
        return Err(format!(
            "`{new_name}` is not a name: a name is ASCII letters, digits and `_`, and does not start with a digit"
        ));
    }
    if crate::frontend::reserved(new_name) {
        return Err(format!("`{new_name}` is a reserved word"));
    }
    if new_name == old {
        return Err(format!("the name is already `{old}`"));
    }
    Ok(())
}

fn read(overlay: &HashMap<PathBuf, String>, path: &Path) -> Result<String, String> {
    match overlay.get(path) {
        Some(text) => Ok(text.clone()),
        None => std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())),
    }
}

/// A written reference and the declaration it names.
struct Place {
    path: PathBuf,
    location: Location,
    identity: Identity,
}

fn places(analysis: &Analysis) -> Vec<Place> {
    analysis
        .references
        .iter()
        .filter_map(|r| {
            let File::Path(path) = &r.location.file else {
                return None;
            };
            let symbol = &analysis.symbols[r.symbol];
            Some(Place {
                path: path.clone(),
                location: r.location.clone(),
                identity: symbol
                    .location
                    .as_ref()
                    .map(|l| (l.file.clone(), l.line, l.column)),
            })
        })
        .collect()
}

fn from_use(u: &Use) -> Place {
    Place {
        path: u.path.clone(),
        location: u.location.clone(),
        identity: u.identity.clone(),
    }
}

/// Where each reference of `paths` is written, from 1, and the
/// declarations it names, with `edits` applied to the text the positions
/// were read from.
fn resolved(
    places: &[Place],
    paths: &HashSet<PathBuf>,
    edits: &HashMap<PathBuf, Vec<TextEdit>>,
) -> HashMap<(PathBuf, u32, u32, u32), HashSet<Identity>> {
    let none: Vec<TextEdit> = vec![];
    let map = |path: &Path, line: u32, column: u32| -> Option<(u32, u32)> {
        let edits = edits.get(path).unwrap_or(&none);
        map_point(edits, line - 1, column - 1).map(|(l, c)| (l + 1, c + 1))
    };
    let mut out: HashMap<_, HashSet<Identity>> = HashMap::default();
    for p in places.iter().filter(|p| paths.contains(&p.path)) {
        let l = &p.location;
        let (Some(start), Some(end)) = (
            map(&p.path, l.line, l.column),
            map(&p.path, l.line, l.column + l.length),
        ) else {
            continue;
        };
        let identity = match &p.identity {
            Some((File::Path(file), line, column)) => map(file, *line, *column)
                .map(|(line, column)| (File::Path(file.clone()), line, column)),
            other => other.clone(),
        };
        // A reference is on one line, and so is a rename's edit.
        out.entry((p.path.clone(), start.0, start.1, end.1 - start.1))
            .or_default()
            .insert(identity);
    }
    out
}

/// The first reference of `after` that names something else than it did
/// in `before`, or is new or gone.
fn changed(
    before: &HashMap<(PathBuf, u32, u32, u32), HashSet<Identity>>,
    after: &HashMap<(PathBuf, u32, u32, u32), HashSet<Identity>>,
) -> Option<(PathBuf, u32, u32)> {
    let mut differ: Vec<(PathBuf, u32, u32)> = before
        .iter()
        .filter(|(key, identities)| after.get(*key) != Some(identities))
        .chain(after.iter().filter(|(key, _)| !before.contains_key(*key)))
        .map(|((path, line, column, _), _)| (path.clone(), *line, *column))
        .collect();
    differ.sort();
    differ.into_iter().next()
}

/// Refuse when an `if const` arm of `text` mentions one of `names`: the
/// analysis does not see unselected arms, so a rename could not edit or
/// check them.
fn arms(text: &str, names: &[&str], shown: &str) -> Result<(), String> {
    if !names.iter().any(|n| mentions(text, n)) {
        return Ok(());
    }
    let lines: Vec<&str> = text.lines().collect();
    for (first, last) in conditionals(text)? {
        let block = lines[first as usize..=(last as usize).min(lines.len() - 1)].join("\n");
        if let Some(name) = names.iter().find(|n| mentions(&block, n)) {
            return Err(format!(
                "the `if const` block at {shown}:{} mentions `{name}`; a rename cannot check the arms a build does not select",
                first + 1
            ));
        }
    }
    Ok(())
}

/// Rename a local or an import alias: `first` and `rest` are where its name
/// is written in `path`, all in `text`.
#[allow(clippy::too_many_arguments)]
fn in_file(
    analysis: &Analysis,
    path: &Path,
    text: &str,
    old: &str,
    new_name: &str,
    rest: Vec<Location>,
    first: Location,
    overlay: &HashMap<PathBuf, String>,
    recheck: &dyn Fn(&std::collections::HashMap<PathBuf, String>) -> Analysis,
) -> Result<Rename, String> {
    let shown = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    if let Some(error) = analysis.diagnostics.first() {
        return Err(format!(
            "the file does not check, so the rename cannot be verified: {}:{}: {}",
            error.line, error.column, error.message
        ));
    }
    arms(text, &[old, new_name], &shown)?;
    let mut edits = vec![];
    for location in std::iter::once(&first).chain(&rest) {
        let location = name_at(text, location);
        if written(text, &location) != old {
            return Err(format!(
                "cannot rewrite the name at {shown}:{}:{}",
                location.line, location.column
            ));
        }
        edits.push(replace(
            location.line - 1,
            location.column - 1,
            location.length,
            new_name,
        ));
    }
    edits.sort_by_key(|e| (e.line, e.column));
    edits.dedup();
    let edited = apply(text, &edits)?;
    let mut after_overlay = overlay.clone();
    after_overlay.insert(path.to_owned(), edited);
    let after_overlay: std::collections::HashMap<PathBuf, String> =
        after_overlay.into_iter().collect();
    let after = recheck(&after_overlay);
    if let Some(error) = after.diagnostics.first() {
        let detail = format!("{}:{}: {}", error.line, error.column, error.message);
        if mentions(text, new_name) {
            return Err(format!(
                "`{new_name}` already names something in scope, so after the rename the file would not check: {detail}"
            ));
        }
        return Err(format!(
            "after the rename the file would not check: {detail}"
        ));
    }
    let paths: HashSet<PathBuf> = [path.to_owned()].into_iter().collect();
    let edit_map: HashMap<PathBuf, Vec<TextEdit>> =
        [(path.to_owned(), edits.clone())].into_iter().collect();
    let before = resolved(&places(analysis), &paths, &edit_map);
    let after = resolved(&places(&after), &paths, &HashMap::default());
    if let Some((_, line, column)) = changed(&before, &after) {
        return Err(format!(
            "after the rename, the name at {shown}:{line}:{column} would refer to a different declaration"
        ));
    }
    Ok(Rename {
        name: old.to_owned(),
        new_name: new_name.to_owned(),
        files: vec![FileEdits {
            path: path.to_owned(),
            edits,
        }],
        api_changes: vec![],
    })
}

/// Rename a module-level declaration or a member across its package and
/// the workspace members that depend on it. `analysis` is the cursor file's.
fn declaration(
    analysis: &Analysis,
    (path, cursor): (&Path, &Location),
    identity: &(File, u32, u32),
    (old, qualified, member): (&str, &str, Option<SymbolKind>),
    new_name: &str,
    overlay: &HashMap<PathBuf, String>,
) -> Result<Rename, String> {
    let File::Path(declared) = &identity.0 else {
        return Err("the declaration is in the standard library".into());
    };
    path.parent()
        .and_then(|p| crate::project::discover(p).ok())
        .ok_or(NO_PACKAGE)?;
    let mut package = Package::open(path)?;
    match package.member_of(declared) {
        Some(member) if member != package.member => {
            package = Package::for_member(package.project, member);
        }
        Some(_) => {}
        None => {
            return Err(in_dependency(old));
        }
    }
    let (uses, reach) = package.analyze(overlay, None, Errors::Stop).map_err(|e| {
        format!("the package does not check, so the rename cannot be verified: {e}")
    })?;
    let at =
        |path: &Path, line: u32, column: u32| format!("{}:{line}:{column}", package.relative(path));
    let target = Some(identity.clone());
    let read_by_any =
        reach.reached().contains(path) || reach.files.values().any(|f| f.contains(path));
    if !read_by_any {
        return Err(format!(
            "no target or test of the package reads {}, so a rename cannot find every use",
            package.relative(path)
        ));
    }
    let found = uses.iter().any(|u| {
        u.path == path
            && u.identity == target
            && u.location.line == cursor.line
            && (u.location.column..u.location.column + u.location.length).contains(&cursor.column)
    });
    if !found {
        return Err("the package's check does not match the file's; try the rename again".into());
    }
    // A generic body can name a different declaration in each
    // instantiation; renaming one of them would change the others.
    let mut by_place: BTreeMap<(PathBuf, u32, u32, u32), Vec<&Use>> = BTreeMap::new();
    for u in &uses {
        let l = &u.location;
        by_place
            .entry((u.path.clone(), l.line, l.column, l.length))
            .or_default()
            .push(u);
    }
    for ((path, line, column, _), place) in &by_place {
        if place.iter().any(|u| u.identity == target) && place.iter().any(|u| u.identity != target)
        {
            return Err(format!(
                "the name at {} names a different declaration in each instantiation of a generic body",
                at(path, *line, *column)
            ));
        }
    }
    let type_name = qualified.rsplit_once('.').map_or("", |(o, _)| o);
    if member.is_some() {
        members(
            analysis,
            &package,
            overlay,
            &reach,
            (type_name, old, qualified),
            new_name,
        )?;
    }
    let mut texts: HashMap<PathBuf, String> = HashMap::default();
    let mut edits: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
    // Files where the new name is written bare, so it must not already
    // name something else there. A member is always written after a `.`,
    // as a declaration in its owner, or as a named field of a pattern.
    let mut bare: HashSet<PathBuf> = HashSet::default();
    if member.is_none() {
        bare.insert(declared.clone());
    }
    for u in uses.iter().filter(|u| u.identity == target) {
        if package.member_of(&u.path).is_none() {
            return Err(format!(
                "{} uses `{old}` from outside the workspace, which a rename does not edit",
                package.relative(&u.path)
            ));
        }
        if !texts.contains_key(&u.path) {
            texts.insert(u.path.clone(), read(overlay, &u.path)?);
        }
        let text = &texts[&u.path];
        let location = name_at(text, &u.location);
        let spelled = written(text, &location);
        if spelled != old {
            // A use through `from m import old as alias` keeps the alias.
            let aliased = member.is_none()
                && aliases(text)?
                    .iter()
                    .any(|a| !a.module && a.name == old && a.alias == spelled);
            if aliased {
                continue;
            }
            return Err(format!(
                "cannot rewrite `{spelled}` at {}",
                at(&u.path, location.line, location.column)
            ));
        }
        let line = text
            .lines()
            .nth(location.line as usize - 1)
            .unwrap_or_default();
        if member.is_none() && matches!(form(line, &location), Form::Bare { .. }) {
            bare.insert(u.path.clone());
        }
        edits.entry(u.path.clone()).or_default().push(replace(
            location.line - 1,
            location.column - 1,
            location.length,
            new_name,
        ));
    }
    // The new name must not already name a declaration, import or module
    // where it is written bare. A local or parameter of that name is
    // checked after the edit: it captures the name only where the
    // declaration is used in its scope.
    for u in &uses {
        if !bare.contains(&u.path)
            || matches!(
                u.kind,
                SymbolKind::Field
                    | SymbolKind::Method
                    | SymbolKind::Variant
                    | SymbolKind::Local
                    | SymbolKind::Parameter
            )
        {
            continue;
        }
        if !texts.contains_key(&u.path) {
            texts.insert(u.path.clone(), read(overlay, &u.path)?);
        }
        let location = name_at(&texts[&u.path], &u.location);
        if written(&texts[&u.path], &location) == new_name {
            return Err(format!(
                "`{new_name}` already names something at {}",
                at(&u.path, location.line, location.column)
            ));
        }
    }
    // The analysis does not see an `if const` arm a build does not select.
    let mut read_files: Vec<PathBuf> = reach
        .reached()
        .into_iter()
        .chain(reach.files.values().flatten().cloned())
        .filter(|p| package.member_of(p).is_some())
        .collect();
    read_files.sort();
    read_files.dedup();
    for file in &read_files {
        let text = read(overlay, file)?;
        let names: &[&str] = if bare.contains(file) {
            &[old, new_name]
        } else {
            &[old]
        };
        arms(&text, names, &package.relative(file))?;
    }
    let mut after_overlay = overlay.clone();
    let mut edited = std::collections::BTreeSet::new();
    for (file, file_edits) in &mut edits {
        file_edits.sort_by_key(|e| (e.line, e.column));
        file_edits.dedup();
        after_overlay.insert(file.clone(), apply(&texts[file], file_edits)?);
        edited.insert(file.clone());
    }
    let (after, _) = package
        .analyze(&after_overlay, Some((&reach, &edited)), Errors::Stop)
        .map_err(|e| {
            let taken = texts.values().any(|t| mentions(t, new_name))
                || read_files
                    .iter()
                    .any(|f| read(overlay, f).is_ok_and(|t| mentions(&t, new_name)));
            if taken {
                format!(
                    "`{new_name}` already names something in scope, so after the rename the package would not check: {e}"
                )
            } else {
                format!("after the rename the package would not check: {e}")
            }
        })?;
    let paths: HashSet<PathBuf> = edited.iter().cloned().collect();
    let edit_map: HashMap<PathBuf, Vec<TextEdit>> =
        edits.iter().map(|(p, e)| (p.clone(), e.clone())).collect();
    let before: Vec<Place> = uses.iter().map(from_use).collect();
    let after: Vec<Place> = after.iter().map(from_use).collect();
    let before = resolved(&before, &paths, &edit_map);
    let after = resolved(&after, &paths, &HashMap::default());
    if let Some((path, line, column)) = changed(&before, &after) {
        return Err(format!(
            "after the rename, the name at {} would refer to a different declaration",
            at(&path, line, column)
        ));
    }
    let mut api_changes = vec![];
    let owner = package
        .member_of(declared)
        .map(|m| &package.project.packages[m]);
    let declared_pub = |line: u32| {
        texts
            .get(declared)
            .and_then(|t| t.lines().nth(line as usize - 1))
            .is_some_and(|l| l.trim_start().starts_with("pub "))
    };
    // A member is public when its owner is, and a field or method only with
    // its own `pub`; every variant of a public enum is public.
    let public = match member {
        None => declared_pub(identity.1),
        Some(kind) => {
            let owner_line = analysis
                .symbols
                .iter()
                .find(|s| {
                    s.name == type_name && matches!(s.kind, SymbolKind::Class | SymbolKind::Enum)
                })
                .and_then(|s| s.location.as_ref())
                .map(|l| l.line);
            owner_line.is_some_and(declared_pub)
                && (kind == SymbolKind::Variant || declared_pub(identity.1))
        }
    };
    let shown = match member {
        Some(_) => format!(
            "{}.{old}",
            type_name.rsplit('.').next().unwrap_or(type_name)
        ),
        None => old.to_owned(),
    };
    if let Some(owner) = owner
        && public
        && let Some(library) = &owner.manifest.library
    {
        // Another package imports the library's declarations under the
        // package's name, and another module's under its path.
        let package_name = &owner.manifest.name;
        let src = owner.directory.join("src").canonicalize().ok();
        let module = src
            .as_ref()
            .and_then(|src| declared.strip_prefix(src).ok())
            .map(|relative| {
                relative
                    .with_extension("")
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(".")
            });
        let is_library =
            owner.directory.join(library).canonicalize().ok().as_ref() == Some(declared);
        let path = match module {
            Some(_) if is_library => format!("{package_name}.{shown}"),
            Some(module) => format!("{package_name}.{module}.{shown}"),
            None => format!("{package_name}: {qualified}"),
        };
        api_changes.push(format!(
            "renames public `{path}` to `{new_name}`, so packages outside this workspace that import it must change"
        ));
    }
    Ok(Rename {
        name: old.to_owned(),
        new_name: new_name.to_owned(),
        files: edits
            .into_iter()
            .map(|(path, edits)| FileEdits { path, edits })
            .collect(),
        api_changes,
    })
}

/// Refuse a member rename that would clash with another member of `owner`,
/// or that could miss a use: a place in a generic body that no call
/// instantiates, where the member's name is written on a value whose type
/// the analysis cannot tell.
fn members(
    analysis: &Analysis,
    package: &Package,
    overlay: &HashMap<PathBuf, String>,
    reach: &super::index::Reach,
    (owner, old, qualified): (&str, &str, &str),
    new_name: &str,
) -> Result<(), String> {
    let taken = format!("{owner}.{new_name}");
    if analysis.symbols.iter().any(|s| {
        s.name == taken
            && matches!(
                s.kind,
                SymbolKind::Field | SymbolKind::Method | SymbolKind::Variant
            )
    }) {
        let short = owner.rsplit('.').next().unwrap_or(owner);
        return Err(format!("`{short}` already has a member named `{new_name}`"));
    }
    for (file, location) in &reach.unresolved {
        if package.member_of(file).is_none() {
            continue;
        }
        if written(&read(overlay, file)?, location) == old {
            return Err(format!(
                "`{old}` is written at {}:{}:{} in a generic body that no call instantiates, on a value whose type the editor analysis cannot tell, so a rename cannot tell whether it names `{qualified}`",
                package.relative(file),
                location.line,
                location.column
            ));
        }
    }
    Ok(())
}
