//! Editor queries (issue #155). During an editor check the front end records
//! what each written name refers to, from the resolution it performs anyway:
//! module loading records modules and imports, module resolution records
//! declarations, locals, and module-level names, and lowering records what
//! needs types (methods, fields, enum variants, and the types of locals).
//! An ordinary compilation records nothing.
//!
//! The front end keeps the position of a construct's first token, which is
//! not always the name's: `def` precedes a function's name, and a field's type
//! follows its name. A record keeps that anchor and how to find the name from
//! it ([`Find`]); the file's text settles the exact range when checking ends.
//! A member name (`value.name`) carries its own position, see [`Pos`].
use super::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static RECORDER: RefCell<Recorder> = RefCell::new(Recorder::default());
}

/// A source file of an analysis: a file on disk, or an embedded `std` module
/// named by its path below the standard library root, such as `std/io.plenty`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum File {
    Path(PathBuf),
    Standard(String),
}

/// A range on one line. Lines and columns start at 1; columns and the length
/// count characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub file: File,
    pub line: u32,
    pub column: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Enum,
    Variant,
    Protocol,
    Field,
    Module,
    Local,
    Parameter,
    Constant,
    TypeAlias,
}

/// A declaration: its kind, qualified name, declaration text as written (a
/// function's header, a field's line), docstring, and where its name is.
#[derive(Clone, Debug)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    pub declaration: String,
    pub doc: String,
    pub location: Option<Location>,
}

/// A written name and the declaration it refers to. A declaration's own name
/// is a reference to it too. `ty` is the type of a local where it is read.
#[derive(Clone, Debug)]
pub struct Reference {
    pub location: Location,
    pub symbol: usize,
    pub ty: Option<String>,
}

/// A compiler error at a position; `file` is None for one without a position.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub file: Option<File>,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

/// What an editor check found. `sources` holds the text of every module the
/// check read, so an embedded `std` module can be shown.
#[derive(Default, Debug)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub sources: HashMap<File, Arc<str>>,
}

impl Analysis {
    /// The reference whose range holds the given position, if any.
    pub fn reference_at(&self, file: &File, line: u32, column: u32) -> Option<&Reference> {
        self.references
            .iter()
            .filter(|r| {
                &r.location.file == file
                    && r.location.line == line
                    && r.location.column <= column
                    && column < r.location.column + r.location.length.max(1)
            })
            .min_by_key(|r| r.location.length)
    }
}

/// How to find a name from its anchor token.
#[derive(Clone, Debug)]
pub(super) enum Find {
    /// The name starts at the anchor and is this many characters long.
    At(u32),
    /// The first occurrence at or after the anchor.
    After(String),
    /// The last occurrence on the anchor's line that starts at or before it.
    Before(String),
    /// The first occurrence on a later line.
    Below(String),
    /// The first occurrence after the `import` keyword that follows the anchor.
    Imported(String),
}

/// How to show a declaration.
#[derive(Clone, Debug)]
pub(super) enum Shape {
    /// From the name's keyword to the `:` that ends the header.
    Header,
    /// The name's line.
    Line,
    /// Given text.
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Anchor {
    source: Arc<str>,
    line: u32,
    column: u32,
}

impl Anchor {
    fn of(at: &Token) -> Option<Self> {
        Some(Anchor {
            source: at.source()?,
            line: at.line,
            column: at.column,
        })
    }
    fn at(at: &Token, pos: Pos) -> Option<Self> {
        (pos != Pos::default()).then_some(())?;
        Some(Anchor {
            source: at.source()?,
            line: pos.line,
            column: pos.column,
        })
    }
}

struct Declared {
    kind: SymbolKind,
    name: String,
    anchor: Option<(Anchor, Find)>,
    shape: Shape,
    doc: String,
}

#[derive(Default)]
struct Recorder {
    overlay: HashMap<PathBuf, String>,
    /// Text of each module read, by the source name its tokens carry.
    texts: HashMap<Arc<str>, (File, Arc<str>)>,
    keys: HashMap<String, usize>,
    declared: Vec<Declared>,
    references: Vec<(Anchor, Find, String)>,
    /// Types of locals, by the anchor of their binding or of a read.
    types: HashMap<Anchor, String>,
    /// Method names being lowered, innermost last.
    methods: Vec<(Pos, String)>,
    /// Functions whose bodies did not resolve: they keep their signatures,
    /// and lowering skips them.
    skipped: HashSet<String>,
    diagnostics: Vec<String>,
}

pub(super) fn active() -> bool {
    ACTIVE.get()
}

fn with<T: Default>(record: impl FnOnce(&mut Recorder) -> T) -> T {
    if !active() {
        return T::default();
    }
    RECORDER.with_borrow_mut(record)
}

/// Unsaved text of a file being edited, which a check reads in place of the
/// file on disk.
pub(super) fn overlay(path: &Path) -> Option<String> {
    with(|r| r.overlay.get(path).cloned())
}

/// Keep the text of a module that was read, for positions and declarations.
pub(super) fn loaded(path: &Path, standard: bool, text: &str) {
    with(|r| {
        let source: Arc<str> = path.to_string_lossy().as_ref().into();
        let file = if standard {
            File::Standard(source.to_string())
        } else {
            File::Path(path.to_path_buf())
        };
        r.texts.insert(source, (file, text.into()));
    })
}

pub(super) fn declare(
    key: String,
    kind: SymbolKind,
    name: &str,
    at: &Token,
    find: Find,
    shape: Shape,
    doc: &str,
) {
    with(|r| {
        let anchor = Anchor::of(at).map(|anchor| (anchor, find));
        if let Some((anchor, find)) = &anchor {
            r.references
                .push((anchor.clone(), find.clone(), key.clone()));
        }
        if let Entry::Vacant(entry) = r.keys.entry(key) {
            entry.insert(r.declared.len());
            r.declared.push(Declared {
                kind,
                name: name.to_owned(),
                anchor,
                shape,
                doc: doc.trim().to_owned(),
            });
        }
    })
}

/// A written name at `at` referring to the declaration `key`.
pub(super) fn refer(at: &Token, find: Find, key: String) {
    with(|r| {
        if let Some(anchor) = Anchor::of(at) {
            r.references.push((anchor, find, key));
        }
    })
}

/// A member name written at `pos`, in the file of `at`.
pub(super) fn refer_member(at: &Token, pos: Pos, name: &str, key: String) {
    with(|r| {
        if let Some(anchor) = Anchor::at(at, pos) {
            let length = name.chars().count() as u32;
            r.references.push((anchor, Find::At(length), key));
        }
    })
}

/// The key of a local bound at `pos` in the file of `at`.
pub(super) fn local_key(at: &Token, pos: Pos, name: &str) -> String {
    let source = at.source();
    let source = source.as_deref().unwrap_or_default();
    format!("local {source}:{}:{}:{name}", pos.line, pos.column)
}

/// A local's binding, anchored at `pos`, from which `find` finds its name.
pub(super) fn bind_local(at: &Token, pos: Pos, name: &str, parameter: bool, find: Find) {
    if !active() {
        return;
    }
    let key = local_key(at, pos, name);
    let anchor = Token {
        line: pos.line,
        column: pos.column,
        ..at.clone()
    };
    let kind = if parameter {
        SymbolKind::Parameter
    } else {
        SymbolKind::Local
    };
    declare(key, kind, name, &anchor, find, Shape::Text(name.into()), "");
}

/// A read of the local bound at `pos`, written at `at`.
pub(super) fn refer_local(at: &Token, pos: Pos, name: &str) {
    if active() {
        refer(
            at,
            Find::At(name.chars().count() as u32),
            local_key(at, pos, name),
        );
    }
}

/// The type of the local named or bound at `at`.
pub(super) fn local_type(at: &Token, ty: &Ty) {
    with(|r| {
        if let Some(anchor) = Anchor::of(at) {
            r.types.insert(anchor, ty.to_string());
        }
    })
}

pub(super) fn enter_method(pos: Pos, name: &str) {
    with(|r| r.methods.push((pos, name.to_owned())))
}

pub(super) fn leave_method() {
    with(|r| r.methods.pop());
}

/// The method being lowered resolved to `callee`. The innermost method name
/// being lowered is the one written for this call when the names agree.
pub(super) fn method_call(at: &Token, name: &str, callee: &str) {
    let pos = with(|r| {
        r.methods
            .last()
            .filter(|(_, written)| written == name)
            .map(|(pos, _)| *pos)
    });
    if let Some(pos) = pos {
        refer_member(at, pos, name, global(callee));
    }
}

/// The key of a module-level declaration. Instances of generic classes and
/// enums refer to their declaration.
pub(super) fn global(symbol: &str) -> String {
    if crate::record::is_method(symbol) {
        let display = crate::record::method_display(symbol);
        if let Some((class, method)) = display.rsplit_once('.') {
            return format!("global {}", crate::record::method(base(class), method));
        }
    }
    format!("global {}", base(symbol))
}

fn base(name: &str) -> &str {
    name.split('[').next().unwrap_or(name)
}

pub(super) fn field_key(class: &str, field: &str) -> String {
    format!("field {}.{field}", base(class))
}

pub(super) fn variant_key(owner: &str, variant: &str) -> String {
    format!("variant {}.{variant}", base(owner))
}

pub(super) fn module_key(module: &str) -> String {
    format!("module {module}")
}

/// Record a module the program loads, and the import naming it.
pub(super) fn module(name: &str, path: &Path, standard: bool, import: &Token) {
    if !active() {
        return;
    }
    let source: Arc<str> = path.to_string_lossy().as_ref().into();
    let at = Token {
        kind: Kind::Eof,
        line: 1,
        column: 1,
        file: Some(FileId::intern(&source)),
    };
    let display = if standard {
        name.to_owned()
    } else {
        name.rsplit("::").next().unwrap_or(name).to_owned()
    };
    declare(
        module_key(name),
        SymbolKind::Module,
        &display,
        &at,
        Find::At(0),
        Shape::Text(format!("module {display}")),
        "",
    );
    // The module's own anchor is not a written name.
    with(|r| {
        r.references.pop();
    });
    refer(import, Find::After(display), module_key(name));
}

/// Record a function whose body failed to resolve; lowering skips it.
pub(super) fn skip(name: &str) {
    with(|r| r.skipped.insert(name.to_owned()));
}

pub(super) fn skipped(name: &str) -> bool {
    with(|r| r.skipped.contains(name))
}

/// During an editor check, record `error` and continue with the next
/// function; otherwise the error ends the compilation.
pub(super) fn recover(error: &dyn Error) -> bool {
    with(|r| {
        let text = error.to_string();
        if !r.diagnostics.contains(&text) {
            r.diagnostics.push(text);
        }
        true
    })
}

/// Every standard module, with the names the compiler provides for it
/// rather than declaring in its source.
pub fn standard_modules() -> Vec<(&'static str, Vec<&'static str>)> {
    modules::standard_modules()
}

/// Check `path` as an editor does: record what names refer to, recover from
/// an error in one function to check the next, and read unsaved text from
/// `overlay` in place of the files it names. A file that defines `main` is
/// checked as an application, any other as a library module.
pub fn analyze(
    path: &Path,
    graph: Option<&crate::project::SourceGraph>,
    overlay: HashMap<PathBuf, String>,
) -> Analysis {
    RECORDER.set(Recorder {
        overlay,
        ..Recorder::default()
    });
    ACTIVE.set(true);
    let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(path, graph)));
    let result = match checked {
        Ok(result) => result,
        Err(_) => Err("internal compiler error while checking this file".into()),
    };
    if let Err(error) = result {
        recover(&*error);
    }
    ACTIVE.set(false);
    finish(RECORDER.take())
}

fn check(path: &Path, graph: Option<&crate::project::SourceGraph>) -> Result<()> {
    let mut resolved = match graph {
        Some(graph) => modules::load_graph(path, graph, false)?,
        None => modules::load(path, None, false)?,
    };
    resolved.require_main = resolved.functions.iter().any(|f| f.name == "main");
    let mut heap = Heap::default();
    let program = lower(resolved, &mut heap, crate::CheckOptions::default())?;
    // A function that failed was left out, so later passes would report
    // what is missing rather than what is wrong.
    if with(|r| r.diagnostics.is_empty()) {
        crate::op::check(&program.ops)?;
    }
    Ok(())
}

/// Record the declarations of a resolved program.
pub(super) fn declare_program(resolved: &modules::Resolved) {
    if !active() {
        return;
    }
    let short = |name: &str| name.rsplit('.').next().unwrap_or(name).to_owned();
    for f in &resolved.functions {
        if let Some(constant) = f.name.strip_prefix("$const.") {
            declare(
                global(&f.name),
                SymbolKind::Constant,
                constant,
                &f.at,
                Find::After(short(constant)),
                Shape::Line,
                &f.doc,
            );
        } else if !f.name.starts_with("__plenty_") {
            declare_function(&global(&f.name), SymbolKind::Function, &f.name, f);
        }
    }
    for c in &resolved.classes {
        declare(
            global(&c.name),
            SymbolKind::Class,
            &c.name,
            &c.at,
            Find::After(short(&c.name)),
            Shape::Header,
            "",
        );
        for (field, ty) in &c.fields {
            declare(
                field_key(&c.name, field),
                SymbolKind::Field,
                &format!("{}.{field}", c.name),
                &ty.at,
                Find::Before(field.clone()),
                Shape::Line,
                "",
            );
        }
        for f in &c.methods {
            let symbol = crate::record::method(&c.name, &f.name);
            let name = format!("{}.{}", c.name, f.name);
            declare_function(&global(&symbol), SymbolKind::Method, &name, f);
        }
    }
    for e in &resolved.enums {
        declare(
            global(&e.name),
            SymbolKind::Enum,
            &e.name,
            &e.at,
            Find::After(short(&e.name)),
            Shape::Header,
            "",
        );
        for (variant, _) in &e.variants {
            declare(
                variant_key(&e.name, variant),
                SymbolKind::Variant,
                &format!("{}.{variant}", e.name),
                &e.at,
                Find::Below(variant.clone()),
                Shape::Line,
                "",
            );
        }
        for f in &e.methods {
            let symbol = crate::record::method(&e.name, &f.name);
            let name = format!("{}.{}", e.name, f.name);
            declare_function(&global(&symbol), SymbolKind::Method, &name, f);
        }
    }
    for p in &resolved.protocols {
        declare(
            global(&p.name),
            SymbolKind::Protocol,
            &p.name,
            &p.at,
            Find::After(short(&p.name)),
            Shape::Header,
            "",
        );
        for f in &p.methods {
            let name = format!("{}.{}", p.name, f.name);
            declare_function(&global(&name), SymbolKind::Method, &name, f);
        }
    }
    for a in &resolved.declarations {
        declare(
            global(&a.name),
            SymbolKind::TypeAlias,
            &a.name,
            &a.at,
            Find::After(short(&a.name)),
            Shape::Line,
            "",
        );
    }
}

fn declare_function(key: &str, kind: SymbolKind, name: &str, f: &Function) {
    let short = f.name.rsplit('.').next().unwrap_or(&f.name).to_owned();
    declare(
        key.to_owned(),
        kind,
        name,
        &f.at,
        Find::After(short),
        Shape::Header,
        &f.doc,
    );
}

fn finish(recorder: Recorder) -> Analysis {
    let Recorder {
        texts,
        keys,
        declared,
        references,
        types,
        diagnostics,
        ..
    } = recorder;
    let lines: HashMap<&Arc<str>, Vec<usize>> = texts
        .iter()
        .map(|(source, (_, text))| {
            let starts = std::iter::once(0)
                .chain(text.match_indices('\n').map(|(i, _)| i + 1))
                .collect();
            (source, starts)
        })
        .collect();
    let locate = |anchor: &Anchor, find: &Find| -> Option<(Location, usize)> {
        let (file, text) = texts.get(&anchor.source)?;
        let starts = lines.get(&anchor.source)?;
        let offset = offset(text, starts, anchor.line, anchor.column)?;
        let (start, length) = find_name(text, starts, offset, anchor.line, find)?;
        let line = starts.partition_point(|&s| s <= start);
        let column = text[starts[line - 1]..start].chars().count() + 1;
        Some((
            Location {
                file: file.clone(),
                line: line as u32,
                column: column as u32,
                length,
            },
            offset,
        ))
    };
    let mut symbols = Vec::with_capacity(declared.len());
    for d in &declared {
        let found = d
            .anchor
            .as_ref()
            .and_then(|(a, f)| Some((a, locate(a, f)?)));
        let declaration = match (&d.shape, &found) {
            (Shape::Text(text), _) => match (d.kind, &d.anchor) {
                (SymbolKind::Local | SymbolKind::Parameter, Some((anchor, _))) => {
                    match types.get(anchor) {
                        Some(ty) => format!("{text}: {ty}"),
                        None => text.clone(),
                    }
                }
                _ => text.clone(),
            },
            (Shape::Header, Some((a, (_, offset)))) => {
                // Show `pub` written before the keyword.
                let text = &texts[&a.source].1;
                let start = starts_of_line(&lines[&a.source], *offset);
                let before = text[start..*offset].trim();
                let from = if before == "pub" {
                    start + text[start..].len() - text[start..].trim_start().len()
                } else {
                    *offset
                };
                header(text, from)
            }
            (Shape::Line, Some((a, (location, _)))) => {
                let text = &texts[&a.source].1;
                let starts = &lines[&a.source];
                let line = location.line as usize;
                let end = starts.get(line).copied().unwrap_or(text.len());
                text[starts[line - 1]..end].trim().to_owned()
            }
            _ => d.name.clone(),
        };
        symbols.push(Symbol {
            kind: d.kind,
            name: d.name.clone(),
            declaration,
            doc: d.doc.clone(),
            location: found.map(|(_, (location, _))| location),
        });
    }
    let mut seen = HashSet::default();
    let mut found = Vec::new();
    for (anchor, find, key) in &references {
        let Some(&symbol) = keys.get(key) else {
            continue;
        };
        let Some((location, _)) = locate(anchor, find) else {
            continue;
        };
        if location.length == 0 || !seen.insert((location.clone(), symbol)) {
            continue;
        }
        found.push(Reference {
            location,
            symbol,
            ty: types.get(anchor).cloned(),
        });
    }
    let diagnostics = diagnostics
        .iter()
        .map(|text| diagnostic(text, &texts))
        .collect();
    Analysis {
        diagnostics,
        symbols,
        references: found,
        sources: texts.into_values().collect(),
    }
}

/// Byte offset of the start of the line holding `offset`.
fn starts_of_line(starts: &[usize], offset: usize) -> usize {
    starts[starts.partition_point(|&s| s <= offset) - 1]
}

/// Byte offset of a 1-based line and character column.
fn offset(text: &str, starts: &[usize], line: u32, column: u32) -> Option<usize> {
    let start = *starts.get((line as usize).checked_sub(1)?)?;
    let rest = &text[start..];
    let skip = (column as usize).saturating_sub(1);
    Some(start + rest.char_indices().nth(skip).map_or(rest.len(), |(i, _)| i))
}

fn word_char(c: char, dotted: bool) -> bool {
    c.is_alphanumeric() || c == '_' || (dotted && c == '.')
}

/// Whole-word occurrences of `name` in `text[from..to]`.
fn occurrences<'a>(
    text: &'a str,
    name: &'a str,
    from: usize,
    to: usize,
) -> impl Iterator<Item = usize> + 'a {
    let dotted = name.contains('.');
    text[from..to]
        .match_indices(name)
        .map(move |(i, _)| from + i)
        .filter(move |&i| {
            !text[..i]
                .chars()
                .next_back()
                .is_some_and(|c| word_char(c, dotted))
                && !text[i + name.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| word_char(c, dotted))
        })
}

/// Where `find` puts the name, from `offset` on `line`: its byte offset and
/// its length in characters.
fn find_name(
    text: &str,
    starts: &[usize],
    offset: usize,
    line: u32,
    find: &Find,
) -> Option<(usize, u32)> {
    let line_end = |line: usize| starts.get(line).map_or(text.len(), |&s| s - 1);
    // A header or an import spans a few lines at most.
    // The window end must not split a character, or slicing it panics.
    let near = |from: usize| {
        let mut to = text.len().min(from + 4096);
        while !text.is_char_boundary(to) {
            to -= 1;
        }
        to
    };
    let length = |name: &str| name.chars().count() as u32;
    match find {
        Find::At(length) => Some((offset, *length)),
        Find::After(name) => {
            let start = occurrences(text, name, offset, near(offset)).next()?;
            Some((start, length(name)))
        }
        Find::Before(name) => {
            let first = starts[line as usize - 1];
            let end = line_end(line as usize).max(offset);
            let start = occurrences(text, name, first, end)
                .filter(|&i| i <= offset)
                .last()?;
            Some((start, length(name)))
        }
        Find::Below(name) => {
            let next = starts.get(line as usize).copied()?;
            let start = occurrences(text, name, next, near(next)).next()?;
            Some((start, length(name)))
        }
        Find::Imported(name) => {
            let keyword = occurrences(text, "import", offset, near(offset)).next()?;
            let start = occurrences(text, name, keyword + 6, near(keyword)).next()?;
            Some((start, length(name)))
        }
    }
}

/// A declaration's header from its keyword up to the `:` that ends it,
/// outside brackets and strings, with runs of whitespace collapsed.
fn header(text: &str, offset: usize) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    let mut quote = None;
    let mut chars = text[offset..].chars();
    while let Some(c) = chars.next() {
        if out.len() > 2000 {
            out.push('…');
            break;
        }
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                out.extend(chars.next());
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' | '\n' if depth <= 0 => break,
            '#' => {
                chars.by_ref().find(|&c| c == '\n');
                continue;
            }
            _ => {}
        }
        if c.is_whitespace() {
            if !out.ends_with([' ', '(', '[']) {
                out.push(' ');
            }
        } else {
            if matches!(c, ')' | ']' | ',') && out.ends_with(' ') {
                out.pop();
            }
            out.push(c);
        }
    }
    out.trim().replace(",)", ")").replace(",]", "]")
}

/// A diagnostic from an error's text, which starts with `source:line:column: `
/// when the error has a position.
fn diagnostic(text: &str, texts: &HashMap<Arc<str>, (File, Arc<str>)>) -> Diagnostic {
    let first = text.lines().next().unwrap_or_default();
    let mut search = 0;
    while let Some(i) = first[search..].find(": ").map(|i| i + search) {
        let mut parts = first[..i].rsplitn(3, ':');
        let column = parts.next().and_then(|c| c.parse::<u32>().ok());
        let line = parts.next().and_then(|l| l.parse::<u32>().ok());
        if let (Some(column), Some(line)) = (column, line) {
            let source = parts.next().unwrap_or_default();
            let file = (!source.is_empty()).then(|| {
                texts
                    .get(source)
                    .map(|(file, _)| file.clone())
                    .unwrap_or_else(|| File::Path(PathBuf::from(source)))
            });
            return Diagnostic {
                file,
                line,
                column,
                message: text[i + 2..].to_owned(),
            };
        }
        search = i + 2;
    }
    Diagnostic {
        file: None,
        line: 1,
        column: 1,
        message: text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_ends_at_its_colon_outside_brackets() {
        let text = "pub def f[T: Show](a: dict[str, i64] = {\"k\": 1},\n    b: &mut T) -> i64:\n    pass\n";
        assert_eq!(
            header(text, 4),
            "def f[T: Show](a: dict[str, i64] = {\"k\": 1}, b: &mut T) -> i64"
        );
    }

    #[test]
    fn a_diagnostic_keeps_its_position() {
        let found = diagnostic(
            "/a/b.plenty:3:7: expected i64, got str",
            &HashMap::default(),
        );
        assert_eq!((found.line, found.column), (3, 7));
        assert_eq!(found.message, "expected i64, got str");
        assert_eq!(found.file, Some(File::Path("/a/b.plenty".into())));
        let found = diagnostic("reading x: missing", &HashMap::default());
        assert_eq!((found.file, found.line), (None, 1));
    }

    #[test]
    fn names_are_found_from_their_anchors() {
        let text = "class Point:\n    pub x: i64\nenum Color:\n    Red\n";
        let starts: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        assert_eq!(
            find_name(text, &starts, 0, 1, &Find::After("Point".into())),
            Some((6, 5))
        );
        let ty = text.find("i64").unwrap();
        assert_eq!(
            find_name(text, &starts, ty, 2, &Find::Before("x".into())),
            Some((text.find("x:").unwrap(), 1))
        );
        let color = text.find("enum").unwrap();
        assert_eq!(
            find_name(text, &starts, color, 3, &Find::Below("Red".into())),
            Some((text.find("Red").unwrap(), 3))
        );
    }

    #[test]
    fn a_search_window_ends_on_a_character_boundary() {
        // Byte 4096 falls inside a two-byte "é", so the window must stop short.
        let text = format!("def f() -> ():\n# {}\n", "é".repeat(2100));
        let starts: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        assert!(!text.is_char_boundary(4096));
        assert_eq!(
            find_name(&text, &starts, 0, 1, &Find::After("f".into())),
            Some((4, 1))
        );
        assert_eq!(
            find_name(&text, &starts, 0, 1, &Find::After("missing".into())),
            None
        );
    }
}
