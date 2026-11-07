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
    /// Whether it reads a file by a path relative to its module's directory.
    includes: bool,
    import: bool,
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

/// The tokens of each top-level item of `text`, without layout tokens.
fn top_level(text: &str) -> Result<Vec<Vec<Token>>, String> {
    let mut lexer = Lexer::new(text);
    let mut out: Vec<Vec<Token>> = vec![];
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
            out.push(tokens);
        }
        if eof {
            break;
        }
    }
    Ok(out)
}

fn is_import(tokens: &[Token]) -> bool {
    tokens
        .first()
        .is_some_and(|t| t.column == 1 && (t.is("import") || t.is("from")))
}

/// The top-level items of `text`, with each item's attached comments.
fn items(text: &str) -> Result<Vec<Item>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let starts = top_level(text)?;
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
            includes: tokens
                .iter()
                .any(|t| t.is("include_str") || t.is("include_bytes")),
            import: is_import(tokens),
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
    /// The column (from 1) of each module path written: the `from`
    /// module's, or each item's.
    module_columns: Vec<u32>,
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

/// A dotted module path and its column, where the analysis records the
/// module's reference.
fn dotted<'a>(
    tokens: &mut std::iter::Peekable<impl Iterator<Item = &'a Token>>,
) -> Option<(String, u32)> {
    let first = tokens.next()?;
    let mut path: String = word(first)?.to_owned();
    while tokens.peek().is_some_and(|t| t.is(".")) {
        tokens.next();
        path.push('.');
        path.push_str(word(tokens.next()?)?);
    }
    Some((path, first.column))
}

/// The file's import statements, from its top-level items, or an error
/// when one cannot be rewritten safely: an import inside an `if const` block.
fn imports(text: &str) -> Result<Vec<Import>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    for tokens in top_level(text)? {
        if !is_import(&tokens) {
            if let Some(nested) = tokens.iter().find(|t| t.is("import")) {
                return Err(format!(
                    "line {} has an indented import, which the move cannot rewrite",
                    nested.line
                ));
            }
            continue;
        }
        let line = tokens[0].line;
        let unreadable = || format!("cannot read the import on line {line}");
        if tokens.iter().any(|t| t.line != line) {
            return Err(unreadable());
        }
        let mut words = tokens.iter().peekable();
        let mut module_columns = vec![];
        let from = if words.next().is_some_and(|t| t.is("from")) {
            let (module, column) = dotted(&mut words).ok_or_else(unreadable)?;
            module_columns.push(column);
            if !words.next().is_some_and(|t| t.is("import")) {
                return Err(unreadable());
            }
            Some(module)
        } else {
            None
        };
        let mut items = vec![];
        loop {
            let name = if from.is_some() {
                words.next().and_then(word).map(str::to_owned)
            } else {
                dotted(&mut words).map(|(path, column)| {
                    module_columns.push(column);
                    path
                })
            }
            .ok_or_else(unreadable)?;
            let alias = if words.peek().is_some_and(|t| t.is("as")) {
                words.next();
                Some(
                    words
                        .next()
                        .and_then(word)
                        .ok_or_else(unreadable)?
                        .to_owned(),
                )
            } else {
                None
            };
            items.push((name, alias));
            match words.next() {
                Some(t) if t.is(",") => continue,
                None => break,
                Some(_) => return Err(unreadable()),
            }
        }
        // An import holds no strings, so `#` on its line starts a comment.
        let comment = lines
            .get(line as usize - 1)
            .is_some_and(|l| l.contains('#'));
        out.push(Import {
            line: line - 1,
            from,
            items,
            module_columns,
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
    /// A member written with spaces around the `.`, or across lines.
    Other,
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
    let mut before = start.min(chars.len());
    while before > 0 && chars[before - 1].is_whitespace() {
        before -= 1;
    }
    if before > 0 && chars[before - 1] == '.' {
        return Form::Other;
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
    /// The declaration's position in any file, `std` included.
    identity: Option<(File, u32, u32)>,
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
    /// The package targets and tests, with their graphs.
    entries: Vec<(PathBuf, crate::project::SourceGraph)>,
    /// Every source file, checked as its own entry when no target reaches
    /// it, so code no target imports is still read.
    files: Vec<(PathBuf, crate::project::SourceGraph)>,
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
        // The targets and tests of this package and of workspace members
        // that depend on it, and every source file of those packages.
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
        Ok(Planner {
            overlay,
            project,
            member,
            source_path,
            destination_path,
            source_module,
            destination_module,
            entries,
            files: all_files,
        })
    }

    fn text(&self, path: &Path) -> Result<String, String> {
        match self.overlay.get(path) {
            Some(text) => Ok(text.clone()),
            None => std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())),
        }
    }

    /// Analyze the targets and then each file they do not reach, with
    /// `overlay`; the references found and the files the targets reach.
    /// Each file is read by as few analyses as its imports allow.
    fn analyze(
        &self,
        overlay: &HashMap<PathBuf, String>,
    ) -> Result<(Vec<Use>, BTreeSet<PathBuf>), String> {
        let mut uses: BTreeMap<(PathBuf, u32, u32, u32), Use> = BTreeMap::new();
        let mut reached = BTreeSet::new();
        for (root, graph) in &self.entries {
            let analysis = self.analyze_one(root, graph, overlay, &mut uses)?;
            reached.extend(analysis);
        }
        let mut seen = reached.clone();
        for (root, graph) in &self.files {
            if seen.contains(root) {
                continue;
            }
            let analysis = self.analyze_one(root, graph, overlay, &mut uses)?;
            seen.extend(analysis);
        }
        Ok((uses.into_values().collect(), reached))
    }

    /// Analyze one entry, adding its references to `uses`; the files it read.
    fn analyze_one(
        &self,
        root: &Path,
        graph: &crate::project::SourceGraph,
        overlay: &HashMap<PathBuf, String>,
        uses: &mut BTreeMap<(PathBuf, u32, u32, u32), Use>,
    ) -> Result<Vec<PathBuf>, String> {
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
            let identity = symbol
                .location
                .as_ref()
                .map(|l| (l.file.clone(), l.line, l.column));
            let l = &reference.location;
            uses.entry((path.clone(), l.line, l.column, l.length))
                .or_insert_with(|| Use {
                    path: path.clone(),
                    location: l.clone(),
                    target,
                    identity,
                    kind: symbol.kind,
                });
        }
        Ok(analysis
            .sources
            .keys()
            .filter_map(|f| match f {
                File::Path(path) => Some(path.clone()),
                File::Standard(_) => None,
            })
            .collect())
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.project.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    fn plan(&self, line: u32) -> Result<Move, String> {
        let source_text = self.text(&self.source_path)?;
        let destination_text = self.text(&self.destination_path)?;
        let all_items = items(&source_text)?;
        let item = all_items
            .iter()
            .find(|i| i.contains(line))
            .ok_or("no top-level declaration here")?;
        let declaration = item.declaration.clone()?;
        let name = declaration.name.clone();
        // `include_str` and `include_bytes` paths are relative to the
        // including file's directory.
        if let Some(directory) = self.source_path.parent()
            && item.includes
            && self.destination_path.parent() != Some(directory)
        {
            return Err(format!(
                "`{name}` includes a file by a path relative to {}/; move it to a module in that directory, or change the path first",
                self.relative(directory)
            ));
        }
        let (uses, reached_before) = self
            .analyze(self.overlay)
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
        for path in [&self.source_path, &self.destination_path]
            .into_iter()
            .chain(uses.iter().map(|u| &u.path))
        {
            if !sources.contains_key(path) {
                let text = self.text(path)?;
                let imports = imports(&text);
                sources.insert(path.clone(), Source { text, imports });
            }
        }
        let destination_items = items(&destination_text)?;
        let destination_names = destination_items
            .iter()
            .filter_map(|i| i.declaration.as_ref().ok().map(|d| d.name.clone()))
            .collect();
        let mut work = Work {
            planner: self,
            name,
            declaration,
            target,
            span: (item.first - 1, item.last - 1),
            source_declarations: all_items
                .iter()
                .filter_map(|i| i.declaration.as_ref().ok())
                .map(|d| ((d.line, d.column), d.clone()))
                .collect(),
            uses,
            sources,
            destination_items,
            destination_names,
            edits: BTreeMap::new(),
            span_edits: vec![],
            api_changes: vec![],
            imports: BTreeMap::new(),
            dependencies: BTreeSet::new(),
            needs_public: false,
        };
        work.moved_references()?;
        work.carried_imports()?;
        work.callers()?;
        work.move_text()?;
        work.import_edits()?;
        self.finish(work, &reached_before)
    }

    /// Apply the plan's edits to the overlay and check the package again.
    fn finish(&self, work: Work, reached_before: &BTreeSet<PathBuf>) -> Result<Move, String> {
        let name = work.name;
        let mut overlay = self.overlay.clone();
        let mut files = vec![];
        for (path, mut file_edits) in work.edits {
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
            let new_text = apply(&work.sources[&path].text, &merged)?;
            overlay.insert(path.clone(), new_text);
            files.push(FileEdits {
                path,
                edits: merged,
            });
        }
        let (_, reached_after) = self
            .analyze(&overlay)
            .map_err(|e| format!("after moving `{name}`, the package would not check: {e}"))?;
        // A module left without declarations may stop being imported; any
        // other file must stay checked.
        let empty = |path: &Path| {
            overlay
                .get(path)
                .is_some_and(|text| items(text).is_ok_and(|items| items.iter().all(|i| i.import)))
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
            api_changes: work.api_changes,
        })
    }
}

/// One plan's state: the references found, the files read, and the edits
/// collected so far.
struct Work<'p, 'a> {
    planner: &'p Planner<'a>,
    name: String,
    declaration: Declaration,
    /// The moved declaration as references record it: (file, line, column).
    target: (PathBuf, u32, u32),
    /// The first and last line (from 0) of the moved text.
    span: (u32, u32),
    /// The source module's declarations by name position.
    source_declarations: HashMap<(u32, u32), Declaration>,
    uses: Vec<Use>,
    sources: HashMap<PathBuf, Source>,
    destination_items: Vec<Item>,
    destination_names: BTreeSet<String>,
    edits: BTreeMap<PathBuf, Vec<TextEdit>>,
    /// Edits to the moved text, with lines counted from its first line.
    span_edits: Vec<TextEdit>,
    api_changes: Vec<String>,
    imports: BTreeMap<PathBuf, ImportChanges>,
    /// Declarations of the source module the moved code uses.
    dependencies: BTreeSet<String>,
    /// Whether a module other than the destination uses the moved name.
    needs_public: bool,
}

fn is_declaration(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function
            | SymbolKind::Class
            | SymbolKind::Enum
            | SymbolKind::Protocol
            | SymbolKind::Constant
            | SymbolKind::TypeAlias
    )
}

impl Work<'_, '_> {
    fn source(&self) -> &Path {
        &self.planner.source_path
    }

    fn destination(&self) -> &Path {
        &self.planner.destination_path
    }

    fn in_span(&self, u: &Use) -> bool {
        u.path == self.planner.source_path
            && (self.span.0..=self.span.1).contains(&location_line(&u.location))
    }

    fn line_text(&self, path: &Path, line: u32) -> String {
        self.sources[path]
            .text
            .lines()
            .nth(line as usize)
            .unwrap_or_default()
            .to_owned()
    }

    fn form_of(&self, u: &Use) -> Form {
        form(
            &self.line_text(&u.path, location_line(&u.location)),
            &u.location,
        )
    }

    fn import_lines(&self, path: &Path) -> HashSet<u32> {
        match &self.sources[path].imports {
            Ok(imports) => imports.iter().map(|i| i.line).collect(),
            Err(_) => HashSet::default(),
        }
    }

    fn file_imports(&self, path: &Path) -> Result<Vec<Import>, String> {
        self.sources[path]
            .imports
            .clone()
            .map_err(|e| format!("{}: {e}", self.planner.relative(path)))
    }

    /// The module the `index`th module path of `import` names, as the
    /// analysis resolved it: the file, or the `std` module.
    fn module_of(&self, path: &Path, import: &Import, index: usize) -> Option<File> {
        let index = if import.from.is_some() { 0 } else { index };
        let column = *import.module_columns.get(index)?;
        self.uses
            .iter()
            .find(|u| {
                u.path == path
                    && location_line(&u.location) == import.line
                    && u.location.column == column
                    && u.kind == SymbolKind::Module
            })
            .and_then(|u| u.identity.as_ref().map(|i| i.0.clone()))
    }

    /// Whether the `index`th module of `import` in `path` is the file `module`.
    fn imports_file(&self, path: &Path, import: &Import, index: usize, module: &Path) -> bool {
        self.module_of(path, import, index) == Some(File::Path(module.to_owned()))
    }

    /// The reference a `from` import records for its item `item`.
    fn item_use(&self, path: &Path, import: &Import, item: &str) -> Option<&Use> {
        let text = self.line_text(path, import.line);
        self.uses.iter().find(|u| {
            u.path == path
                && location_line(&u.location) == import.line
                && u.kind != SymbolKind::Module
                && text
                    .chars()
                    .skip(u.location.column as usize - 1)
                    .take(u.location.length as usize)
                    .collect::<String>()
                    == item
        })
    }

    /// Whether `u` is written `binding.member` for a member of `module`.
    fn qualified_by(&self, u: &Use, binding: &str, module: Option<&File>) -> bool {
        let in_module = match (module, &u.identity) {
            (Some(module), Some((file, _, _))) => file == module,
            _ => true,
        };
        in_module
            && matches!(self.form_of(u), Form::Qualified { qualifier, .. } if qualifier == binding)
    }

    fn import_error(&self, path: &Path, import: &Import) -> Result<(), String> {
        if import.comment {
            return Err(format!(
                "{}:{}: an import with a comment cannot be rewritten",
                self.planner.relative(path),
                import.line + 1
            ));
        }
        Ok(())
    }

    /// Examine what the moved text refers to: declarations of the source
    /// module it must import, and the destination's own declarations.
    fn moved_references(&mut self) -> Result<(), String> {
        let name = self.name.clone();
        let span_uses: Vec<Use> = self
            .uses
            .iter()
            .filter(|u| self.in_span(u))
            .cloned()
            .collect();
        for u in &span_uses {
            let Some((file, line, declared_column)) = &u.target else {
                continue;
            };
            let declared_in_span =
                file == self.source() && (self.span.0 + 1..=self.span.1 + 1).contains(line);
            if declared_in_span || matches!(u.kind, SymbolKind::Local | SymbolKind::Parameter) {
                continue;
            }
            let written = self.form_of(u);
            if file == self.source() {
                if !is_declaration(u.kind) {
                    continue;
                }
                let Some(dependency) = self.source_declarations.get(&(*line, *declared_column))
                else {
                    return Err(format!(
                        "`{name}` uses a declaration of `{}` the move cannot import",
                        self.planner.source_module
                    ));
                };
                let dependency = dependency.clone();
                if !matches!(&written, Form::Bare { name, .. } if *name == dependency.name) {
                    return Err(format!(
                        "`{name}` refers to `{}` in a form the move cannot rewrite",
                        dependency.name
                    ));
                }
                if self.dependencies.insert(dependency.name.clone())
                    && let Some(at) = dependency.private_at
                {
                    self.edits
                        .entry(self.planner.source_path.clone())
                        .or_default()
                        .push(insert(at.0 - 1, at.1 - 1, "pub "));
                    self.api_changes.push(format!(
                        "makes `{}.{}` public, because the moved code uses it",
                        self.planner.source_module, dependency.name
                    ));
                }
                continue;
            }
            if file == self.destination() {
                // The destination's own declaration: write its bare name.
                let line_in_span = location_line(&u.location) - self.span.0;
                match written {
                    Form::Bare {
                        column,
                        name: written,
                    } => {
                        let real = destination_declaration_name(
                            &self.destination_items,
                            *line,
                            *declared_column,
                        )
                        .unwrap_or(written.clone());
                        if real != written {
                            self.span_edits.push(replace(
                                line_in_span,
                                column,
                                written.chars().count() as u32,
                                &real,
                            ));
                        }
                    }
                    Form::Qualified {
                        column, qualifier, ..
                    } => {
                        let length = qualifier.chars().count() as u32 + 1;
                        self.span_edits
                            .push(replace(line_in_span, column, length, ""));
                    }
                    Form::Other => {
                        return Err(format!(
                            "`{name}` refers to `{}.{}` in a form the move cannot rewrite",
                            self.planner.destination_module,
                            self.line_text(&u.path, location_line(&u.location))
                                .chars()
                                .skip(u.location.column as usize - 1)
                                .take(u.location.length as usize)
                                .collect::<String>()
                        ));
                    }
                }
                continue;
            }
            if let Form::Bare { name: written, .. } = written
                && self.destination_names.contains(&written)
            {
                return Err(format!(
                    "`{name}` uses `{written}`, which `{}` declares as something else",
                    self.planner.destination_module
                ));
            }
        }
        if self.destination_names.contains(&name) {
            return Err(format!(
                "`{}` already declares `{name}`",
                self.planner.destination_module
            ));
        }
        Ok(())
    }

    /// Copy the source's imports the moved text uses to the destination, and
    /// drop those nothing else in the source uses.
    fn carried_imports(&mut self) -> Result<(), String> {
        let name = self.name.clone();
        let source = self.source().to_owned();
        let destination = self.destination().to_owned();
        let source_imports = self.file_imports(&source)?;
        let destination_imports = self.file_imports(&destination)?;
        let target = Some(self.target.clone());
        let mut bindings: HashMap<String, String> = HashMap::default();
        for import in &destination_imports {
            for item in &import.items {
                let binding = Import::binding(item);
                if binding == name {
                    // The destination's own import of the moved name is
                    // removed with the other callers' imports.
                    let own = import.from.is_some()
                        && self
                            .item_use(&destination, import, &item.0)
                            .is_some_and(|u| u.target == target);
                    if !own {
                        return Err(format!(
                            "`{}` already imports a `{name}`",
                            self.planner.destination_module
                        ));
                    }
                }
                bindings.insert(binding.to_owned(), render_one(import, item));
            }
        }
        let source_lines = self.import_lines(&source);
        let outside = |u: &Use| -> bool {
            u.path == source
                && !self.in_span(u)
                && !source_lines.contains(&location_line(&u.location))
        };
        let mut additions = vec![];
        let mut removals = vec![];
        for import in &source_imports {
            for (index, item) in import.items.iter().enumerate() {
                let binding = Import::binding(item).to_owned();
                let module = self.module_of(&source, import, index);
                let (used_in_span, used_outside) = match &import.from {
                    None => {
                        let qualified = |u: &Use| self.qualified_by(u, &binding, module.as_ref());
                        (
                            self.uses.iter().any(|u| self.in_span(u) && qualified(u)),
                            self.uses.iter().any(|u| outside(u) && qualified(u)),
                        )
                    }
                    Some(_) => {
                        let Some(identity) = self
                            .item_use(&source, import, &item.0)
                            .and_then(|u| u.identity.clone())
                        else {
                            // A name the analysis did not record: keep it.
                            continue;
                        };
                        let bare = |u: &Use| {
                            u.identity.as_ref() == Some(&identity)
                                && matches!(self.form_of(u), Form::Bare { .. })
                        };
                        (
                            self.uses.iter().any(|u| self.in_span(u) && bare(u)),
                            self.uses.iter().any(|u| outside(u) && bare(u)),
                        )
                    }
                };
                if !used_in_span {
                    continue;
                }
                if !used_outside {
                    removals.push((import.line, binding.clone()));
                }
                if module == Some(File::Path(destination.clone())) {
                    continue;
                }
                let rendered = render_one(import, item);
                match bindings.get(&binding) {
                    Some(existing) if *existing == rendered => {}
                    Some(existing) => {
                        return Err(format!(
                            "`{name}` needs `{rendered}`, but `{}` already has `{existing}`",
                            self.planner.destination_module
                        ));
                    }
                    None if self.destination_names.contains(&binding) => {
                        return Err(format!(
                            "`{name}` needs `{rendered}`, but `{}` declares `{binding}`",
                            self.planner.destination_module
                        ));
                    }
                    None => {
                        bindings.insert(binding, rendered.clone());
                        additions.push(rendered);
                    }
                }
            }
        }
        for dependency in &self.dependencies {
            if bindings.contains_key(dependency) || self.destination_names.contains(dependency) {
                return Err(format!(
                    "`{name}` uses `{}.{dependency}`, but `{}` already binds `{dependency}`",
                    self.planner.source_module, self.planner.destination_module
                ));
            }
            additions.push(format!(
                "from {} import {dependency}",
                self.planner.source_module
            ));
        }
        self.imports.entry(source).or_default().0.extend(removals);
        self.imports
            .entry(destination)
            .or_default()
            .1
            .extend(additions);
        Ok(())
    }

    /// Rewrite the references to the moved declaration outside the moved
    /// text, and their imports.
    fn callers(&mut self) -> Result<(), String> {
        let name = self.name.clone();
        let source = self.source().to_owned();
        let destination = self.destination().to_owned();
        let source_module = self.planner.source_module.clone();
        let destination_module = self.planner.destination_module.clone();
        let target = Some(self.target.clone());
        let mut by_file: BTreeMap<PathBuf, Vec<Use>> = BTreeMap::new();
        for u in &self.uses {
            if u.target == target && !self.in_span(u) {
                by_file.entry(u.path.clone()).or_default().push(u.clone());
            }
        }
        let mut source_uses_name = false;
        let cannot_rewrite = |path: &Path, u: &Use| {
            format!(
                "{}:{}:{}: cannot rewrite this reference to `{name}`",
                self.planner.relative(path),
                u.location.line,
                u.location.column
            )
        };
        // The part of `module` before the source module's path, such as
        // `tools.` in `tools.shapes`.
        let prefix = |module: &str| -> Option<String> {
            let prefix = module.strip_suffix(source_module.as_str())?;
            (prefix.is_empty() || prefix.ends_with('.')).then(|| prefix.to_owned())
        };
        let mut file_edits: Vec<(PathBuf, TextEdit)> = vec![];
        let mut file_imports: Vec<(PathBuf, ImportChanges)> = vec![];
        for (path, file_uses) in &by_file {
            let lines = self.import_lines(path);
            if *path == source {
                if file_uses
                    .iter()
                    .any(|u| !lines.contains(&location_line(&u.location)))
                {
                    source_uses_name = true;
                    self.needs_public = true;
                }
                continue;
            }
            let path_imports = self.file_imports(path)?;
            let is_destination = *path == destination;
            if !is_destination {
                self.needs_public = true;
            }
            let (mut removals, mut additions): ImportChanges = Default::default();
            // `from source import name`: import it from the destination.
            for import in &path_imports {
                let Some(module) = &import.from else {
                    continue;
                };
                for item in &import.items {
                    let own = self
                        .item_use(path, import, &item.0)
                        .is_some_and(|u| u.target == target);
                    if !own {
                        continue;
                    }
                    let Some(prefix) =
                        prefix(module).filter(|_| self.imports_file(path, import, 0, &source))
                    else {
                        return Err(format!(
                            "{}:{}: cannot rewrite this import of `{name}`",
                            self.planner.relative(path),
                            import.line + 1
                        ));
                    };
                    self.import_error(path, import)?;
                    removals.push((import.line, Import::binding(item).to_owned()));
                    if !is_destination {
                        let alias = item
                            .1
                            .as_ref()
                            .map(|a| format!(" as {a}"))
                            .unwrap_or_default();
                        additions.push(format!(
                            "from {prefix}{destination_module} import {name}{alias}"
                        ));
                    }
                }
            }
            let mut qualifier_for_destination: Option<String> = None;
            for u in file_uses {
                let l = location_line(&u.location);
                if lines.contains(&l) {
                    continue;
                }
                match self.form_of(u) {
                    Form::Other => return Err(cannot_rewrite(path, u)),
                    Form::Bare {
                        column,
                        name: written,
                    } => {
                        if is_destination && written != name {
                            file_edits.push((
                                path.clone(),
                                replace(l, column, written.chars().count() as u32, &name),
                            ));
                        }
                    }
                    Form::Qualified {
                        column,
                        qualifier,
                        name: written,
                    } => {
                        // The `import` binding the qualifier, which must
                        // name the source module.
                        let binding = path_imports.iter().find_map(|i| {
                            if i.from.is_some() {
                                return None;
                            }
                            i.items.iter().enumerate().find_map(|(index, item)| {
                                (Import::binding(item) == qualifier
                                    && self.imports_file(path, i, index, &source))
                                .then_some((i, item))
                            })
                        });
                        let Some((import, item)) = binding else {
                            return Err(cannot_rewrite(path, u));
                        };
                        let Some(prefix) = prefix(&item.0).filter(|_| written == name) else {
                            return Err(cannot_rewrite(path, u));
                        };
                        let length =
                            qualifier.chars().count() as u32 + 1 + written.chars().count() as u32;
                        let replacement = if is_destination {
                            name.clone()
                        } else {
                            let qualifier = qualifier_for_destination.get_or_insert_with(|| {
                                let existing = path_imports.iter().find_map(|i| {
                                    if i.from.is_some() {
                                        return None;
                                    }
                                    i.items.iter().enumerate().find_map(|(index, item)| {
                                        self.imports_file(path, i, index, &destination)
                                            .then(|| Import::binding(item).to_owned())
                                    })
                                });
                                existing.unwrap_or_else(|| {
                                    let module = format!("{prefix}{destination_module}");
                                    additions.push(format!("import {module}"));
                                    module
                                })
                            });
                            format!("{qualifier}.{name}")
                        };
                        file_edits.push((path.clone(), replace(l, column, length, &replacement)));
                        // Remove `import source` when this was its last use.
                        let other_uses = self.uses.iter().any(|o| {
                            o.path == *path
                                && o.target != target
                                && !lines.contains(&location_line(&o.location))
                                && self.qualified_by(o, &qualifier, None)
                        });
                        if !other_uses && !removals.iter().any(|(_, b)| *b == qualifier) {
                            self.import_error(path, import)?;
                            removals.push((import.line, qualifier.clone()));
                        }
                    }
                }
            }
            file_imports.push((path.clone(), (removals, additions)));
        }
        for (path, edit) in file_edits {
            self.edits.entry(path).or_default().push(edit);
        }
        for (path, (removals, additions)) in file_imports {
            let entry = self.imports.entry(path).or_default();
            entry.0.extend(removals);
            entry.1.extend(additions);
        }
        if source_uses_name {
            if !self.dependencies.is_empty() {
                return Err(format!(
                    "moving `{name}` would make `{source_module}` and `{destination_module}` import each other: `{name}` uses {} and the rest of `{source_module}` uses `{name}`",
                    self.dependencies
                        .iter()
                        .map(|d| format!("`{d}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ));
            }
            self.imports
                .entry(source)
                .or_default()
                .1
                .push(format!("from {destination_module} import {name}"));
        }
        Ok(())
    }

    /// Delete the moved text from the source and append it to the destination.
    fn move_text(&mut self) -> Result<(), String> {
        let name = &self.name;
        let (first, last) = self.span;
        let source_text = &self.sources[self.source()].text;
        let destination_text = &self.sources[self.destination()].text;
        let newline = newline(destination_text);
        let source_lines: Vec<&str> = source_text.lines().collect();
        let mut moved: String = source_lines[first as usize..=last as usize].join(newline);
        moved.push_str(newline);
        if let Some(at) = self.declaration.private_at
            && self.needs_public
        {
            self.span_edits
                .push(insert(at.0 - 1 - first, at.1 - 1, "pub "));
            self.api_changes.push(format!(
                "makes `{}.{name}` public, because other modules use it",
                self.planner.destination_module
            ));
        }
        let member = &self.planner.project.packages[self.planner.member];
        if self.declaration.private_at.is_none() && member.manifest.library.is_some() {
            let package = &member.manifest.name;
            self.api_changes.push(format!(
                "moves public `{package}.{}.{name}` to `{package}.{}.{name}`; packages outside this workspace that import it must change",
                self.planner.source_module, self.planner.destination_module
            ));
        }
        let moved = apply(&moved, &self.span_edits)?;

        // Remove the declaration and the blank lines after it.
        let mut end = last + 1;
        while (end as usize) < source_lines.len() && source_lines[end as usize].trim().is_empty() {
            end += 1;
        }
        let mut start = first;
        if end as usize >= source_lines.len() {
            while start > 0 && source_lines[start as usize - 1].trim().is_empty() {
                start -= 1;
            }
        }
        let removal = TextEdit {
            line: start,
            column: 0,
            end_line: end,
            end_column: 0,
            text: String::new(),
        };
        let destination_lines: Vec<&str> = destination_text.lines().collect();
        let last_line = destination_lines.len() as u32;
        let ends_with_newline = destination_text.is_empty() || destination_text.ends_with('\n');
        let mut appended = String::new();
        if !ends_with_newline {
            appended.push_str(newline);
        }
        if destination_lines.iter().any(|l| !l.trim().is_empty()) {
            appended.push_str(newline);
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
        let source = self.source().to_owned();
        let destination = self.destination().to_owned();
        self.edits.entry(source).or_default().push(removal);
        self.edits.entry(destination).or_default().push(insert(
            append_line,
            append_column,
            &appended,
        ));
        Ok(())
    }

    /// Edit the import statements: remove bindings, then add the new
    /// imports after the last one, combining `from` imports of one module.
    fn import_edits(&mut self) -> Result<(), String> {
        let changes = std::mem::take(&mut self.imports);
        for (path, (removals, additions)) in &changes {
            let path_imports = self.file_imports(path)?;
            let text = &self.sources[path].text;
            let newline = newline(text);
            let file_lines: Vec<&str> = text.lines().collect();
            let mut file_edits = vec![];
            let mut changed: BTreeMap<u32, Import> = BTreeMap::new();
            for (line, binding) in removals {
                let import = path_imports
                    .iter()
                    .find(|i| i.line == *line)
                    .expect("a removal names an import line");
                self.import_error(path, import)?;
                changed
                    .entry(*line)
                    .or_insert_with(|| import.clone())
                    .items
                    .retain(|item| Import::binding(item) != binding);
            }
            let existing: HashSet<String> = path_imports
                .iter()
                .flat_map(|i| i.items.iter().map(|item| render_one(i, item)))
                .collect();
            let mut added: Vec<Import> = vec![];
            for addition in additions {
                if existing.contains(addition) {
                    continue;
                }
                let import = imports(addition)
                    .ok()
                    .and_then(|mut i| i.pop())
                    .expect("the move writes readable imports");
                let same = added
                    .iter_mut()
                    .find(|a| a.from.is_some() && a.from == import.from);
                match same {
                    Some(a) => {
                        for item in import.items {
                            if !a.items.contains(&item) {
                                a.items.push(item);
                            }
                        }
                    }
                    None => {
                        if !added.iter().any(|a| a.render() == import.render()) {
                            added.push(import);
                        }
                    }
                }
            }
            // Removing every import also removes the blank lines after them.
            let all_removed = added.is_empty()
                && path_imports
                    .iter()
                    .all(|i| changed.get(&i.line).is_some_and(|c| c.items.is_empty()));
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
                    let length = self.line_text(path, *line).chars().count() as u32;
                    file_edits.push(replace(*line, 0, length, &import.render()));
                }
            }
            if !added.is_empty() {
                let mut text: String = added.iter().map(|i| i.render() + newline).collect();
                let at = match path_imports.last() {
                    Some(last) => last.line + 1,
                    None => {
                        if file_lines.first().is_some_and(|l| !l.trim().is_empty()) {
                            text.push_str(newline);
                        }
                        0
                    }
                };
                file_edits.push(insert(at, 0, &text));
            }
            self.edits
                .entry(path.clone())
                .or_default()
                .extend(file_edits);
        }
        Ok(())
    }
}

/// The line ending `text` uses.
fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
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
        module_columns: vec![],
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
