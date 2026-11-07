//! Move a top-level declaration to another module of its package (issue
//! #822). A plan is computed from editor analyses of every file that could
//! refer to the declaration and returned as text edits; nothing is written.
//! Every unsupported case is refused before any edit is returned, and the
//! edited files are checked again before the plan is accepted.
use super::super::lexer::{Kind, Lexer, Token};
use super::{Analysis, File, HashMap, HashSet, Location, SymbolKind, analyze};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

/// Per file: import lines rewritten (line, new text) and imports added.
type ImportChanges = (Vec<(u32, String)>, Vec<String>);

/// A replacement of the text from (`line`, `column`) to (`end_line`,
/// `end_column`). Lines count from 0 and columns are character offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub text: String,
}

/// The edits for one file, against the text the plan read: the overlay's,
/// or the file on disk.
#[derive(Clone, Debug)]
pub struct FileEdits {
    pub path: PathBuf,
    pub edits: Vec<TextEdit>,
}

/// A checked move, ready to preview and apply.
#[derive(Clone, Debug)]
pub struct Move {
    pub name: String,
    pub source: String,
    pub destination: String,
    pub files: Vec<FileEdits>,
    /// Public API changes the move makes, one sentence each.
    pub api_changes: Vec<String>,
}

/// A declaration the cursor is on and the modules it could move to.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub name: String,
    /// `(module path, file)` for each other module of the package's `src/`.
    pub destinations: Vec<(String, PathBuf)>,
}

const KEYWORDS: [&str; 6] = ["def", "class", "enum", "protocol", "const", "type"];
const REFUSED_KEYWORDS: [&str; 4] = ["extern", "opaque", "dimension", "if"];

/// The declaration whose text includes `line` (from 1) of `text`, and the
/// modules of the same package it could move to. This reads no other file
/// than the package's directory listing, so an editor can call it often.
pub fn candidates(path: &Path, text: &str, line: u32) -> Option<Candidate> {
    let path = path.canonicalize().ok()?;
    let src = package_src(&path)?;
    let item = items(text).ok()?.into_iter().find(|i| i.contains(line))?;
    let declaration = item.declaration.ok()?;
    let mut files = vec![];
    source_files(&src, &mut files);
    files.sort();
    let destinations = files
        .into_iter()
        .filter(|f| *f != path)
        .filter_map(|f| Some((module_path(&src, &f)?, f)))
        .collect();
    Some(Candidate {
        name: declaration.name,
        destinations,
    })
}

/// Plan moving the declaration whose text includes `line` (from 1) of
/// `path` to the module in file `destination`. `overlay` holds unsaved text
/// by canonical path. An `Err` explains why the move is refused.
pub fn plan(
    path: &Path,
    line: u32,
    destination: &Path,
    overlay: &std::collections::HashMap<PathBuf, String, impl std::hash::BuildHasher>,
) -> Result<Move, String> {
    let overlay: HashMap<PathBuf, String> = overlay
        .iter()
        .map(|(p, t)| (p.clone(), t.clone()))
        .collect();
    Planner::new(path, destination, &overlay)?.plan(line)
}

/// A top-level item's lines and, for a supported declaration, its name.
struct Item {
    /// First and last line (from 1) of the text that moves with it:
    /// attached comments and annotations through its last body line.
    first: u32,
    last: u32,
    declaration: Result<Declaration, String>,
}

impl Item {
    fn contains(&self, line: u32) -> bool {
        (self.first..=self.last).contains(&line)
    }
}

#[derive(Clone)]
struct Declaration {
    name: String,
    /// The name's position, where the analysis records the declaration.
    line: u32,
    column: u32,
    /// The keyword's position, where `pub ` is inserted; None when public.
    private_at: Option<(u32, u32)>,
}

/// The top-level items of `text`, with each item's attached comments.
fn items(text: &str) -> Result<Vec<Item>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut lexer = Lexer::new(text);
    let mut starts: Vec<Vec<Token>> = vec![];
    loop {
        let mut tokens = VecDeque::new();
        lexer
            .item(&mut tokens)
            .map_err(|e| format!("cannot read the file: {e}"))?;
        let eof = tokens.iter().any(|t| t.kind == Kind::Eof);
        let tokens: Vec<Token> = tokens
            .into_iter()
            .filter(|t| {
                !matches!(
                    t.kind,
                    Kind::Newline | Kind::Indent | Kind::Dedent | Kind::Eof
                )
            })
            .collect();
        if !tokens.is_empty() {
            starts.push(tokens);
        }
        if eof {
            break;
        }
    }
    let blank = |line: u32| {
        lines
            .get(line as usize - 1)
            .is_none_or(|l| l.trim().is_empty())
    };
    let margin_comment = |line: u32| {
        lines
            .get(line as usize - 1)
            .is_some_and(|l| l.starts_with('#'))
    };
    let mut out = vec![];
    for (i, tokens) in starts.iter().enumerate() {
        let start = tokens[0].line;
        let mut last = match starts.get(i + 1) {
            Some(next) => next[0].line - 1,
            None => lines.len() as u32,
        };
        // Margin comments and blank lines before the next item belong to it,
        // or to no declaration.
        while last > start && (blank(last) || margin_comment(last)) {
            last -= 1;
        }
        let mut first = start;
        while first > 1 && margin_comment(first - 1) {
            first -= 1;
        }
        out.push(Item {
            first,
            last,
            declaration: declaration(tokens),
        });
    }
    Ok(out)
}

fn word(token: &Token) -> Option<&str> {
    match &token.kind {
        Kind::Word(w) => Some(w),
        _ => None,
    }
}

fn declaration(tokens: &[Token]) -> Result<Declaration, String> {
    let mut words = tokens
        .iter()
        .filter(|t| t.column == 1 || word(t).is_some())
        .skip_while(|t| !(t.column == 1 && word(t).is_some()));
    let first = words.next().ok_or("this is not a declaration")?;
    let (public, keyword) = if word(first) == Some("pub") {
        (true, words.next().ok_or("this is not a declaration")?)
    } else {
        (false, first)
    };
    let keyword_text = word(keyword).unwrap_or_default();
    if REFUSED_KEYWORDS.contains(&keyword_text) {
        return Err(format!(
            "moving `{keyword_text}` declarations is not supported"
        ));
    }
    if !KEYWORDS.contains(&keyword_text) {
        return Err("this is not a declaration that can move".into());
    }
    let name = words
        .next()
        .and_then(word)
        .ok_or("this declaration has no name")?;
    let at = tokens
        .iter()
        .find(|t| word(t) == Some(name) && t.line >= keyword.line)
        .ok_or("this declaration has no name")?;
    Ok(Declaration {
        name: name.to_owned(),
        line: at.line,
        column: at.column,
        private_at: (!public).then_some((keyword.line, keyword.column)),
    })
}

/// One `import` or `from ... import` statement at the left margin.
#[derive(Clone, Debug)]
struct Import {
    /// The statement's line, from 0.
    line: u32,
    /// The module of a `from` statement.
    from: Option<String>,
    /// `(module path or item name, alias)`.
    items: Vec<(String, Option<String>)>,
    /// Whether the line carries a comment, which a rewrite would lose.
    comment: bool,
}

impl Import {
    fn binding(item: &(String, Option<String>)) -> &str {
        item.1.as_deref().unwrap_or(&item.0)
    }

    fn render(&self) -> String {
        let items: Vec<String> = self
            .items
            .iter()
            .map(|(name, alias)| match alias {
                Some(alias) => format!("{name} as {alias}"),
                None => name.clone(),
            })
            .collect();
        match &self.from {
            Some(module) => format!("from {module} import {}", items.join(", ")),
            None => format!("import {}", items.join(", ")),
        }
    }
}

fn parse_item(text: &str) -> Option<(String, Option<String>)> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    match parts.as_slice() {
        [name] => Some(((*name).to_owned(), None)),
        [name, "as", alias] => Some(((*name).to_owned(), Some((*alias).to_owned()))),
        _ => None,
    }
}

/// The file's import statements, or an error when one cannot be rewritten
/// safely: an indented import, or one continued past its line.
fn imports(text: &str) -> Result<Vec<Import>, String> {
    let mut out = vec![];
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let is_import = trimmed.starts_with("import ") || trimmed.starts_with("from ");
        if !is_import {
            continue;
        }
        if trimmed.len() != line.len() {
            return Err(format!(
                "line {} has an indented import, which the move cannot rewrite",
                i + 1
            ));
        }
        let (code, comment) = match line.split_once('#') {
            Some((code, _)) => (code, true),
            None => (line, false),
        };
        let unreadable = || format!("cannot read the import on line {}", i + 1);
        let (from, list) = if let Some(rest) = code.strip_prefix("from ") {
            let (module, list) = rest.split_once(" import ").ok_or_else(unreadable)?;
            (Some(module.trim().to_owned()), list)
        } else {
            (None, &code["import ".len()..])
        };
        let items = list
            .split(',')
            .map(parse_item)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(unreadable)?;
        out.push(Import {
            line: i as u32,
            from,
            items,
            comment,
        });
    }
    Ok(out)
}

/// How a reference names its symbol.
#[derive(Clone, Debug, PartialEq)]
enum Form {
    /// A bare name: a declaration of the same module or a `from` import.
    Bare { column: u32, name: String },
    /// `qualifier.name`, where the qualifier names a module.
    Qualified {
        column: u32,
        qualifier: String,
        name: String,
    },
}

fn form(line: &str, location: &Location) -> Form {
    let chars: Vec<char> = line.chars().collect();
    let start = location.column as usize - 1;
    let end = (start + location.length as usize).min(chars.len());
    let written: String = chars[start.min(end)..end].iter().collect();
    if let Some((qualifier, name)) = written.rsplit_once('.') {
        return Form::Qualified {
            column: start as u32,
            qualifier: qualifier.to_owned(),
            name: name.to_owned(),
        };
    }
    if start > 0 && chars[start - 1] == '.' {
        let mut begin = start - 1;
        while begin > 0 && (chars[begin - 1].is_alphanumeric() || "_.".contains(chars[begin - 1])) {
            begin -= 1;
        }
        let qualifier: String = chars[begin..start - 1].iter().collect();
        if !qualifier.is_empty() && !qualifier.starts_with(|c: char| c.is_ascii_digit()) {
            return Form::Qualified {
                column: begin as u32,
                qualifier,
                name: written,
            };
        }
    }
    Form::Bare {
        column: start as u32,
        name: written,
    }
}

/// A reference from any analysis, keyed by where it is written.
#[derive(Clone, Debug)]
struct Use {
    path: PathBuf,
    location: Location,
    /// The declaration's position: `(file, line, column)`; None for `std`.
    target: Option<(PathBuf, u32, u32)>,
    kind: SymbolKind,
}

/// A file the move reads: its text and how its module is imported.
struct Source {
    text: String,
    imports: Result<Vec<Import>, String>,
}

struct Planner<'a> {
    overlay: &'a HashMap<PathBuf, String>,
    project: crate::project::Project,
    member: usize,
    source_path: PathBuf,
    destination_path: PathBuf,
    source_module: String,
    destination_module: String,
    /// Every file analyzed as its own entry, with its graph.
    roots: Vec<(PathBuf, crate::project::SourceGraph)>,
    /// The files of `roots` that are package targets, whose reach matters.
    entries: BTreeSet<PathBuf>,
}

fn package_src(path: &Path) -> Option<PathBuf> {
    let directory = crate::project::discover(path.parent()?).ok()?;
    let manifest = directory.join("src");
    let src = manifest.canonicalize().ok()?;
    path.starts_with(&src).then_some(src)
}

fn source_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            source_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "plenty")
            && let Ok(path) = path.canonicalize()
        {
            files.push(path);
        }
    }
}

fn module_path(src: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(src).ok()?.with_extension("");
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(parts.join("."))
}

fn location_line(location: &Location) -> u32 {
    location.line - 1
}

impl<'a> Planner<'a> {
    fn new(
        path: &Path,
        destination: &Path,
        overlay: &'a HashMap<PathBuf, String>,
    ) -> Result<Self, String> {
        let source_path = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let destination_path = destination
            .canonicalize()
            .map_err(|e| format!("{}: {e}", destination.display()))?;
        let directory = source_path
            .parent()
            .and_then(|p| crate::project::discover(p).ok())
            .ok_or("moving a declaration needs a package: this file has no plenty.toml")?;
        let options = crate::project::ResolveOptions {
            locked: true,
            offline: true,
            ..Default::default()
        };
        let project = crate::project::Project::load(&directory, &options)
            .map_err(|e| format!("cannot load the package: {e}"))?;
        let found = project.members.iter().copied().find(|&m| {
            project.packages[m]
                .directory
                .join("src")
                .canonicalize()
                .is_ok_and(|src| source_path.starts_with(src))
        });
        let member = found.ok_or("only files below a package's src/ can move declarations")?;
        let package = &project.packages[member];
        let src = package
            .directory
            .join("src")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !destination_path.starts_with(&src)
            || destination_path.extension().is_none_or(|e| e != "plenty")
        {
            return Err("the destination must be a module of the same package's src/".into());
        }
        if destination_path == source_path {
            return Err("the destination is the declaration's own module".into());
        }
        let source_module = module_path(&src, &source_path).ok_or("source module path")?;
        let destination_module =
            module_path(&src, &destination_path).ok_or("destination module path")?;
        // Every file of this package and of workspace members that depend on
        // it, each checked as its own entry so files no target imports are
        // still read.
        let mut roots = vec![];
        let mut entries = BTreeSet::new();
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
                    entries.insert(path);
                }
            }
            roots.extend(files.into_iter().map(|f| (f, graph.clone())));
            let mut tests = vec![];
            source_files(&package.directory.join("tests"), &mut tests);
            tests.sort();
            if !tests.is_empty() {
                let graph = project.test_graph(m);
                entries.extend(tests.iter().cloned());
                roots.extend(tests.into_iter().map(|f| (f, graph.clone())));
            }
        }
        Ok(Planner {
            overlay,
            project,
            member,
            source_path,
            destination_path,
            source_module,
            destination_module,
            roots,
            entries,
        })
    }

    fn text(&self, path: &Path) -> Result<String, String> {
        match self.overlay.get(path) {
            Some(text) => Ok(text.clone()),
            None => std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())),
        }
    }

    /// Analyze every root with `overlay`; the references found and the
    /// files the package targets reach.
    fn analyze(
        &self,
        overlay: &HashMap<PathBuf, String>,
    ) -> Result<(Vec<Use>, BTreeSet<PathBuf>), String> {
        let mut uses: BTreeMap<(PathBuf, u32, u32, u32), Use> = BTreeMap::new();
        let mut reached = BTreeSet::new();
        for (root, graph) in &self.roots {
            let analysis: Analysis = analyze(root, Some(graph), overlay.clone());
            if let Some(error) = analysis.diagnostics.first() {
                let at = match &error.file {
                    Some(File::Path(path)) => {
                        format!("{}:{}:{}: ", self.relative(path), error.line, error.column)
                    }
                    _ => String::new(),
                };
                return Err(format!("{at}{}", error.message));
            }
            if self.entries.contains(root) {
                reached.extend(analysis.sources.keys().filter_map(|f| match f {
                    File::Path(path) => Some(path.clone()),
                    File::Standard(_) => None,
                }));
            }
            for reference in &analysis.references {
                let File::Path(path) = &reference.location.file else {
                    continue;
                };
                let symbol = &analysis.symbols[reference.symbol];
                let target = match &symbol.location {
                    Some(Location {
                        file: File::Path(file),
                        line,
                        column,
                        ..
                    }) => Some((file.clone(), *line, *column)),
                    _ => None,
                };
                let l = &reference.location;
                uses.entry((path.clone(), l.line, l.column, l.length))
                    .or_insert_with(|| Use {
                        path: path.clone(),
                        location: l.clone(),
                        target,
                        kind: symbol.kind,
                    });
            }
        }
        Ok((uses.into_values().collect(), reached))
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.project.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    fn plan(&self, line: u32) -> Result<Move, String> {
        let mut overlay = self.overlay.clone();
        let source_text = self.text(&self.source_path)?;
        let destination_text = self.text(&self.destination_path)?;
        let all_items = items(&source_text)?;
        let item = all_items
            .iter()
            .find(|i| i.contains(line))
            .ok_or("no top-level declaration here")?;
        let declaration = item.declaration.clone()?;
        let name = declaration.name.clone();
        let (span_first, span_last) = (item.first - 1, item.last - 1);
        let in_span = |path: &Path, l: &Location| {
            path == self.source_path && (span_first..=span_last).contains(&location_line(l))
        };
        let (uses, reached_before) = self
            .analyze(&overlay)
            .map_err(|e| format!("fix this error before moving `{name}`: {e}"))?;
        let target = (
            self.source_path.clone(),
            declaration.line,
            declaration.column,
        );
        if !uses.iter().any(|u| u.target.as_ref() == Some(&target)) {
            return Err(format!("the checker found no declaration `{name}` here"));
        }
        let mut sources: HashMap<PathBuf, Source> = HashMap::default();
        let mut source = |path: &Path| -> Result<(), String> {
            if !sources.contains_key(path) {
                let text = self.text(path)?;
                let imports = imports(&text);
                sources.insert(path.to_owned(), Source { text, imports });
            }
            Ok(())
        };
        source(&self.source_path)?;
        source(&self.destination_path)?;
        for u in &uses {
            source(&u.path)?;
        }
        let line_text = |path: &Path, line: u32| -> String {
            sources[path]
                .text
                .lines()
                .nth(line as usize)
                .unwrap_or_default()
                .to_owned()
        };
        let import_lines = |path: &Path| -> HashSet<u32> {
            match &sources[path].imports {
                Ok(imports) => imports.iter().map(|i| i.line).collect(),
                Err(_) => HashSet::default(),
            }
        };
        let file_imports = |path: &Path| -> Result<Vec<Import>, String> {
            sources[path]
                .imports
                .clone()
                .map_err(|e| format!("{}: {e}", self.relative(path)))
        };
        let source_imports = file_imports(&self.source_path)?;
        let destination_imports = file_imports(&self.destination_path)?;
        let destination_items = items(&destination_text)?;

        let is_declaration = |kind: SymbolKind| {
            matches!(
                kind,
                SymbolKind::Function
                    | SymbolKind::Class
                    | SymbolKind::Enum
                    | SymbolKind::Protocol
                    | SymbolKind::Constant
                    | SymbolKind::TypeAlias
            )
        };
        let mut edits: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
        let mut span_edits: Vec<TextEdit> = vec![];
        let mut api_changes = vec![];
        let library = self.project.packages[self.member]
            .manifest
            .library
            .is_some();
        let package_name = self.project.packages[self.member].manifest.name.clone();

        // Names the moved text uses that the destination must bind the same
        // way, and the declarations of the source module it depends on.
        let mut dependencies: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        let destination_names: BTreeSet<String> = destination_items
            .iter()
            .filter_map(|i| i.declaration.as_ref().ok().map(|d| d.name.clone()))
            .collect();
        let source_declarations: HashMap<(u32, u32), Declaration> = all_items
            .iter()
            .filter_map(|i| i.declaration.as_ref().ok())
            .map(|d| ((d.line, d.column), d.clone()))
            .collect();
        let span_uses: Vec<&Use> = uses
            .iter()
            .filter(|u| in_span(&u.path, &u.location))
            .collect();
        for u in &span_uses {
            let Some((file, line, declared_column)) = &u.target else {
                continue;
            };
            let declared_in_span =
                *file == self.source_path && (span_first + 1..=span_last + 1).contains(line);
            if declared_in_span || matches!(u.kind, SymbolKind::Local | SymbolKind::Parameter) {
                continue;
            }
            let text = line_text(&u.path, location_line(&u.location));
            if *file == self.source_path {
                if !is_declaration(u.kind) {
                    continue;
                }
                let Some(dependency) = source_declarations.get(&(*line, *declared_column)) else {
                    return Err(format!(
                        "`{name}` uses a declaration of `{}` the move cannot import",
                        self.source_module
                    ));
                };
                if let Form::Bare { name: written, .. } = form(&text, &u.location)
                    && written == dependency.name
                {
                    dependencies.insert(dependency.name.clone(), (*line, *declared_column));
                    if let Some(at) = dependency.private_at {
                        edits
                            .entry(self.source_path.clone())
                            .or_default()
                            .push(insert(at.0 - 1, at.1 - 1, "pub "));
                        api_changes.push(format!(
                            "makes `{}.{}` public, because the moved code uses it",
                            self.source_module, dependency.name
                        ));
                    }
                    continue;
                }
                return Err(format!(
                    "`{name}` refers to `{}` in a form the move cannot rewrite",
                    dependency.name
                ));
            }
            if *file == self.destination_path {
                // The destination's own declaration: write its bare name.
                match form(&text, &u.location) {
                    Form::Bare {
                        column,
                        name: written,
                    } => {
                        let real = destination_declaration_name(
                            &destination_items,
                            *line,
                            *declared_column,
                        )
                        .unwrap_or(written.clone());
                        if real != written {
                            span_edits.push(replace(
                                location_line(&u.location) - span_first,
                                column,
                                written.chars().count() as u32,
                                &real,
                            ));
                        }
                    }
                    Form::Qualified {
                        column,
                        qualifier,
                        name: written,
                    } => {
                        let length = qualifier.chars().count() as u32 + 1;
                        let _ = written;
                        span_edits.push(replace(
                            location_line(&u.location) - span_first,
                            column,
                            length,
                            "",
                        ));
                    }
                }
                continue;
            }
            if let Form::Bare { name: written, .. } = form(&text, &u.location)
                && destination_names.contains(&written)
            {
                return Err(format!(
                    "`{name}` uses `{written}`, which `{}` declares as something else",
                    self.destination_module
                ));
            }
        }
        if destination_names.contains(&name) {
            return Err(format!(
                "`{}` already declares `{name}`",
                self.destination_module
            ));
        }

        // Imports of the source module that the moved text uses.
        let qualified_by = |u: &Use, binding: &str| -> bool {
            let text = line_text(&u.path, location_line(&u.location));
            matches!(form(&text, &u.location), Form::Qualified { qualifier, .. } if qualifier == binding)
        };
        let symbol_of_item =
            |path: &Path, import: &Import, item: &str| -> Option<Option<(PathBuf, u32, u32)>> {
                uses.iter()
                    .find(|u| {
                        u.path == path
                            && location_line(&u.location) == import.line
                            && line_text(path, import.line)
                                .chars()
                                .skip(u.location.column as usize - 1)
                                .take(u.location.length as usize)
                                .collect::<String>()
                                == item
                    })
                    .map(|u| u.target.clone())
            };
        let mut destination_bindings: HashMap<String, String> = HashMap::default();
        for import in &destination_imports {
            for item in &import.items {
                let binding = Import::binding(item);
                let first = binding.split('.').next().unwrap_or(binding).to_owned();
                destination_bindings.insert(first, render_one(import, item));
            }
        }
        let mut new_destination_imports: Vec<String> = vec![];
        let mut source_import_removals: Vec<(u32, String)> = vec![];
        let source_lines = import_lines(&self.source_path);
        let outside = |u: &Use| -> bool {
            u.path == self.source_path
                && !in_span(&u.path, &u.location)
                && !source_lines.contains(&location_line(&u.location))
        };
        for import in &source_imports {
            for item in &import.items {
                let binding = Import::binding(item).to_owned();
                let (used_in_span, used_outside, of_destination) = match &import.from {
                    None => (
                        span_uses.iter().any(|u| qualified_by(u, &binding)),
                        uses.iter().any(|u| outside(u) && qualified_by(u, &binding)),
                        item.0 == self.destination_module,
                    ),
                    Some(module) => {
                        let symbol = symbol_of_item(&self.source_path, import, &item.0);
                        let Some(symbol) = symbol else {
                            // A name the analysis did not record: keep it.
                            continue;
                        };
                        let bare = |u: &&Use| {
                            u.target == symbol
                                && matches!(
                                    form(
                                        &line_text(&u.path, location_line(&u.location)),
                                        &u.location
                                    ),
                                    Form::Bare { .. }
                                )
                        };
                        (
                            span_uses.iter().any(&bare),
                            uses.iter().filter(|u| outside(u)).any(|u| bare(&u)),
                            *module == self.destination_module,
                        )
                    }
                };
                if !used_in_span {
                    continue;
                }
                if !used_outside {
                    source_import_removals.push((import.line, binding.clone()));
                }
                if of_destination {
                    if import.from.is_some() && item.1.is_some() {
                        // Handled by renaming the alias to the real name above.
                    }
                    continue;
                }
                let rendered = render_one(import, item);
                let first = binding.split('.').next().unwrap_or(&binding).to_owned();
                match destination_bindings.get(&first) {
                    Some(existing) if *existing == rendered => {}
                    Some(existing) => {
                        return Err(format!(
                            "`{name}` needs `{rendered}`, but `{}` already has `{existing}`",
                            self.destination_module
                        ));
                    }
                    None if destination_names.contains(&first) => {
                        return Err(format!(
                            "`{name}` needs `{rendered}`, but `{}` declares `{first}`",
                            self.destination_module
                        ));
                    }
                    None => {
                        destination_bindings.insert(first, rendered.clone());
                        new_destination_imports.push(rendered);
                    }
                }
            }
        }
        for dependency in dependencies.keys() {
            if destination_bindings.contains_key(dependency)
                || destination_names.contains(dependency)
            {
                return Err(format!(
                    "`{name}` uses `{}.{dependency}`, but `{}` already binds `{dependency}`",
                    self.source_module, self.destination_module
                ));
            }
            new_destination_imports
                .push(format!("from {} import {dependency}", self.source_module));
        }

        // References to the moved declaration outside the moved text.
        let target_uses: Vec<&Use> = uses
            .iter()
            .filter(|u| u.target.as_ref() == Some(&target) && !in_span(&u.path, &u.location))
            .collect();
        let mut needs_public = false;
        let mut file_import_changes: BTreeMap<PathBuf, ImportChanges> = BTreeMap::new();
        let mut source_uses_name = false;
        let mut by_file: BTreeMap<PathBuf, Vec<&Use>> = BTreeMap::new();
        for u in &target_uses {
            by_file.entry(u.path.clone()).or_default().push(u);
        }
        for (path, file_uses) in &by_file {
            let lines = import_lines(path);
            let path_imports = file_imports(path)?;
            if *path == self.source_path {
                if file_uses
                    .iter()
                    .any(|u| !lines.contains(&location_line(&u.location)))
                {
                    source_uses_name = true;
                    needs_public = true;
                }
                continue;
            }
            let is_destination = *path == self.destination_path;
            if !is_destination {
                needs_public = true;
            }
            // Where this file imports the source module, and the path it uses.
            let mut module_paths = BTreeSet::new();
            let (removals, additions) = file_import_changes.entry(path.clone()).or_default();
            for import in &path_imports {
                for item in &import.items {
                    let module = import.from.as_ref().unwrap_or(&item.0);
                    if !module.ends_with(&self.source_module)
                        || !(module.len() == self.source_module.len()
                            || module[..module.len() - self.source_module.len()].ends_with('.'))
                    {
                        continue;
                    }
                    let prefix = &module[..module.len() - self.source_module.len()];
                    if import.from.is_some() {
                        if item.0 != name {
                            continue;
                        }
                        if import.comment {
                            return Err(format!(
                                "{}:{}: an import with a comment cannot be rewritten",
                                self.relative(path),
                                import.line + 1
                            ));
                        }
                        module_paths.insert(prefix.to_owned());
                        removals.push((import.line, Import::binding(item).to_owned()));
                        if !is_destination {
                            let alias = item
                                .1
                                .as_ref()
                                .map(|a| format!(" as {a}"))
                                .unwrap_or_default();
                            additions.push(format!(
                                "from {prefix}{} import {name}{alias}",
                                self.destination_module
                            ));
                        }
                    } else {
                        module_paths.insert(prefix.to_owned());
                    }
                }
            }
            let mut qualifier_for_destination: Option<String> = None;
            for u in file_uses {
                let l = location_line(&u.location);
                if lines.contains(&l) {
                    continue;
                }
                let text = line_text(path, l);
                match form(&text, &u.location) {
                    Form::Bare {
                        column,
                        name: written,
                    } => {
                        if is_destination && written != name {
                            edits.entry(path.clone()).or_default().push(replace(
                                l,
                                column,
                                written.chars().count() as u32,
                                &name,
                            ));
                        }
                    }
                    Form::Qualified {
                        column,
                        qualifier,
                        name: written,
                    } => {
                        let binds_source = path_imports.iter().any(|i| {
                            i.from.is_none()
                                && i.items.iter().any(|item| {
                                    Import::binding(item) == qualifier
                                        && module_paths
                                            .iter()
                                            .any(|p| item.0 == format!("{p}{}", self.source_module))
                                })
                        });
                        if !binds_source || written != name {
                            return Err(format!(
                                "{}:{}:{}: cannot rewrite this reference to `{name}`",
                                self.relative(path),
                                u.location.line,
                                u.location.column
                            ));
                        }
                        let length =
                            qualifier.chars().count() as u32 + 1 + written.chars().count() as u32;
                        let replacement = if is_destination {
                            name.clone()
                        } else {
                            let qualifier = qualifier_for_destination.get_or_insert_with(|| {
                                let prefix =
                                    module_paths.iter().next().cloned().unwrap_or_default();
                                let module = format!("{prefix}{}", self.destination_module);
                                let existing = path_imports.iter().find_map(|i| {
                                    i.from.is_none().then(|| {
                                        i.items
                                            .iter()
                                            .find(|item| item.0 == module)
                                            .map(|item| Import::binding(item).to_owned())
                                    })?
                                });
                                existing.unwrap_or_else(|| {
                                    additions.push(format!("import {module}"));
                                    module
                                })
                            });
                            format!("{qualifier}.{name}")
                        };
                        edits.entry(path.clone()).or_default().push(replace(
                            l,
                            column,
                            length,
                            &replacement,
                        ));
                        // Remove `import source` when this was its last use.
                        let other_uses = uses.iter().any(|o| {
                            o.path == *path
                                && o.target.as_ref() != Some(&target)
                                && !lines.contains(&location_line(&o.location))
                                && qualified_by(o, &qualifier)
                        });
                        if !other_uses && !removals.iter().any(|(_, b)| *b == qualifier) {
                            let import = path_imports
                                .iter()
                                .find(|i| {
                                    i.from.is_none()
                                        && i.items
                                            .iter()
                                            .any(|item| Import::binding(item) == qualifier)
                                })
                                .expect("the qualifier's import");
                            if import.comment {
                                return Err(format!(
                                    "{}:{}: an import with a comment cannot be rewritten",
                                    self.relative(path),
                                    import.line + 1
                                ));
                            }
                            removals.push((import.line, qualifier.clone()));
                        }
                    }
                }
            }
        }
        if source_uses_name {
            if !dependencies.is_empty() {
                return Err(format!(
                    "moving `{name}` would make `{}` and `{}` import each other: `{name}` uses {} and the rest of `{}` uses `{name}`",
                    self.source_module,
                    self.destination_module,
                    dependencies
                        .keys()
                        .map(|d| format!("`{d}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    self.source_module,
                ));
            }
            file_import_changes
                .entry(self.source_path.clone())
                .or_default()
                .1
                .push(format!("from {} import {name}", self.destination_module));
        }
        {
            let entry = file_import_changes
                .entry(self.source_path.clone())
                .or_default();
            entry.0.extend(source_import_removals);
        }
        file_import_changes
            .entry(self.destination_path.clone())
            .or_default()
            .1
            .extend(new_destination_imports);

        // The moved text.
        let source_lines_all: Vec<&str> = source_text.lines().collect();
        let mut moved: String =
            source_lines_all[span_first as usize..=span_last as usize].join("\n");
        moved.push('\n');
        if let Some(at) = declaration.private_at
            && needs_public
        {
            span_edits.push(insert(at.0 - 1 - span_first, at.1 - 1, "pub "));
            api_changes.push(format!(
                "makes `{name}` public, because other modules use it"
            ));
        }
        if declaration.private_at.is_none() && library {
            api_changes.push(format!(
                "moves public `{package_name}.{}.{name}` to `{package_name}.{}.{name}`; packages outside this workspace that import it must change",
                self.source_module, self.destination_module
            ));
        }
        let moved = apply(&moved, &span_edits)?;

        // Remove the declaration and the blank lines after it.
        let mut end = span_last + 1;
        while (end as usize) < source_lines_all.len()
            && source_lines_all[end as usize].trim().is_empty()
        {
            end += 1;
        }
        let mut start = span_first;
        if end as usize >= source_lines_all.len() {
            while start > 0 && source_lines_all[start as usize - 1].trim().is_empty() {
                start -= 1;
            }
        }
        edits
            .entry(self.source_path.clone())
            .or_default()
            .push(TextEdit {
                line: start,
                column: 0,
                end_line: end,
                end_column: 0,
                text: String::new(),
            });
        let destination_lines: Vec<&str> = destination_text.lines().collect();
        let last_line = destination_lines.len() as u32;
        let ends_with_newline = destination_text.is_empty() || destination_text.ends_with('\n');
        let mut appended = String::new();
        if !ends_with_newline {
            appended.push('\n');
        }
        if destination_lines.iter().any(|l| !l.trim().is_empty()) {
            appended.push('\n');
        }
        appended.push_str(&moved);
        let (append_line, append_column) = if ends_with_newline {
            (last_line, 0)
        } else {
            (
                last_line - 1,
                destination_lines
                    .last()
                    .map_or(0, |l| l.chars().count() as u32),
            )
        };
        edits
            .entry(self.destination_path.clone())
            .or_default()
            .push(insert(append_line, append_column, &appended));

        // Import statements.
        for (path, (removals, additions)) in &file_import_changes {
            let path_imports = file_imports(path)?;
            let file_edits = edits.entry(path.clone()).or_default();
            let mut changed: BTreeMap<u32, Import> = BTreeMap::new();
            for (line, binding) in removals {
                let import = changed.entry(*line).or_insert_with(|| {
                    path_imports
                        .iter()
                        .find(|i| i.line == *line)
                        .expect("import")
                        .clone()
                });
                if import.comment {
                    return Err(format!(
                        "{}:{}: an import with a comment cannot be rewritten",
                        self.relative(path),
                        line + 1
                    ));
                }
                import.items.retain(|item| Import::binding(item) != binding);
            }
            // Removing every import also removes the blank lines after them.
            let all_removed = additions.is_empty()
                && path_imports
                    .iter()
                    .all(|i| changed.get(&i.line).is_some_and(|c| c.items.is_empty()));
            let file_lines: Vec<&str> = sources[path].text.lines().collect();
            for (line, import) in &changed {
                if import.items.is_empty() {
                    let mut end = line + 1;
                    if all_removed && Some(*line) == path_imports.last().map(|i| i.line) {
                        while file_lines
                            .get(end as usize)
                            .is_some_and(|l| l.trim().is_empty())
                        {
                            end += 1;
                        }
                    }
                    file_edits.push(TextEdit {
                        line: *line,
                        column: 0,
                        end_line: end,
                        end_column: 0,
                        text: String::new(),
                    });
                } else {
                    let length = line_text(path, *line).chars().count() as u32;
                    file_edits.push(replace(*line, 0, length, &import.render()));
                }
            }
            let existing: HashSet<String> = path_imports.iter().map(|i| i.render()).collect();
            let mut added = vec![];
            for addition in additions {
                if !existing.contains(addition) && !added.contains(addition) {
                    added.push(addition.clone());
                }
            }
            if added.is_empty() {
                continue;
            }
            let mut text = added.join("\n");
            text.push('\n');
            let at = match path_imports.last() {
                Some(last) => last.line + 1,
                None => {
                    let starts_with_code = sources[path]
                        .text
                        .lines()
                        .next()
                        .is_some_and(|l| !l.trim().is_empty());
                    if starts_with_code {
                        text.push('\n');
                    }
                    0
                }
            };
            file_edits.push(insert(at, 0, &text));
        }

        // Apply the edits and check the result.
        let mut files = vec![];
        for (path, mut file_edits) in edits {
            if file_edits.is_empty() {
                continue;
            }
            file_edits.sort_by_key(|e| (e.line, e.column, e.end_line, e.end_column));
            // Removed imports and the removed declaration can share blank
            // lines; LSP edits may not overlap.
            let mut merged: Vec<TextEdit> = vec![];
            for edit in file_edits {
                if let Some(last) = merged.last_mut()
                    && last.text.is_empty()
                    && edit.text.is_empty()
                    && (edit.line, edit.column) <= (last.end_line, last.end_column)
                {
                    if (edit.end_line, edit.end_column) > (last.end_line, last.end_column) {
                        last.end_line = edit.end_line;
                        last.end_column = edit.end_column;
                    }
                    continue;
                }
                merged.push(edit);
            }
            let file_edits = merged;
            let text = &sources[&path].text;
            let new_text = apply(text, &file_edits)?;
            overlay.insert(path.clone(), new_text);
            files.push(FileEdits {
                path,
                edits: file_edits,
            });
        }
        let (_, reached_after) = self
            .analyze(&overlay)
            .map_err(|e| format!("after moving `{name}`, the package would not check: {e}"))?;
        // A module left without declarations may stop being imported; any
        // other file must stay checked.
        let empty = |path: &Path| {
            overlay.get(path).is_some_and(|text| {
                text.lines().all(|l| {
                    let l = l.trim_start();
                    l.is_empty()
                        || l.starts_with('#')
                        || l.starts_with("import ")
                        || l.starts_with("from ")
                })
            })
        };
        if let Some(lost) = reached_before
            .difference(&reached_after)
            .find(|path| !empty(path))
        {
            return Err(format!(
                "after moving `{name}`, no target would import {}",
                self.relative(lost)
            ));
        }
        Ok(Move {
            name,
            source: self.source_module.clone(),
            destination: self.destination_module.clone(),
            files,
            api_changes,
        })
    }
}

fn destination_declaration_name(items: &[Item], line: u32, column: u32) -> Option<String> {
    items
        .iter()
        .filter_map(|i| i.declaration.as_ref().ok())
        .find(|d| d.line == line && d.column == column)
        .map(|d| d.name.clone())
}

fn render_one(import: &Import, item: &(String, Option<String>)) -> String {
    Import {
        line: import.line,
        from: import.from.clone(),
        items: vec![item.clone()],
        comment: false,
    }
    .render()
}

fn insert(line: u32, column: u32, text: &str) -> TextEdit {
    TextEdit {
        line,
        column,
        end_line: line,
        end_column: column,
        text: text.to_owned(),
    }
}

fn replace(line: u32, column: u32, length: u32, text: &str) -> TextEdit {
    TextEdit {
        line,
        column,
        end_line: line,
        end_column: column + length,
        text: text.to_owned(),
    }
}

/// The text with `edits` applied; edits may not overlap.
pub fn apply(text: &str, edits: &[TextEdit]) -> Result<String, String> {
    let mut starts = vec![0];
    for (i, c) in text.char_indices() {
        if c == '\n' {
            starts.push(i + 1);
        }
    }
    let offset = |line: u32, column: u32| -> usize {
        let Some(&start) = starts.get(line as usize) else {
            return text.len();
        };
        text[start..]
            .char_indices()
            .nth(column as usize)
            .map_or(text.len(), |(i, _)| start + i)
    };
    let mut ranges: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|e| {
            (
                offset(e.line, e.column),
                offset(e.end_line, e.end_column),
                e.text.as_str(),
            )
        })
        .collect();
    ranges.sort_by_key(|r| (r.0, r.1));
    let mut out = String::new();
    let mut at = 0;
    for (start, end, replacement) in ranges {
        if start < at {
            return Err("internal error: the move's edits overlap".into());
        }
        out.push_str(&text[at..start]);
        out.push_str(replacement);
        at = end;
    }
    out.push_str(&text[at..]);
    Ok(out)
}
