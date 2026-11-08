//! Move a top-level declaration to another module of its package (issue
//! #822). A plan is computed from editor analyses of every file that could
//! refer to the declaration and returned as text edits; nothing is written.
//! Every unsupported case is refused before any edit is returned, and the
//! edited files are checked again before the plan is accepted.
use super::super::lexer::{Kind, Lexer, Token};
use super::index::{Errors, Package, Reach, Use, one_per_place, source_files};
use super::{File, HashMap, HashSet, Location, SymbolKind};
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
    keyword: String,
}

/// A file's top-level items and its multi-line strings.
struct Lexed {
    /// The tokens of each top-level item, without layout tokens.
    items: Vec<Vec<Token>>,
    /// The first and last line (from 1) of each string token that spans
    /// lines.
    strings: Vec<(u32, u32)>,
    /// Every token, with layout tokens: an `if const` block's extent.
    all: Vec<Token>,
}

impl Lexed {
    /// Whether `line` (from 1) starts inside a string, so a `#` there is text.
    fn inside_string(&self, line: u32) -> bool {
        self.strings.iter().any(|&(a, b)| a < line && line <= b)
    }

    /// Whether the line break ending `line` (from 1) belongs to a string.
    fn breaks_string(&self, line: u32) -> bool {
        self.strings.iter().any(|&(a, b)| a <= line && line < b)
    }
}

fn lex(text: &str) -> Result<Lexed, String> {
    let mut lexer = Lexer::new(text);
    let mut items: Vec<Vec<Token>> = vec![];
    let mut strings = vec![];
    // A token has no end position; the token after a string sits on the
    // string's last line.
    let mut open_string: Option<u32> = None;
    let mut all = vec![];
    loop {
        let mut tokens = VecDeque::new();
        lexer
            .item(&mut tokens)
            .map_err(|e| format!("cannot read the file: {e}"))?;
        let eof = tokens.iter().any(|t| t.kind == Kind::Eof);
        for t in &tokens {
            if let Some(line) = open_string.take()
                && t.line > line
            {
                strings.push((line, t.line));
            }
            if matches!(t.kind, Kind::Text(_) | Kind::Format(_) | Kind::Bytes(_)) {
                open_string = Some(t.line);
            }
        }
        all.extend(tokens.iter().cloned());
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
            items.push(tokens);
        }
        if eof {
            break;
        }
    }
    Ok(Lexed {
        items,
        strings,
        all,
    })
}

/// The lines (from 0) of each `if const` conditional in `text`, from its
/// `if const` line to the last line of its last arm. The analysis does not
/// see unselected arms, so names there are neither found nor checked. Each
/// arm is an indented suite (the parser rejects one-line arms), which ends
/// at its matching dedent; the lexer emits no layout tokens inside
/// brackets.
fn conditionals(text: &str) -> Result<Vec<(u32, u32)>, String> {
    let lexed = lex(text)?;
    let tokens = &lexed.all;
    let layout = |t: &Token| {
        matches!(
            t.kind,
            Kind::Newline | Kind::Indent | Kind::Dedent | Kind::Eof
        )
    };
    // The last line a token covers, counting a string's lines. When
    // several strings start on the token's line this takes the first one
    // spanning lines, which may not be the token itself; that string is on
    // the same line of the same arm, so the extent stays inside the block.
    let end = |t: &Token| {
        lexed
            .strings
            .iter()
            .find(|&&(a, _)| a == t.line)
            .map_or(t.line, |&(_, b)| b)
    };
    let mut out = vec![];
    for start in 0..tokens.len() {
        if !(tokens[start].is("if") && tokens.get(start + 1).is_some_and(|t| t.is("const"))) {
            continue;
        }
        let mut last = tokens[start].line;
        let mut i = start;
        loop {
            // The header's `:` outside brackets.
            let mut depth = 0i32;
            while i < tokens.len() {
                let t = &tokens[i];
                if t.is("(") || t.is("[") || t.is("{") {
                    depth += 1;
                } else if t.is(")") || t.is("]") || t.is("}") {
                    depth -= 1;
                } else if depth == 0 && t.is(":") {
                    break;
                }
                i += 1;
            }
            // The arm: a newline, then an indented suite to its dedent.
            i += 1;
            if !tokens.get(i).is_some_and(|t| t.kind == Kind::Newline)
                || !tokens.get(i + 1).is_some_and(|t| t.kind == Kind::Indent)
            {
                return Err(format!(
                    "line {}: an `if const` arm must start on its own line",
                    tokens[start].line
                ));
            }
            let mut indents = 0i32;
            while i < tokens.len() {
                let t = &tokens[i];
                match t.kind {
                    Kind::Indent => indents += 1,
                    Kind::Dedent => {
                        indents -= 1;
                        if indents == 0 {
                            i += 1;
                            break;
                        }
                    }
                    Kind::Eof => break,
                    _ => {}
                }
                if !layout(t) {
                    last = last.max(end(t));
                }
                i += 1;
            }
            let next = tokens[i.min(tokens.len() - 1)..]
                .iter()
                .find(|t| t.kind != Kind::Newline);
            if next.is_some_and(|t| t.is("elif") || t.is("else")) {
                continue;
            }
            break;
        }
        out.push((tokens[start].line - 1, last - 1));
    }
    Ok(out)
}

/// Whether `text` contains `name` as a whole word.
fn mentions(text: &str, name: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(name).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(is_word)
            && !text[at + name.len()..].chars().next().is_some_and(is_word)
    })
}

/// The tokens of each top-level item of `text`, without layout tokens.
fn top_level(text: &str) -> Result<Vec<Vec<Token>>, String> {
    lex(text).map(|l| l.items)
}

fn is_import(tokens: &[Token]) -> bool {
    tokens
        .first()
        .is_some_and(|t| t.column == 1 && (t.is("import") || t.is("from")))
}

/// The top-level items of `text`, with each item's attached comments and
/// decorators.
fn items(text: &str) -> Result<Vec<Item>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let lexed = lex(text)?;
    let starts = &lexed.items;
    let blank = |line: u32| {
        lines
            .get(line as usize - 1)
            .is_none_or(|l| l.trim().is_empty())
    };
    let margin_comment = |line: u32| {
        !lexed.inside_string(line)
            && lines
                .get(line as usize - 1)
                .is_some_and(|l| l.starts_with('#'))
    };
    let mut out = vec![];
    // The first line and include flag of the decorators before an item.
    let mut decorators: Option<(u32, bool)> = None;
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
        let includes = tokens
            .iter()
            .any(|t| t.is("include_str") || t.is("include_bytes"));
        // A decorator is its own item; it and its comments move with the
        // declaration after it.
        if tokens[0].is("@") && starts.get(i + 1).is_some() {
            let (earlier, earlier_includes) = decorators.unwrap_or((first, false));
            decorators = Some((earlier, earlier_includes || includes));
            continue;
        }
        let (first, includes) = match decorators.take() {
            Some((earlier, earlier_includes)) => (earlier, earlier_includes || includes),
            None => (first, includes),
        };
        out.push(Item {
            first,
            last,
            declaration: declaration(tokens),
            includes,
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
        keyword: keyword_text.to_owned(),
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

/// The file's import statements at the left margin. Imports inside a
/// top-level `if const` block are left out: the move never rewrites them,
/// and [`arm_imports`] reads them for the checks that cover them.
fn imports(text: &str) -> Result<Vec<Import>, String> {
    let lines: Vec<&str> = text.lines().collect();
    top_level(text)?
        .iter()
        .filter(|tokens| is_import(tokens))
        .map(|tokens| import_statement(tokens, &lines))
        .collect()
}

/// The imports inside the file's `if const` blocks, the only place an
/// indented import can appear. One the move cannot read is skipped: the
/// parser accepts only one-line imports, which it can read.
fn arm_imports(text: &str) -> Result<Vec<Import>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    for tokens in top_level(text)? {
        if is_import(&tokens) {
            continue;
        }
        for statement in tokens.chunk_by(|a, b| a.line == b.line) {
            if statement[0].is("import") || statement[0].is("from") {
                out.extend(import_statement(statement, &lines).ok());
            }
        }
    }
    Ok(out)
}

/// The import statement `tokens`, all on one line.
fn import_statement(tokens: &[Token], lines: &[&str]) -> Result<Import, String> {
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
    Ok(Import {
        line: line - 1,
        from,
        items,
        module_columns,
        comment,
    })
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

/// A file the move reads: its text and how its module is imported.
struct Source {
    text: String,
    imports: Result<Vec<Import>, String>,
}

struct Planner<'a> {
    overlay: &'a HashMap<PathBuf, String>,
    package: Package,
    source_path: PathBuf,
    destination_path: PathBuf,
    source_module: String,
    destination_module: String,
    /// Whether the destination is the package's library, which code outside
    /// `src/` imports under the package name.
    destination_is_library: bool,
}

fn package_src(path: &Path) -> Option<PathBuf> {
    let directory = crate::project::discover(path.parent()?).ok()?;
    let manifest = directory.join("src");
    let src = manifest.canonicalize().ok()?;
    path.starts_with(&src).then_some(src)
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
        source_path
            .parent()
            .and_then(|p| crate::project::discover(p).ok())
            .ok_or("moving a declaration needs a package: this file has no plenty.toml")?;
        let index = Package::open(&source_path)?;
        let package = &index.project.packages[index.member];
        let in_src = |src: &PathBuf| source_path.starts_with(src);
        if !package
            .directory
            .join("src")
            .canonicalize()
            .is_ok_and(|src| in_src(&src))
        {
            return Err("only files below a package's src/ can move declarations".into());
        }
        let src = package
            .directory
            .join("src")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !destination_path.starts_with(&src) || !crate::source_path::is_source(&destination_path)
        {
            return Err("the destination must be a module of the same package's src/".into());
        }
        if destination_path == source_path {
            return Err("the destination is the declaration's own module".into());
        }
        let library = package
            .manifest
            .library
            .as_ref()
            .and_then(|l| package.directory.join(l).canonicalize().ok());
        let source_module = module_path(&src, &source_path).ok_or("source module path")?;
        if library.as_ref() == Some(&source_path) {
            return Err(format!(
                "`{source_module}` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported"
            ));
        }
        let destination_is_library = library.as_ref() == Some(&destination_path);
        let destination_module =
            module_path(&src, &destination_path).ok_or("destination module path")?;
        Ok(Planner {
            overlay,
            package: index,
            source_path,
            destination_path,
            source_module,
            destination_module,
            destination_is_library,
        })
    }

    /// The package modules `path` imports, with `text` as its text, each
    /// with whether the import is inside an `if const` block. Imports of
    /// another package cannot close a cycle and are left out.
    fn module_imports(&self, path: &Path, text: &str) -> Result<Vec<(PathBuf, bool)>, String> {
        let Some(src) = self
            .sources_dirs()
            .into_iter()
            .find(|s| path.starts_with(s))
        else {
            return Ok(vec![]);
        };
        let file = |module: &str| {
            let base = src.join(module.replace('.', "/"));
            crate::source_path::EXTENSIONS
                .iter()
                .find_map(|e| base.with_extension(e).canonicalize().ok())
        };
        let mut out = vec![];
        let arms = arm_imports(text)?;
        for (import, arm) in imports(text)?
            .iter()
            .map(|i| (i, false))
            .chain(arms.iter().map(|i| (i, true)))
        {
            let modules: Vec<String> = match &import.from {
                Some(module) => std::iter::once(module.clone())
                    .chain(import.items.iter().map(|i| format!("{module}.{}", i.0)))
                    .collect(),
                None => import.items.iter().map(|i| i.0.clone()).collect(),
            };
            out.extend(modules.iter().filter_map(|m| file(m)).map(|f| (f, arm)));
        }
        Ok(out)
    }

    /// Each workspace member's `src/` directory.
    fn sources_dirs(&self) -> Vec<PathBuf> {
        self.package
            .project
            .members
            .iter()
            .filter_map(|&m| {
                self.package.project.packages[m]
                    .directory
                    .join("src")
                    .canonicalize()
                    .ok()
            })
            .collect()
    }

    /// Refuse when an import the move adds closes a circular import through
    /// an import in an `if const` block. The check after the move sees only
    /// the selected arms, so it catches every other cycle.
    fn arm_cycle(
        &self,
        name: &str,
        after: &HashMap<PathBuf, String>,
        edited: &BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        let mut graph: BTreeMap<PathBuf, Vec<(PathBuf, bool)>> = BTreeMap::new();
        let mut any_arm = false;
        // The package's `.plenty` files, then each module they import that
        // is not one of them, such as a `.plentyi` module.
        let mut pending: Vec<PathBuf> = self.package.files.iter().map(|f| f.0.clone()).collect();
        while let Some(path) = pending.pop() {
            if graph.contains_key(&path) {
                continue;
            }
            let text = match after.get(&path) {
                Some(text) => text.clone(),
                None => self.text(&path)?,
            };
            let edges = self.module_imports(&path, &text)?;
            any_arm |= edges.iter().any(|e| e.1);
            pending.extend(edges.iter().map(|e| e.0.clone()));
            graph.insert(path, edges);
        }
        if !any_arm {
            return Ok(());
        }
        let module = |path: &Path| {
            self.sources_dirs()
                .iter()
                .find_map(|src| module_path(src, path))
                .unwrap_or_else(|| self.package.relative(path))
        };
        for from in edited {
            // An edge is new unless it was a top-level import before: one
            // that only an arm had now holds on every target.
            let before: HashSet<PathBuf> = self
                .module_imports(from, &self.text(from)?)?
                .into_iter()
                .filter(|e| !e.1)
                .map(|e| e.0)
                .collect();
            for (to, arm) in graph.get(from).into_iter().flatten() {
                if *arm || before.contains(to) {
                    continue;
                }
                // A path back from `to` to `from`, breadth first.
                let mut previous: HashMap<&Path, &Path> = HashMap::default();
                let mut queue = std::collections::VecDeque::from([to.as_path()]);
                while let Some(at) = queue.pop_front() {
                    if at == from.as_path() {
                        let mut cycle = vec![module(from)];
                        let mut step = at;
                        let mut back = vec![];
                        while step != to.as_path() {
                            step = previous[step];
                            back.push(module(step));
                        }
                        back.reverse();
                        cycle.extend(back);
                        cycle.push(module(from));
                        return Err(format!(
                            "after moving `{name}`, an import in an `if const` block would close a circular import: {}",
                            cycle.join(" -> ")
                        ));
                    }
                    for (next, _) in graph.get(at).into_iter().flatten() {
                        if next.as_path() != to.as_path() && !previous.contains_key(next.as_path())
                        {
                            previous.insert(next, at);
                            queue.push_back(next);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The destination's import path, after a dependency or test prefix
    /// such as `tools.`: the library is imported as `tools` itself.
    fn destination_import(&self, prefix: &str) -> String {
        match prefix.strip_suffix('.') {
            Some(package) if self.destination_is_library => package.to_owned(),
            _ => format!("{prefix}{}", self.destination_module),
        }
    }

    fn text(&self, path: &Path) -> Result<String, String> {
        match self.overlay.get(path) {
            Some(text) => Ok(text.clone()),
            None => std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())),
        }
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
        // A target's entry file runs its own `main`.
        if name == "main"
            && self
                .package
                .entries
                .iter()
                .any(|(path, _)| *path == self.source_path)
        {
            return Err(format!(
                "{} runs `main`; a target's `main` cannot move",
                self.package.relative(&self.source_path)
            ));
        }
        // `include_str` and `include_bytes` paths are relative to the
        // including file's directory.
        if let Some(directory) = self.source_path.parent()
            && item.includes
            && self.destination_path.parent() != Some(directory)
        {
            return Err(format!(
                "`{name}` includes a file by a path relative to {}/; move it to a module in that directory, or change the path first",
                self.package.relative(directory)
            ));
        }
        let (uses, reach_before) = self
            .package
            .analyze(self.overlay, None, Errors::Stop)
            .map_err(|e| format!("fix this error before moving `{name}`: {e}"))?;
        let uses = one_per_place(uses);
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
            rewrites: HashMap::default(),
            span_rewrites: HashMap::default(),
            source_qualifier: None,
            api_changes: vec![],
            imports: BTreeMap::new(),
            dependencies: BTreeSet::new(),
            needs_public: false,
        };
        work.conditional_in_moved_text()?;
        work.moved_references()?;
        work.carried_imports()?;
        work.callers()?;
        work.move_text()?;
        work.conditional_arms(&reach_before)?;
        work.import_edits()?;
        self.finish(work, &reach_before)
    }

    /// Apply the plan's edits to the overlay and check the package again.
    fn finish(&self, mut work: Work, reach_before: &Reach) -> Result<Move, String> {
        let name = work.name.clone();
        let mut overlay = self.overlay.clone();
        let mut files = vec![];
        for (path, mut file_edits) in std::mem::take(&mut work.edits) {
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
        let edited: BTreeSet<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
        let (uses_after, reach_after) = self
            .package
            .analyze(&overlay, Some((reach_before, &edited)), Errors::Stop)
            .map_err(|e| format!("after moving `{name}`, the package would not check: {e}"))?;
        let uses_after = one_per_place(uses_after);
        work.check_identities(&files, &overlay, &uses_after)?;
        self.arm_cycle(&name, &overlay, &edited)?;
        // A module left without declarations may stop being imported; any
        // other file must stay checked.
        let empty = |path: &Path| {
            overlay
                .get(path)
                .is_some_and(|text| items(text).is_ok_and(|items| items.iter().all(|i| i.import)))
        };
        let reached_after = reach_after.reached();
        if let Some(lost) = reach_before
            .reached()
            .difference(&reached_after)
            .find(|path| !empty(path))
        {
            return Err(format!(
                "after moving `{name}`, no target would import {}",
                self.package.relative(lost)
            ));
        }
        Ok(Move {
            name,
            source: self.source_module.clone(),
            destination: self.destination_module.clone(),
            files,
            api_changes: std::mem::take(&mut work.api_changes),
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
    /// Qualified references to the moved declaration that are rewritten,
    /// by (file, line, column) of their location: the edit's start (from
    /// 0) and the name's offset in its replacement.
    rewrites: HashMap<(PathBuf, u32, u32), (u32, u32, u32)>,
    /// The same for references in the moved text, with the edit's start in
    /// the moved text.
    span_rewrites: HashMap<(PathBuf, u32, u32), (u32, u32, u32)>,
    /// The destination's binding of a plain `import` of the source, which
    /// qualifies the source declarations the moved text uses.
    source_qualifier: Option<String>,
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
            .map_err(|e| format!("{}: {e}", self.planner.package.relative(path)))
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

    /// The binding of a plain `import` of the file `module` in `path`.
    fn plain_binding(&self, path: &Path, module: &Path) -> Result<Option<String>, String> {
        Ok(self.file_imports(path)?.iter().find_map(|i| {
            if i.from.is_some() {
                return None;
            }
            i.items.iter().enumerate().find_map(|(index, item)| {
                self.imports_file(path, i, index, module)
                    .then(|| Import::binding(item).to_owned())
            })
        }))
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

    /// Refuse when the moved text holds an `if const` conditional, whose
    /// unselected arms the analysis cannot see.
    fn conditional_in_moved_text(&self) -> Result<(), String> {
        let name = &self.name;
        let source_text = &self.sources[self.source()].text;
        if let Some((first, _)) = conditionals(source_text)?
            .into_iter()
            .find(|&(first, last)| first <= self.span.1 && self.span.0 <= last)
        {
            return Err(format!(
                "{}:{}: `{name}` has an `if const` block, whose unselected arms the move cannot check",
                self.planner.package.relative(self.source()),
                first + 1
            ));
        }
        Ok(())
    }

    /// Refuse when an `if const` conditional could hold a use the analysis
    /// cannot see: one whose text, in any arm, mentions the moved name, a
    /// binding of the source module, or a binding of an import line the
    /// move adds, removes or rewrites in that file. Runs once the import
    /// changes are known.
    fn conditional_arms(&self, reach: &Reach) -> Result<(), String> {
        let name = &self.name;
        let source_leaf = self
            .planner
            .source_module
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_owned();
        let mut files: BTreeSet<PathBuf> = reach.reached();
        files.extend(reach.files.values().flatten().cloned());
        files.insert(self.source().to_owned());
        files.insert(self.destination().to_owned());
        files.extend(self.imports.keys().cloned());
        for path in &files {
            let text = match self.sources.get(path) {
                Some(source) => source.text.clone(),
                None => self.planner.text(path)?,
            };
            let arms = conditionals(&text)?;
            if arms.is_empty() {
                continue;
            }
            let mut names = vec![name.clone()];
            if path != self.source() {
                names.push(source_leaf.clone());
                if self.sources.contains_key(path) {
                    names.extend(self.plain_binding(path, self.source())?);
                    for import in self.file_imports(path)? {
                        names.extend(
                            import
                                .items
                                .iter()
                                .filter(|item| import.from.is_some() && item.0 == *name)
                                .map(|item| Import::binding(item).to_owned()),
                        );
                    }
                }
            }
            if let Some((removals, additions)) = self.imports.get(path) {
                names.extend(removals.iter().map(|(_, binding)| binding.clone()));
                for addition in additions {
                    for import in imports(addition)? {
                        names.extend(import.items.iter().map(|i| Import::binding(i).to_owned()));
                    }
                }
            }
            let lines: Vec<&str> = text.lines().collect();
            for (first, last) in arms {
                let arm = lines[first as usize..=last as usize].join("\n");
                if let Some(found) = names.iter().find(|n| mentions(&arm, n)) {
                    return Err(format!(
                        "{}:{}: an `if const` block names `{found}`; the move cannot check its unselected arms",
                        self.planner.package.relative(path),
                        first + 1
                    ));
                }
            }
        }
        Ok(())
    }

    fn import_error(&self, path: &Path, import: &Import) -> Result<(), String> {
        if import.comment {
            return Err(format!(
                "{}:{}: an import with a comment cannot be rewritten",
                self.planner.package.relative(path),
                import.line + 1
            ));
        }
        Ok(())
    }

    /// Check that the edited package names what it named before: each
    /// reference to the moved declaration resolves to it at its new
    /// position, and each reference in the moved text keeps its target. A
    /// rewritten name can otherwise be captured, such as `geometry.SCALE`
    /// written `SCALE` inside a function with a parameter `SCALE`.
    fn check_identities(
        &self,
        files: &[FileEdits],
        overlay: &HashMap<PathBuf, String>,
        after: &[Use],
    ) -> Result<(), String> {
        let name = &self.name;
        let edits: HashMap<&Path, &[TextEdit]> = files
            .iter()
            .map(|f| (f.path.as_path(), f.edits.as_slice()))
            .collect();
        let destination_text = overlay
            .get(self.destination())
            .ok_or("internal error: the destination was not edited")?;
        let moved = items(destination_text)?
            .into_iter()
            .filter_map(|i| i.declaration.ok())
            .find(|d| d.name == *name)
            .ok_or("internal error: the moved declaration is missing")?;
        // The moved text's first line (from 0) in the edited destination.
        let moved_first = (moved.line - 1) - (self.declaration.line - 1 - self.span.0);
        // Where a point (from 0) of the text before the move is afterwards.
        let map = |path: &Path, line: u32, column: u32| -> Option<(PathBuf, u32, u32)> {
            if path == self.source() && (self.span.0..=self.span.1).contains(&line) {
                let (l, c) = map_point(&self.span_edits, line - self.span.0, column)?;
                return Some((self.destination().to_owned(), moved_first + l, c));
            }
            let (l, c) = match edits.get(path) {
                Some(edits) => map_point(edits, line, column)?,
                None => (line, column),
            };
            Some((path.to_owned(), l, c))
        };
        let identity = |identity: &Option<(File, u32, u32)>| match identity {
            Some((File::Path(path), line, column)) => map(path, line - 1, column - 1)
                .map(|(path, line, column)| Some((File::Path(path), line + 1, column + 1))),
            other => Some(other.clone()),
        };
        let mut by_line: HashMap<(&Path, u32), Vec<&Use>> = HashMap::default();
        for u in after {
            by_line
                .entry((u.path.as_path(), location_line(&u.location)))
                .or_default()
                .push(u);
        }
        // Whether every reference covering the point names `expected`, and
        // one does.
        let names = |(path, line, column): (PathBuf, u32, u32), expected| {
            let covering: Vec<&&Use> = by_line
                .get(&(path.as_path(), line))
                .into_iter()
                .flatten()
                .filter(|u| {
                    (u.location.column - 1..u.location.column - 1 + u.location.length)
                        .contains(&column)
                })
                .collect();
            !covering.is_empty() && covering.iter().all(|u| u.identity == expected)
        };
        let target = Some(self.target.clone());
        let moved_identity = Some((
            File::Path(self.destination().to_owned()),
            moved.line,
            moved.column,
        ));
        let at = |u: &Use, column: u32| {
            format!(
                "{}:{}:{}",
                self.planner.package.relative(&u.path),
                u.location.line,
                column + 1
            )
        };
        for u in &self.uses {
            let line = location_line(&u.location);
            let in_span = self.in_span(u);
            if !in_span && (u.target != target || self.import_lines(&u.path).contains(&line)) {
                continue;
            }
            // Qualifiers are checked by the imports the move writes.
            if in_span && u.kind == SymbolKind::Module {
                continue;
            }
            let form = self.form_of(u);
            let (start, column, written) = match &form {
                Form::Qualified {
                    column,
                    qualifier,
                    name,
                } => (
                    *column,
                    column + qualifier.chars().count() as u32 + 1,
                    format!("{qualifier}.{name}"),
                ),
                Form::Bare { column, name } => (*column, *column, name.clone()),
                Form::Other => (u.location.column - 1, u.location.column - 1, name.clone()),
            };
            let key = (u.path.clone(), u.location.line, u.location.column);
            let point = if let Some(&(l, c, offset)) = self.span_rewrites.get(&key) {
                map_point(&self.span_edits, l, c)
                    .map(|(l, c)| (self.destination().to_owned(), moved_first + l, c + offset))
            } else if let Some(&(l, c, offset)) = self.rewrites.get(&key) {
                edits
                    .get(u.path.as_path())
                    .and_then(|e| map_point(e, l, c))
                    .map(|(l, c)| (u.path.clone(), l, c + offset))
            } else {
                map(&u.path, line, column)
            };
            let expected = if in_span {
                identity(&u.identity)
            } else {
                Some(moved_identity.clone())
            };
            let (Some(point), Some(expected)) = (point, expected) else {
                return Err(format!(
                    "{}: the move cannot check this reference",
                    at(u, start)
                ));
            };
            if !names(point, expected) {
                return Err(if in_span {
                    format!(
                        "{}: after the move, `{written}` in `{name}` would name something else",
                        at(u, start)
                    )
                } else {
                    format!(
                        "{}: after the move, `{written}` would no longer name `{name}`",
                        at(u, start)
                    )
                });
            }
        }
        Ok(())
    }

    /// Examine what the moved text refers to: declarations of the source
    /// module it must import, and the destination's own declarations.
    fn moved_references(&mut self) -> Result<(), String> {
        let name = self.name.clone();
        self.source_qualifier = self.plain_binding(self.destination(), self.source())?;
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
                let column = match &written {
                    Form::Bare { column, name } if *name == dependency.name => *column,
                    _ => {
                        return Err(format!(
                            "`{name}` refers to `{}` in a form the move cannot rewrite",
                            dependency.name
                        ));
                    }
                };
                // With `import source` already in the destination, write
                // `source.name`, as moving it in the other direction reads it.
                if let Some(qualifier) = &self.source_qualifier {
                    let line_in_span = location_line(&u.location) - self.span.0;
                    self.span_rewrites.insert(
                        (u.path.clone(), u.location.line, u.location.column),
                        (line_in_span, column, qualifier.chars().count() as u32 + 1),
                    );
                    self.span_edits.push(replace(
                        line_in_span,
                        column,
                        dependency.name.chars().count() as u32,
                        &format!("{qualifier}.{}", dependency.name),
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
                // A member, such as `p.x` or `Unit.Metre`, is written the
                // same in any module.
                if !is_declaration(u.kind) {
                    continue;
                }
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
                        // By this binding: `from m import Point as P, Point`
                        // binds one declaration twice.
                        let bare = |u: &Use| {
                            u.identity.as_ref() == Some(&identity)
                                && matches!(self.form_of(u), Form::Bare { name, .. } if name == binding)
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
            if self.source_qualifier.is_some() {
                break;
            }
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
        let mut source_qualified = false;
        let cannot_rewrite = |path: &Path, u: &Use| {
            format!(
                "{}:{}:{}: cannot rewrite this reference to `{name}`",
                self.planner.package.relative(path),
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
        let mut rewrites = vec![];
        let mut file_imports: Vec<(PathBuf, ImportChanges)> = vec![];
        for (path, file_uses) in &by_file {
            let lines = self.import_lines(path);
            if *path == source {
                let rest: Vec<&Use> = file_uses
                    .iter()
                    .filter(|u| !lines.contains(&location_line(&u.location)))
                    .collect();
                if rest.is_empty() {
                    continue;
                }
                source_uses_name = true;
                self.needs_public = true;
                // With `import destination` already there, write
                // `destination.name`, as moving it in the other direction
                // reads it.
                let Some(qualifier) = self.plain_binding(path, &destination)? else {
                    continue;
                };
                let bare: Option<Vec<(u32, u32)>> = rest
                    .iter()
                    .map(|u| match self.form_of(u) {
                        Form::Bare {
                            column,
                            name: written,
                        } if written == name => Some((location_line(&u.location), column)),
                        _ => None,
                    })
                    .collect();
                if let Some(bare) = bare {
                    source_qualified = true;
                    // The moved text may have been the import's only other
                    // user; the rewritten names now use it.
                    if let Some(changes) = self.imports.get_mut(path) {
                        changes.0.retain(|(_, binding)| *binding != qualifier);
                    }
                    let replacement = format!("{qualifier}.{name}");
                    let offset = qualifier.chars().count() as u32 + 1;
                    for (u, (l, column)) in rest.iter().zip(bare) {
                        rewrites.push((
                            (path.clone(), u.location.line, u.location.column),
                            (l, column, offset),
                        ));
                        file_edits.push((
                            path.clone(),
                            replace(l, column, name.chars().count() as u32, &replacement),
                        ));
                    }
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
                            self.planner.package.relative(path),
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
                            "from {} import {name}{alias}",
                            self.planner.destination_import(&prefix)
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
                                    let module = self.planner.destination_import(&prefix);
                                    additions.push(format!("import {module}"));
                                    module
                                })
                            });
                            format!("{qualifier}.{name}")
                        };
                        rewrites.push((
                            (path.clone(), u.location.line, u.location.column),
                            (
                                l,
                                column,
                                (replacement.chars().count() - name.chars().count()) as u32,
                            ),
                        ));
                        file_edits.push((path.clone(), replace(l, column, length, &replacement)));
                        // Remove `import source` when this was its last use,
                        // counting the uses the moved text adds to the
                        // destination, such as `Point` written `g.Point`.
                        let other_uses = self.uses.iter().any(|o| {
                            o.path == *path
                                && o.target != target
                                && !lines.contains(&location_line(&o.location))
                                && self.qualified_by(o, &qualifier, None)
                        });
                        let moved_uses = is_destination
                            && self.source_qualifier.as_deref() == Some(qualifier.as_str())
                            && !self.span_rewrites.is_empty();
                        if !other_uses
                            && !moved_uses
                            && !removals.iter().any(|(_, b)| *b == qualifier)
                        {
                            self.import_error(path, import)?;
                            removals.push((import.line, qualifier.clone()));
                        }
                    }
                }
            }
            file_imports.push((path.clone(), (removals, additions)));
        }
        self.rewrites.extend(rewrites);
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
            if !source_qualified {
                self.imports
                    .entry(source)
                    .or_default()
                    .1
                    .push(format!("from {destination_module} import {name}"));
            }
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
        // Line breaks take the destination's ending, except inside a string,
        // where the ending is part of the string's value.
        let strings = lex(source_text)?;
        let mut moved = String::new();
        for (i, raw) in source_text
            .split_inclusive('\n')
            .enumerate()
            .take(last as usize + 1)
            .skip(first as usize)
        {
            moved.push_str(source_lines[i]);
            if strings.breaks_string(i as u32 + 1) {
                moved.push_str(&raw[source_lines[i].len()..]);
            } else {
                moved.push_str(newline);
            }
        }
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
        let member = &self.planner.package.project.packages[self.planner.package.member];
        if self.declaration.private_at.is_none() && member.manifest.library.is_some() {
            let package = &member.manifest.name;
            self.api_changes.push(format!(
                "moves public `{package}.{}.{name}` to `{}.{name}`, so packages outside this workspace that import it must change",
                self.planner.source_module,
                self.planner.destination_import(&format!("{package}."))
            ));
        }
        if matches!(self.declaration.keyword.as_str(), "class" | "enum") {
            self.api_changes.push(format!(
                "changes the printed type name `{}.{name}` to `{}.{name}`",
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
            // Add to a `from` import of the same module already there: in
            // order when its names are sorted, else at the end.
            added.retain(|import| {
                let Some(line) = path_imports
                    .iter()
                    .filter(|i| i.from.is_some() && i.from == import.from && !i.comment)
                    .map(|i| i.line)
                    .find(|line| changed.get(line).is_none_or(|c| !c.items.is_empty()))
                else {
                    return true;
                };
                let existing = changed.entry(line).or_insert_with(|| {
                    path_imports
                        .iter()
                        .find(|i| i.line == line)
                        .expect("the line has an import")
                        .clone()
                });
                for item in &import.items {
                    let sorted = existing.items.is_sorted_by(|a, b| a.0 <= b.0);
                    let at = if sorted {
                        existing.items.partition_point(|other| other.0 <= item.0)
                    } else {
                        existing.items.len()
                    };
                    existing.items.insert(at, item.clone());
                }
                false
            });
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

/// Where the point (`line`, `column`), from 0, is after `edits`, which
/// do not overlap; None when an edit replaces it. Text inserted at the point
/// comes before it.
fn map_point(edits: &[TextEdit], line: u32, column: u32) -> Option<(u32, u32)> {
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| (e.line, e.column, e.end_line, e.end_column));
    let mut point = (line, column);
    // From the last edit, so each edit's own position is still valid.
    for e in sorted.into_iter().rev() {
        let start = (e.line, e.column);
        let end = (e.end_line, e.end_column);
        if point < end {
            if start < point {
                return None;
            }
            continue;
        }
        let breaks = e.text.matches('\n').count() as u32;
        let tail = e
            .text
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count() as u32;
        point = if point.0 == end.0 {
            let base = if breaks == 0 { start.1 + tail } else { tail };
            (start.0 + breaks, base + point.1 - end.1)
        } else {
            (point.0 - end.0 + start.0 + breaks, point.1)
        };
    }
    Some(point)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_after_a_move_repeats_only_analyses_that_read_an_edited_file() {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            (
                "plenty.toml",
                "manifest-version = 1\n\n[package]\nname = \"tools\"\nversion = \"0.1.0\"\n",
            ),
            ("src/lib.plenty", "pub def version() -> i64:\n    1\n"),
            ("src/shapes.plenty", "pub class Point:\n    pub x: i64\n"),
            ("src/geometry.plenty", "pub def unit() -> i64:\n    1\n"),
            (
                "src/main.plenty",
                "import shapes\nimport geometry\n\ndef main() -> ():\n    p = shapes.Point(x=geometry.unit())\n",
            ),
        ];
        let mut all: Vec<(String, String)> = files
            .iter()
            .map(|(p, t)| (p.to_string(), t.to_string()))
            .collect();
        for i in 0..3 {
            all.push((
                format!("tests/shape{i}.plenty"),
                "import tools.shapes\n\ndef test_point() -> ():\n    p = tools.shapes.Point(x=1)\n"
                    .into(),
            ));
            all.push((
                format!("tests/version{i}.plenty"),
                "import tools\n\ndef test_version() -> ():\n    v = tools.version()\n".into(),
            ));
        }
        for (path, text) in &all {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let root = dir.path().canonicalize().unwrap();
        let overlay = HashMap::default();
        let planner = Planner::new(
            &root.join("src/shapes.plenty"),
            &root.join("src/geometry.plenty"),
            &overlay,
        )
        .unwrap();
        planner.plan(1).unwrap();
        // Before: the library, `main` and six tests. After: `main`, the
        // three tests that import `shapes`, and `shapes` itself, which no
        // target imports any more; the library and the `version` tests read
        // no edited file.
        assert_eq!(planner.package.analyses.get(), 8 + 5);
    }
}
