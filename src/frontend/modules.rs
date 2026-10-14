//! Source loading, explicit name binding, and module privacy before lowering.
//!
//! The backend still consumes one program. Qualified declaration keys retain
//! nominal identity while aliases only affect the names visible in each source.
use super::*;
use std::path::{Path, PathBuf};

/// Toolchain sources have stable identities and never participate in filesystem
/// search. Keeping them in the compiler also makes installation relocatable.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ModuleSource {
    File(PathBuf),
    Standard(&'static str),
}

impl ModuleSource {
    fn path(&self) -> PathBuf {
        match self {
            Self::File(path) => path.clone(),
            Self::Standard(name) => PathBuf::from(format!("{}.plenty", name.replace('.', "/"))),
        }
    }

    fn read(&self) -> Result<String> {
        match self {
            Self::Standard("std.math") => Ok(include_str!("../../stdlib/std/math.plenty").into()),
            Self::Standard("std.fs") => Ok(include_str!("../../stdlib/std/fs.plenty").into()),
            Self::Standard("std.path") => Ok(include_str!("../../stdlib/std/path.plenty").into()),
            Self::Standard("std.deque") => Ok(include_str!("../../stdlib/std/deque.plenty").into()),
            Self::Standard("std.error") => Ok(include_str!("../../stdlib/std/error.plenty").into()),
            Self::Standard("std.bytes") => Ok(include_str!("../../stdlib/std/bytes.plenty").into()),
            Self::Standard("std.time") => Ok(include_str!("../../stdlib/std/time.plenty").into()),
            Self::Standard("std.cli") => Ok(include_str!("../../stdlib/std/cli.plenty").into()),
            Self::Standard("std.text") => Ok(include_str!("../../stdlib/std/text.plenty").into()),
            Self::Standard("std.unicode") => {
                Ok(include_str!("../../stdlib/std/unicode.plenty").into())
            }
            Self::Standard("std.thread" | "std.channel" | "std.concurrent" | "std.sync") => {
                Ok(String::new())
            }
            Self::Standard("std.io") => Ok(include_str!("../../stdlib/std/io.plenty").into()),
            Self::Standard("std.encoding") => {
                Ok(include_str!("../../stdlib/std/encoding.plenty").into())
            }
            Self::Standard(_) => unreachable!("validated standard module"),
            Self::File(path) => std::fs::read_to_string(path)
                .map_err(|e| format!("reading {}: {e}", path.display()).into()),
        }
    }
}

/// These exports are identities, not wrapper functions: lexical task scopes,
/// owned jobs, and wait diagnostics must reach the existing builtin lowering.
fn facility_exports(module: &str) -> &'static [&'static str] {
    match module {
        "std.thread" => &["spawn", "ThreadError", "SpawnError"],
        "std.channel" => &[
            "channel",
            "Sender",
            "Receiver",
            "ChannelError",
            "SendError",
            "RecvError",
            "SendTimeoutError",
            "RecvTimeoutError",
            "Selected",
            "SelectError",
            "select_recv",
            "select_recv_nowait",
            "select_recv_timeout",
        ],
        "std.concurrent" => &[
            "ThreadPoolExecutor",
            "Future",
            "PoolError",
            "SubmitError",
            "FutureError",
            "PoolMapError",
            "ParallelError",
        ],
        "std.sync" => &["CancellationToken"],
        _ => &[],
    }
}

struct Import {
    at: Token,
    module: String,
    item: Option<String>,
    binding: String,
}

#[derive(Default)]
struct Module {
    name: String,
    facility_exports: &'static [&'static str],
    functions: Vec<Function>,
    constants: HashSet<String>,
    declarations: Vec<TypeAlias>,
    enums: Vec<enums::EnumDecl>,
    classes: Vec<classes::ClassDecl>,
    protocols: Vec<protocols::Protocol>,
    imports: Vec<Import>,
    exports: HashSet<String>,
}

pub(super) struct Access {
    owner: Option<Arc<str>>,
    public: bool,
}

#[derive(Default)]
pub(super) struct AccessMap {
    members: HashMap<(String, String), Access>,
    types: HashMap<String, Access>,
    protocols: HashSet<String>,
}

pub(super) struct Resolved {
    pub(super) source_paths: Vec<PathBuf>,
    pub(super) source_root: Option<PathBuf>,
    pub(super) functions: Vec<Function>,
    pub(super) declarations: Vec<TypeAlias>,
    pub(super) enums: Vec<enums::EnumDecl>,
    pub(super) classes: Vec<classes::ClassDecl>,
    pub(super) protocols: Vec<protocols::Protocol>,
    pub(super) access: AccessMap,
    pub(super) public_api: Vec<TypeRef>,
    pub(super) at: Token,
    pub(super) require_main: bool,
}

fn qualified(module: &str, name: &str) -> String {
    if module.is_empty() {
        name.into()
    } else {
        format!("{module}.{name}")
    }
}

/// `qualified` in place: a root-module name is already its own symbol.
fn qualify(module: &str, name: &mut String) {
    if !module.is_empty() {
        *name = format!("{module}.{name}");
    }
}

fn numeric_type_name(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64"
    )
}

impl Parser {
    /// Scalar constants use the ordinary typed function lowering, with a
    /// compiler-only getter name. Initializers cannot execute user code.
    fn constant(&mut self) -> Result<Function> {
        let at = self.take(); // const
        let name = self.name()?;
        self.expect(":")?;
        let output = self.ty()?;
        if !output.args.is_empty() || !output.name.as_deref().is_some_and(numeric_type_name) {
            return Err(output
                .at
                .error("module constants require a concrete numeric type"));
        }
        self.expect("=")?;
        let value = self.expr(0)?;
        let valid = match &ungroup(&value).kind {
            Expression::Number(_) => true,
            Expression::Member(base, _) => {
                matches!(&base.kind, Expression::Name(n) if numeric_type_name(n))
            }
            Expression::Unary(op, inner) if op == "-" => {
                matches!(&ungroup(inner).kind, Expression::Number(_))
                    || matches!(&ungroup(inner).kind, Expression::Member(base, _)
                        if matches!(&base.kind, Expression::Name(n) if n == "f32" || n == "f64"))
            }
            _ => false,
        };
        if !valid {
            return Err(value
                .at
                .error("constant initializer must be a numeric literal or numeric type constant"));
        }
        self.kind(Kind::Newline, "the end of the constant declaration")?;
        Ok(Function {
            once: false,
            captures: vec![],
            foreign: None,
            export: None,
            name,
            type_params: vec![],
            at: at.clone(),
            inputs: vec![],
            output,
            doc: String::new(),
            doc_at: None,
            body: vec![Stmt {
                at,
                kind: Statement::Expr(value),
            }],
        })
    }

    fn module_path(&mut self) -> Result<String> {
        let mut path = self.name()?;
        while self.eat(".") {
            path.push('.');
            path.push_str(&self.name()?);
        }
        Ok(path)
    }

    fn imports(&mut self) -> Result<Vec<Import>> {
        let at = self.peek().clone();
        let from = if self.eat("from") {
            let module = self.module_path()?;
            self.expect("import")?;
            Some(module)
        } else {
            self.expect("import")?;
            None
        };
        let mut imports = Vec::new();
        loop {
            let (module, item, default) = if let Some(module) = &from {
                let item = self.name()?;
                (module.clone(), Some(item.clone()), item)
            } else {
                let module = self.module_path()?;
                (module.clone(), None, module)
            };
            let binding = if self.eat("as") {
                self.name()?
            } else {
                default
            };
            imports.push(Import {
                at: at.clone(),
                module,
                item,
                binding,
            });
            if !self.eat(",") {
                break;
            }
        }
        self.kind(Kind::Newline, "the end of the import")?;
        Ok(imports)
    }
}

/// Items buffered behind the one being parsed. The parser reads no further
/// than the first token of the next item, where it reports a missing block.
const LOOKAHEAD: usize = 1;

fn parse(source: &str, path: Option<&Path>, name: String) -> Result<(Module, Token)> {
    if let Some(parsed) = parse_buffered(source, path, name.clone(), LOOKAHEAD)? {
        return Ok(parsed);
    }
    let parsed = parse_buffered(source, path, name, usize::MAX)?;
    Ok(parsed.expect("a parser holding the whole module has every token"))
}

/// Parses a module while holding the tokens of only `lookahead` items past
/// the one being parsed, and the syntax tree. Returns None when the parser
/// asked for a later token: the module must then be parsed with all of them.
fn parse_buffered(
    source: &str,
    path: Option<&Path>,
    name: String,
    lookahead: usize,
) -> Result<Option<(Module, Token)>> {
    let located = |e: Box<dyn Error>| -> Box<dyn Error> {
        match path {
            Some(p) => format!("{}:{e}", p.display()).into(),
            None => e,
        }
    };
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(path.map(|p| Arc::<str>::from(p.to_string_lossy().as_ref())));
    let parsed = parse_items(&mut parser, &mut lexer, path, name, lookahead, &located);
    if parser.starved.get() {
        return Ok(None);
    }
    if parsed.is_err() {
        // A lexical error anywhere in the module is reported before any
        // syntax error, as it is when all the tokens are lexed first.
        lexer.rest().map_err(located)?;
    }
    parsed.map(Some)
}

fn parse_items(
    parser: &mut Parser,
    lexer: &mut Lexer,
    path: Option<&Path>,
    name: String,
    lookahead: usize,
    located: &dyn Fn(Box<dyn Error>) -> Box<dyn Error>,
) -> Result<(Module, Token)> {
    let mut module = Module {
        name,
        ..Module::default()
    };
    let mut declared = HashSet::new();
    loop {
        parser.fill(lexer, lookahead).map_err(located)?;
        if parser.peek().kind == Kind::Eof {
            break;
        }
        let public = parser.eat("pub");
        let at = parser.peek().clone();
        let name = if parser.peek().is("extern") || parser.peek().is("opaque") {
            if !path.is_some_and(|p| p.extension().is_some_and(|e| e == "plentyi")) {
                return Err(at.error("C declarations require a trusted .plentyi interface module"));
            }
            if parser.eat("extern") {
                parser.expect("def")?;
                parser.pos -= 1;
                let f = parser.function_header(None, true, false, false)?;
                let name = f.name.clone();
                module.functions.push(f);
                name
            } else {
                parser.expect("opaque")?;
                let name = parser.name()?;
                parser.kind(Kind::Newline, "the end of the opaque pointer declaration")?;
                module.declarations.push(TypeAlias {
                    at: at.clone(),
                    name: name.clone(),
                    target: TypeRef {
                        at: at.clone(),
                        name: None,
                        args: vec![],
                        concrete: Some(Ty::ForeignPtr(qualified(&module.name, &name).into())),
                    },
                });
                name
            }
        } else if parser.eat("export") {
            parser.expect("def")?;
            parser.pos -= 1;
            let f = parser.function_header(None, false, true, false)?;
            let name = f.name.clone();
            module.functions.push(f);
            name
        } else if parser.peek().is("def") {
            let f = parser.function()?;
            let name = f.name.clone();
            module.functions.push(f);
            name
        } else if parser.peek().is("const") {
            let f = parser.constant()?;
            let name = f.name.clone();
            module.constants.insert(name.clone());
            module.functions.push(f);
            name
        } else if parser.peek().is("class") {
            let c = parser.class_decl()?;
            let name = c.name.clone();
            module.classes.push(c);
            name
        } else if parser.peek().is("protocol") {
            let p = parser.protocol_decl()?;
            let name = p.name.clone();
            module.protocols.push(p);
            name
        } else if parser.peek().is("enum") {
            let e = parser.enum_decl()?;
            let name = e.name.clone();
            module.enums.push(e);
            name
        } else if parser.peek().is("type") {
            let a = parser.alias()?;
            let name = a.name.clone();
            module.declarations.push(a);
            name
        } else if !public && (parser.peek().is("import") || parser.peek().is("from")) {
            let imports = parser.imports()?;
            for import in &imports {
                if import.binding.split('.').next() == Some("std")
                    && import.module.split('.').next() != Some("std")
                {
                    return Err(import
                        .at
                        .error("`std` is reserved for the toolchain standard library"));
                }
            }
            module.imports.extend(imports);
            continue;
        } else {
            return Err(at.error(if public {
                "pub requires a function, class, enum, type, or const declaration; re-exports are not supported"
            } else {
                "executable statements are not allowed at module scope; put them inside `def main() -> ():`"
            }));
        };
        if name == "std" {
            return Err(at.error("`std` is reserved for the toolchain standard library"));
        }
        if !declared.insert(name.clone()) && (at.is("const") || module.constants.contains(&name)) {
            return Err(at.error(format!("duplicate declaration `{name}`")));
        }
        if builtin(&name) {
            return Err(at.error(format!("cannot redefine builtin `{name}` as a type")));
        }
        if public {
            module.exports.insert(name);
        }
    }
    Ok((module, parser.peek().clone()))
}

impl Module {
    fn names(&self) -> impl Iterator<Item = &String> {
        self.functions
            .iter()
            .map(|f| &f.name)
            .chain(self.declarations.iter().map(|a| &a.name))
            .chain(self.enums.iter().map(|e| &e.name))
            .chain(self.classes.iter().map(|c| &c.name))
            .chain(self.protocols.iter().map(|p| &p.name))
    }
}

pub(super) fn single(source: &str) -> Result<Resolved> {
    let (module, at) = parse(source, None, String::new())?;
    if let Some(import) = module.imports.first() {
        return Err(import
            .at
            .error("imports require a file-based compilation API and a source root"));
    }
    resolve(vec![module], vec![vec![]], at, true)
}

pub(super) fn generated_interface(source: &str) -> Result<Resolved> {
    let (module, at) = parse(source, Some(Path::new("generated.plentyi")), String::new())?;
    if !module.imports.is_empty() {
        return Err(at.error("generated runtime interfaces must be self-contained"));
    }
    resolve(vec![module], vec![vec![]], at, false)
}

pub(super) fn load(path: &Path, root: Option<&Path>, require_main: bool) -> Result<Resolved> {
    let entry = path
        .canonicalize()
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    let root = root
        .unwrap_or_else(|| entry.parent().unwrap())
        .canonicalize()
        .map_err(|e| format!("source root: {e}"))?;
    if !root.is_dir() || !entry.starts_with(&root) {
        return Err(format!(
            "entry {} must be inside source root {}",
            entry.display(),
            root.display()
        )
        .into());
    }
    if ["std", "std.plenty", "std.plentyi"]
        .iter()
        .any(|name| root.join(name).exists())
    {
        return Err("`std` is reserved for the toolchain standard library; rename the local module or namespace".into());
    }
    struct Loader {
        root: PathBuf,
        active: Vec<ModuleSource>,
        loaded: HashMap<ModuleSource, usize>,
        modules: Vec<Module>,
        dependencies: Vec<Vec<usize>>,
    }
    impl Loader {
        fn file(&mut self, source: ModuleSource, name: String) -> Result<(usize, Token)> {
            if let Some(start) = self.active.iter().position(|p| p == &source) {
                let cycle = self.active[start..]
                    .iter()
                    .chain(std::iter::once(&source))
                    .map(|p| p.path().display().to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                return Err(format!("circular import: {cycle}").into());
            }
            if self.active.len() >= 128 || self.modules.len() >= 4096 {
                return Err(
                    "module graph exceeds the implementation limit (128 depth, 4096 modules)"
                        .into(),
                );
            }
            self.active.push(source.clone());
            let (mut module, at) = parse(&source.read()?, Some(&source.path()), name)?;
            if let ModuleSource::Standard(name) = &source {
                module.facility_exports = facility_exports(name);
            }
            let mut deps = Vec::new();
            for import in &module.imports {
                let dependency = self.import_path(import)?;
                let index = if let Some(index) = self.loaded.get(&dependency) {
                    *index
                } else {
                    // Canonical relative paths make alternate spellings refer to one module.
                    let identity = match &dependency {
                        ModuleSource::Standard(name) => (*name).into(),
                        ModuleSource::File(path) => path
                            .strip_prefix(&self.root)
                            .unwrap()
                            .with_extension("")
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("."),
                    };
                    self.file(dependency, identity)
                        .map_err(|e| import.at.error(e))?
                        .0
                };
                deps.push(index);
            }
            self.active.pop();
            if self.modules.len() >= 4096 {
                return Err(
                    at.error("module graph exceeds the implementation limit of 4096 modules")
                );
            }
            let index = self.modules.len();
            self.loaded.insert(source, index);
            self.modules.push(module);
            self.dependencies.push(deps);
            Ok((index, at))
        }

        fn import_path(&self, import: &Import) -> Result<ModuleSource> {
            if import.module.split('.').next() == Some("std") {
                return match import.module.as_str() {
                    "std.math" => Ok(ModuleSource::Standard("std.math")),
                    "std.fs" => Ok(ModuleSource::Standard("std.fs")),
                    "std.path" => Ok(ModuleSource::Standard("std.path")),
                    "std.deque" => Ok(ModuleSource::Standard("std.deque")),
                    "std.error" => Ok(ModuleSource::Standard("std.error")),
                    "std.bytes" => Ok(ModuleSource::Standard("std.bytes")),
                    "std.time" => Ok(ModuleSource::Standard("std.time")),
                    "std.cli" => Ok(ModuleSource::Standard("std.cli")),
                    "std.text" => Ok(ModuleSource::Standard("std.text")),
                    "std.unicode" => Ok(ModuleSource::Standard("std.unicode")),
                    "std.thread" => Ok(ModuleSource::Standard("std.thread")),
                    "std.channel" => Ok(ModuleSource::Standard("std.channel")),
                    "std.concurrent" => Ok(ModuleSource::Standard("std.concurrent")),
                    "std.sync" => Ok(ModuleSource::Standard("std.sync")),
                    "std.io" => Ok(ModuleSource::Standard("std.io")),
                    "std.encoding" => Ok(ModuleSource::Standard("std.encoding")),
                    _ => Err(import.at.error(format!(
                "unknown standard library module `{}`; available modules: std.math, std.fs, std.path, std.deque, std.error, std.bytes, std.time, std.cli, std.text, std.unicode, std.thread, std.channel, std.concurrent, std.sync, std.io, std.encoding",
                        import.module
                ))),
                };
            }
            let mut path = self.root.clone();
            let parts: Vec<_> = import.module.split('.').collect();
            for (i, part) in parts.iter().enumerate() {
                path.push(part);
                let file = path.with_extension("plenty");
                let interface = path.with_extension("plentyi");
                if (path.is_dir() && (file.exists() || interface.exists()))
                    || (file.exists() && interface.exists())
                {
                    return Err(import.at.error(format!(
                        "ambiguous module `{}`: conflicting namespace or .plenty/.plentyi files at {} and {}",
                        import.module,
                        path.display(),
                        file.display()
                    )));
                }
                if i + 1 != parts.len() && !path.is_dir() {
                    return Err(import.at.error(format!(
                        "module namespace {} is not a directory",
                        path.display()
                    )));
                }
            }
            let file = if path.with_extension("plentyi").exists() {
                path.with_extension("plentyi")
            } else {
                path.with_extension("plenty")
            };
            let canonical = file.canonicalize().map_err(|e| {
                import.at.error(format!(
                    "loading `{}` at {}: {e}",
                    import.module,
                    file.display()
                ))
            })?;
            if !canonical.starts_with(&self.root) {
                return Err(import.at.error("import resolves outside the source root"));
            }
            let relative = canonical.strip_prefix(&self.root).unwrap();
            let stem = relative.with_extension("");
            if relative
                .extension()
                .is_none_or(|e| e != "plenty" && e != "plentyi")
                || !stem.components().all(|component| {
                    let name = component.as_os_str().to_string_lossy();
                    let mut chars = name.chars();
                    chars
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                })
            {
                return Err(import.at.error(
                    "canonical imported paths must be .plenty files or .plentyi interfaces with identifier components",
                ));
            }
            Ok(ModuleSource::File(canonical))
        }
    }
    let mut loader = Loader {
        root,
        active: vec![],
        loaded: HashMap::new(),
        modules: vec![],
        dependencies: vec![],
    };
    let (_, at) = loader.file(ModuleSource::File(entry), String::new())?;
    let mut result = resolve(loader.modules, loader.dependencies, at, require_main)?;
    result.source_paths = loader
        .loaded
        .into_keys()
        .filter_map(|source| match source {
            ModuleSource::File(path) => Some(path),
            ModuleSource::Standard(_) => None,
        })
        .collect();
    result.source_paths.sort();
    result.source_root = Some(loader.root);
    Ok(result)
}

#[derive(Clone)]
struct Scope {
    captures: HashSet<String>,
    uncaptured: HashSet<String>,
    module: String,
    // Module-wide tables are shared: every function, class, and closure gets
    // its own scope, and copying these per scope is quadratic (issue #53).
    /// Names the module declares. Each resolves to its qualified name, which
    /// is built on use: storing it would double this table (issue #57).
    declared: Arc<HashSet<String>>,
    /// Names bound by `from module import name`, and their symbols.
    imported: Arc<HashMap<String, String>>,
    namespaces: Arc<HashMap<String, Arc<HashMap<String, String>>>>,
    constants: Arc<HashMap<String, String>>,
    type_params: HashSet<String>,
}

impl Scope {
    fn fresh_capture_binding<'a>(
        &self,
        names: impl IntoIterator<Item = &'a String>,
        at: &Token,
    ) -> Result<()> {
        if let Some(name) = names.into_iter().find(|name| self.captures.contains(*name)) {
            return Err(at.error(format!(
                "cannot shadow closure capture `{name}` with a new binding"
            )));
        }
        Ok(())
    }
    fn symbol(&self, path: &str, at: &Token) -> Result<Option<String>> {
        if self.declared.contains(path) {
            return Ok(Some(qualified(&self.module, path)));
        }
        if let Some(symbol) = self.imported.get(path) {
            return Ok(Some(symbol.clone()));
        }
        if let Some((prefix, member)) = path.rsplit_once('.') {
            if let Some(exports) = self.namespaces.get(prefix) {
                return exports.get(member).cloned().map(Some).ok_or_else(|| {
                    at.error(format!("`{path}` is private or is not a declared export"))
                });
            }
        }
        Ok(None)
    }
    fn namespace_root(&self, name: &str) -> bool {
        self.namespaces
            .keys()
            .any(|p| p.split('.').next() == Some(name))
    }
    fn ty(&self, ty: &mut TypeRef) -> Result<()> {
        if ty.concrete.is_some() {
            return Ok(());
        }
        for arg in &mut ty.args {
            self.ty(arg)?;
        }
        if let Some(name) = &mut ty.name {
            if self.type_params.contains(name) {
                return Ok(());
            }
            // Imports can bind a builtin spelling only to that same identity.
            // Avoid probing the module table for these common types (issue #57).
            if builtin(name) {
                return Ok(());
            }
            if let Some(symbol) = self.symbol(name, &ty.at)? {
                *name = symbol;
            } else if !matches!(name.as_str(), "&" | "&mut") {
                return Err(ty.at.error(format!("unknown type `{name}`")));
            }
        }
        Ok(())
    }
    fn function(&self, f: &mut Function) -> Result<()> {
        let mut scope = self.clone();
        scope.captures = f.captures.iter().map(|c| c.name.clone()).collect();
        scope
            .type_params
            .extend(f.type_params.iter().map(|(n, _)| n.clone()));
        for (_, bound) in &mut f.type_params {
            if let Some(bound) = bound {
                scope.ty(bound)?;
            }
        }
        for (_, ty) in &mut f.inputs {
            scope.ty(ty)?;
        }
        scope.ty(&mut f.output)?;
        let mut locals: HashSet<_> = f.inputs.iter().map(|(n, _)| n.clone()).collect();
        locals.extend(f.captures.iter().map(|c| c.name.clone()));
        scope.block(&mut f.body, &mut locals)
    }
    fn block(&self, body: &mut [Stmt], locals: &mut HashSet<String>) -> Result<()> {
        for stmt in body {
            match &mut stmt.kind {
                Statement::With {
                    manager,
                    name,
                    body,
                } => {
                    self.expr(manager, locals)?;
                    self.fresh_capture_binding(name.iter(), &stmt.at)?;
                    let mut inner = locals.clone();
                    inner.extend(name.iter().cloned());
                    self.block(body, &mut inner)?;
                }
                Statement::Expr(e) | Statement::Yield(e) => self.expr(e, locals)?,
                Statement::Return(e) => {
                    if let Some(e) = e {
                        self.expr(e, locals)?;
                    }
                }
                Statement::Assign {
                    name,
                    annotation,
                    value,
                    mutable,
                } => {
                    if self.captures.contains(name) && (*mutable || annotation.is_some()) {
                        return Err(stmt.at.error("cannot redeclare a closure capture"));
                    }
                    if let Some(ty) = annotation {
                        self.ty(ty)?;
                    }
                    self.expr(value, locals)?;
                    locals.insert(name.clone());
                }
                Statement::Unpack { names, value, .. } => {
                    self.fresh_capture_binding(names.iter(), &stmt.at)?;
                    self.expr(value, locals)?;
                    locals.extend(names.iter().cloned());
                }
                Statement::Augment { target, value, .. } => {
                    self.expr(target, locals)?;
                    self.expr(value, locals)?;
                }
                Statement::SetIndex { target, value } => match &mut target.kind {
                    Expression::Tuple(targets) => {
                        self.expr(value, locals)?;
                        for target in targets {
                            // A name target binds as unpacking does, so it must
                            // not resolve to a declaration of the same name.
                            if let Expression::Name(name) = &target.kind {
                                self.fresh_capture_binding([name], &target.at)?;
                                locals.insert(name.clone());
                            } else {
                                self.expr(target, locals)?;
                            }
                        }
                    }
                    _ => {
                        self.expr(target, locals)?;
                        self.expr(value, locals)?;
                    }
                },
                Statement::If {
                    condition,
                    yes,
                    elifs,
                    no,
                } => {
                    self.expr(condition, locals)?;
                    self.block(yes, &mut locals.clone())?;
                    for elif in elifs {
                        self.expr(&mut elif.condition, locals)?;
                        self.block(&mut elif.body, &mut locals.clone())?;
                    }
                    self.block(no, &mut locals.clone())?;
                }
                Statement::While { condition, body } => {
                    self.expr(condition, locals)?;
                    self.block(body, &mut locals.clone())?;
                }
                Statement::For {
                    name,
                    iterable,
                    body,
                } => {
                    self.expr(iterable, locals)?;
                    self.fresh_capture_binding(name.iter(), &stmt.at)?;
                    let mut inner = locals.clone();
                    inner.extend(name.iter().cloned());
                    self.block(body, &mut inner)?;
                }
                Statement::Match { value, cases } => {
                    self.expr(value, locals)?;
                    for case in cases {
                        let mut inner = locals.clone();
                        case.pattern.qualifiers_mut(&mut |ty| self.ty(ty))?;
                        let bindings = case.pattern.bindings();
                        self.fresh_capture_binding(bindings.iter().copied(), &stmt.at)?;
                        inner.extend(bindings.into_iter().cloned());
                        self.block(&mut case.body, &mut inner)?;
                    }
                }
                Statement::Pass | Statement::Break | Statement::Continue => {}
            }
        }
        Ok(())
    }
    fn expr(&self, e: &mut Expr, locals: &HashSet<String>) -> Result<()> {
        fn root(e: &Expr) -> Option<&str> {
            match &e.kind {
                Expression::Name(n)
                | Expression::Call(n, _)
                | Expression::GenericCall(n, _, _)
                | Expression::GenericValue(n, _) => n.split('.').next(),
                Expression::Member(base, _)
                | Expression::Method(base, _, _)
                | Expression::GenericMethod(base, _, _, _) => root(base),
                _ => None,
            }
        }
        /// The name an index expression starts from.
        fn leading(e: &Expr) -> Option<&str> {
            match &e.kind {
                Expression::Index(base, _)
                | Expression::Group(base)
                | Expression::Unary(_, base) => leading(base),
                Expression::Ambiguous { index, .. } => leading(index),
                _ => root(e),
            }
        }
        if matches!(e.kind, Expression::Ambiguous { .. }) {
            let Expression::Ambiguous { generic, index } =
                std::mem::replace(&mut e.kind, Expression::Unit)
            else {
                unreachable!()
            };
            let local = |name: &str| locals.contains(name.split('.').next().unwrap());
            let indexes = match &generic.kind {
                // Whether `name` is a field or a generic method depends on the
                // receiver's type, which is not known here. Nothing but a local
                // can be an index, so the bracket contents decide. A receiver
                // path that does not start at a local names a declaration.
                Expression::GenericMethod(base, ..) => {
                    path(base).is_none_or(|p| local(&p))
                        && matches!(&index.kind, Expression::Index(_, i) if leading(i).is_some_and(local))
                }
                _ => root(&generic).is_some_and(local),
            };
            e.kind = match generic.kind {
                _ if !indexes => generic.kind,
                Expression::GenericCall(_, _, args) | Expression::GenericMethod(_, _, _, args) => {
                    Expression::Invoke(index, args)
                }
                _ => index.kind,
            };
        }
        if let Some(name) = root(e).filter(|n| !locals.contains(*n) && self.uncaptured.contains(*n))
        {
            return Err(e.at.error(format!(
                "anonymous functions cannot capture `{name}`; pass it as a parameter"
            )));
        }
        if let Expression::Name(name) | Expression::Call(name, _) = &e.kind {
            if self.type_params.contains(name) && locals.contains(name) {
                return Err(e
                    .at
                    .error("a type parameter name cannot also name a value binding"));
            }
        }
        // Brackets on a local always index it. These had no index reading.
        if let Expression::GenericCall(name, types, _) | Expression::GenericValue(name, types) =
            &e.kind
        {
            let base = name.split('.').next().unwrap();
            if locals.contains(base) {
                return Err(match types.as_slice() {
                    [_, extra, ..] => extra.at.error("indexing takes one expression"),
                    _ => types.first().map_or(&e.at, |t| &t.at).error(format!(
                        "`{base}` is a local binding, so these brackets index it; expected an index expression, not a type"
                    )),
                });
            }
        }
        fn path(e: &Expr) -> Option<String> {
            match &e.kind {
                Expression::Name(n) => Some(n.clone()),
                Expression::Member(base, n) => Some(format!("{}.{n}", path(base)?)),
                _ => None,
            }
        }
        let candidate = match &e.kind {
            Expression::Method(base, name, _) | Expression::GenericMethod(base, name, _, _) => {
                path(base).map(|p| format!("{p}.{name}"))
            }
            Expression::Member(..) => path(e),
            _ => None,
        };
        if let Some(candidate) =
            candidate.filter(|p| !locals.contains(p.split('.').next().unwrap()))
        {
            if let Some(symbol) = self.symbol(&candidate, &e.at)? {
                if let Some(getter) = self.constants.get(&symbol) {
                    if !matches!(e.kind, Expression::Member(..)) {
                        return Err(e.at.error("a module constant is a value, not a callable"));
                    }
                    e.kind = Expression::Call(getter.clone(), vec![]);
                    return Ok(());
                }
                e.kind = match &mut e.kind {
                    Expression::Method(_, _, args) => {
                        Expression::Call(symbol, std::mem::take(args))
                    }
                    Expression::GenericMethod(_, _, types, args) => {
                        Expression::GenericCall(symbol, std::mem::take(types), std::mem::take(args))
                    }
                    _ => Expression::Name(symbol),
                };
                // The resolved name must not pass through local resolution a second time.
                if let Expression::GenericCall(_, types, _) = &mut e.kind {
                    for ty in types {
                        self.ty(ty)?;
                    }
                }
                if let Expression::Call(_, args) | Expression::GenericCall(_, _, args) = &mut e.kind
                {
                    for arg in args {
                        self.expr(arg, locals)?;
                    }
                }
                canonical_facility_expression(e);
                return Ok(());
            }
        }
        match &mut e.kind {
            Expression::Name(n)
            | Expression::Call(n, _)
            | Expression::GenericCall(n, _, _)
            | Expression::GenericValue(n, _)
                if !locals.contains(n) && !self.type_params.contains(n) =>
            {
                if let Some(symbol) = self.symbol(n, &e.at)? {
                    *n = symbol;
                } else if n != "replace"
                    && !matches!(n.as_str(), "enumerate" | "zip" | "reversed" | "any" | "all")
                    && !builtin(n)
                    && !self.namespace_root(n)
                {
                    *n = qualified(&self.module, n);
                }
            }
            _ => {}
        }
        if let Expression::Name(n)
        | Expression::Call(n, _)
        | Expression::GenericCall(n, _, _)
        | Expression::GenericValue(n, _) = &e.kind
        {
            if !locals.contains(n) && !self.type_params.contains(n) {
                if let Some(getter) = self.constants.get(n) {
                    if !matches!(e.kind, Expression::Name(_)) {
                        return Err(e.at.error("a module constant is a value, not a callable"));
                    }
                    e.kind = Expression::Call(getter.clone(), vec![]);
                    return Ok(());
                }
            }
        }
        match &mut e.kind {
            Expression::GenericMethod(base, _, types, args) => {
                self.expr(base, locals)?;
                for ty in types {
                    self.ty(ty)?;
                }
                for arg in args {
                    self.expr(arg, locals)?;
                }
            }
            Expression::Anonymous(function) => {
                for capture in &function.captures {
                    if !locals.contains(&capture.name) {
                        return Err(e.at.error(format!(
                            "capture requires a local binding `{}`",
                            capture.name
                        )));
                    }
                }
                let mut scope = self.clone();
                scope.uncaptured.extend(locals.iter().cloned());
                scope.function(function)?;
            }
            Expression::GenericValue(_, types) => {
                for ty in types {
                    self.ty(ty)?;
                }
            }
            Expression::GenericCall(_, types, args) => {
                for ty in types {
                    self.ty(ty)?;
                }
                for arg in args {
                    self.expr(arg, locals)?;
                }
            }
            Expression::Call(_, args) | Expression::Tuple(args) => {
                for arg in args {
                    self.expr(arg, locals)?;
                }
            }
            Expression::Member(base, _)
            | Expression::Group(base)
            | Expression::Unary(_, base)
            | Expression::Try(base) => self.expr(base, locals)?,
            Expression::Method(base, _, args) | Expression::Invoke(base, args) => {
                self.expr(base, locals)?;
                for arg in args {
                    self.expr(arg, locals)?;
                }
            }
            Expression::Index(a, b) | Expression::Binary(_, a, b) => {
                self.expr(a, locals)?;
                self.expr(b, locals)?;
            }
            Expression::Conditional { condition, yes, no } => {
                self.expr(condition, locals)?;
                self.expr(yes, locals)?;
                self.expr(no, locals)?;
            }
            Expression::Type(t) => self.ty(t)?,
            Expression::Constructor(t, args) => {
                self.ty(t)?;
                for arg in args {
                    self.expr(arg, locals)?;
                }
            }
            Expression::Collection {
                entries, clauses, ..
            } => {
                let mut inner = locals.clone();
                for clause in clauses {
                    match clause {
                        Clause::For(n, e) => {
                            self.fresh_capture_binding(n.iter(), &e.at)?;
                            self.expr(e, &inner)?;
                            inner.extend(n.iter().cloned());
                        }
                        Clause::If(e) => self.expr(e, &inner)?,
                    }
                }
                for (a, b) in entries {
                    self.expr(a, &inner)?;
                    if let Some(b) = b {
                        self.expr(b, &inner)?;
                    }
                }
            }
            Expression::Ambiguous { .. } => unreachable!("resolved on entry"),
            Expression::ClassNew(..)
            | Expression::ClassReady(..)
            | Expression::Number(_)
            | Expression::Text(_)
            | Expression::Bool(_)
            | Expression::Unit
            | Expression::Name(_) => {}
        }
        canonical_facility_expression(e);
        Ok(())
    }
}

fn canonical_facility_expression(e: &mut Expr) {
    // The parser recognizes the unqualified `channel[T](...)` spelling before
    // imports are known. Give imported spellings the exact same AST afterwards.
    if let Expression::GenericCall(name, types, args) = &mut e.kind {
        if name == "channel" {
            e.kind = Expression::Constructor(
                Box::new(TypeRef {
                    at: e.at.clone(),
                    concrete: None,
                    name: Some(name.clone()),
                    args: std::mem::take(types),
                }),
                std::mem::take(args),
            );
        }
    }
    if let Expression::GenericValue(name, types) = &mut e.kind {
        if matches!(
            name.as_str(),
            "SpawnError"
                | "SendError"
                | "SendTimeoutError"
                | "Selected"
                | "SubmitError"
                | "ParallelError"
                | "Sender"
                | "Receiver"
                | "Future"
        ) {
            e.kind = Expression::Type(Box::new(TypeRef {
                at: e.at.clone(),
                concrete: None,
                name: Some(name.clone()),
                args: std::mem::take(types),
            }));
        }
    }
}

fn resolve(
    mut modules: Vec<Module>,
    dependencies: Vec<Vec<usize>>,
    at: Token,
    require_main: bool,
) -> Result<Resolved> {
    let constants: Arc<HashMap<String, String>> = Arc::new(
        modules
            .iter()
            .flat_map(|m| {
                m.constants.iter().map(|name| {
                    let symbol = qualified(&m.name, name);
                    (symbol.clone(), format!("$const.{symbol}"))
                })
            })
            .collect(),
    );
    let exports: Vec<Arc<HashMap<String, String>>> = modules
        .iter()
        .map(|m| {
            Arc::new(
                m.exports
                    .iter()
                    .map(|n| (n.clone(), qualified(&m.name, n)))
                    .chain(
                        m.facility_exports
                            .iter()
                            .map(|n| (n.to_string(), n.to_string())),
                    )
                    .collect(),
            )
        })
        .collect();
    let mut result = Resolved {
        source_paths: vec![],
        source_root: None,
        protocols: vec![],
        functions: vec![],
        declarations: vec![],
        enums: vec![],
        classes: vec![],
        access: AccessMap::default(),
        public_api: vec![],
        at,
        require_main,
    };
    for (i, m) in modules.iter_mut().enumerate() {
        let mut scope = Scope {
            captures: HashSet::new(),
            uncaptured: HashSet::new(),
            type_params: HashSet::new(),
            module: m.name.clone(),
            declared: Arc::new(m.names().cloned().collect()),
            imported: Arc::default(),
            namespaces: Arc::default(),
            constants: constants.clone(),
        };
        for (import, dependency) in m.imports.iter().zip(&dependencies[i]) {
            let root = import.binding.split('.').next().unwrap();
            let identical_builtin = import.item.as_ref().is_some_and(|item| {
                exports[*dependency]
                    .get(item)
                    .is_some_and(|symbol| symbol == root)
            });
            if (builtin(root) && !identical_builtin)
                || scope.declared.contains(root)
                || scope.imported.contains_key(root)
                || scope.namespaces.contains_key(&import.binding)
                || scope.namespaces.contains_key(root)
                || (scope.namespace_root(root)
                    && (import.item.is_some() || !import.binding.contains('.')))
            {
                return Err(import.at.error(format!(
                    "import binding `{}` conflicts with an existing name",
                    import.binding
                )));
            }
            if let Some(item) = &import.item {
                let name = exports[*dependency].get(item).ok_or_else(|| {
                    import.at.error(format!(
                        "`{}.{item}` is private or is not a declared export",
                        import.module
                    ))
                })?;
                Arc::make_mut(&mut scope.imported).insert(import.binding.clone(), name.clone());
            } else {
                Arc::make_mut(&mut scope.namespaces)
                    .insert(import.binding.clone(), exports[*dependency].clone());
            }
        }
        for f in &mut m.functions {
            let public = m.exports.contains(&f.name);
            scope.function(f)?;
            if public {
                for t in f
                    .inputs
                    .iter()
                    .map(|(_, t)| t)
                    .chain(std::iter::once(&f.output))
                {
                    data_public_types(t, &f.type_params, &mut result.public_api);
                }
                for bound in f.type_params.iter().filter_map(|(_, b)| b.as_ref()) {
                    if matches!(bound.name.as_deref(), Some("Callable" | "OnceCallable")) {
                        for part in &bound.args {
                            data_public_types(part, &f.type_params, &mut result.public_api);
                        }
                    } else {
                        data_public_types(bound, &f.type_params, &mut result.public_api);
                    }
                }
            }
            qualify(&m.name, &mut f.name);
            if let Some(constant) = constants.get(&f.name) {
                f.name.clone_from(constant);
            }
        }
        for p in &mut m.protocols {
            let public = m.exports.contains(&p.name);
            p.name = qualified(&m.name, &p.name);
            let mut scope = scope.clone();
            scope
                .type_params
                .extend(p.type_params.iter().map(|(n, _)| n.clone()));
            let mut parameters = p.type_params.clone();
            parameters.push((p.name.clone(), None));
            result.access.protocols.insert(p.name.clone());
            result.access.types.insert(
                p.name.clone(),
                Access {
                    owner: p.at.source.clone(),
                    public,
                },
            );
            for method in &mut p.methods {
                scope.function(method)?;
                if public {
                    for t in method
                        .inputs
                        .iter()
                        .map(|(_, t)| t)
                        .chain(std::iter::once(&method.output))
                    {
                        data_public_types(t, &parameters, &mut result.public_api);
                    }
                }
            }
        }
        for a in &mut m.declarations {
            if let Some(Ty::ForeignPtr(name)) = &a.target.concrete {
                result.access.types.insert(
                    name.to_string(),
                    Access {
                        owner: a.at.source.clone(),
                        public: m.exports.contains(&a.name),
                    },
                );
            }
            scope.ty(&mut a.target)?;
            if m.exports.contains(&a.name) {
                result.public_api.push(a.target.clone());
            }
            a.name = qualified(&m.name, &a.name);
        }
        for e in &mut m.enums {
            let public = m.exports.contains(&e.name);
            e.name = qualified(&m.name, &e.name);
            result.access.types.insert(
                e.name.clone(),
                Access {
                    owner: e.at.source.clone(),
                    public,
                },
            );
            let mut scope = scope.clone();
            scope
                .type_params
                .extend(e.type_params.iter().map(|(n, _)| n.clone()));
            for bound in e.type_params.iter_mut().filter_map(|(_, b)| b.as_mut()) {
                scope.ty(bound)?;
            }
            if public {
                for bound in e.type_params.iter().filter_map(|(_, b)| b.as_ref()) {
                    for part in &bound.args {
                        data_public_types(part, &e.type_params, &mut result.public_api);
                    }
                }
            }
            for (_, fields) in &mut e.variants {
                for t in fields {
                    scope.ty(t)?;
                    if public {
                        data_public_types(t, &e.type_params, &mut result.public_api);
                    }
                }
            }
        }
        for c in &mut m.classes {
            let public = m.exports.contains(&c.name);
            c.name = qualified(&m.name, &c.name);
            let mut scope = scope.clone();
            scope
                .type_params
                .extend(c.type_params.iter().map(|(n, _)| n.clone()));
            for bound in c.type_params.iter_mut().filter_map(|(_, b)| b.as_mut()) {
                scope.ty(bound)?;
            }
            if public {
                for bound in c.type_params.iter().filter_map(|(_, b)| b.as_ref()) {
                    for part in &bound.args {
                        data_public_types(part, &c.type_params, &mut result.public_api);
                    }
                }
            }
            result.access.types.insert(
                c.name.clone(),
                Access {
                    owner: c.at.source.clone(),
                    public,
                },
            );
            let constructor = if c.methods.iter().any(|f| f.name == "__init__") {
                c.public_members.contains("__init__")
            } else {
                c.fields.iter().all(|(n, _)| c.public_members.contains(n))
            };
            result.access.members.insert(
                (c.name.clone(), "__new__".into()),
                Access {
                    owner: c.at.source.clone(),
                    public: public && constructor,
                },
            );
            for (n, t) in &mut c.fields {
                scope.ty(t)?;
                let exposed = public && c.public_members.contains(n);
                result.access.members.insert(
                    (c.name.clone(), n.clone()),
                    Access {
                        owner: c.at.source.clone(),
                        public: exposed,
                    },
                );
                if exposed {
                    data_public_types(t, &c.type_params, &mut result.public_api);
                }
            }
            for f in &mut c.methods {
                scope.function(f)?;
                let exposed = public && c.public_members.contains(&f.name);
                result.access.members.insert(
                    (c.name.clone(), f.name.clone()),
                    Access {
                        owner: c.at.source.clone(),
                        public: exposed,
                    },
                );
                if exposed {
                    let params: Vec<_> = c
                        .type_params
                        .iter()
                        .chain(&f.type_params)
                        .cloned()
                        .collect();
                    for t in f
                        .inputs
                        .iter()
                        .skip(1)
                        .map(|(_, t)| t)
                        .chain(std::iter::once(&f.output))
                    {
                        data_public_types(t, &params, &mut result.public_api);
                    }
                    for bound in f.type_params.iter().filter_map(|(_, b)| b.as_ref()) {
                        if matches!(bound.name.as_deref(), Some("Callable" | "OnceCallable")) {
                            for part in &bound.args {
                                data_public_types(part, &params, &mut result.public_api);
                            }
                        } else {
                            data_public_types(bound, &params, &mut result.public_api);
                        }
                    }
                }
            }
        }
        result.functions.append(&mut m.functions);
        result.declarations.append(&mut m.declarations);
        result.enums.append(&mut m.enums);
        result.classes.append(&mut m.classes);
        result.protocols.append(&mut m.protocols);
    }
    Ok(result)
}

pub(super) fn check_member(
    access: &AccessMap,
    class: &str,
    member: &str,
    at: &Token,
) -> Result<()> {
    let class = class.split('[').next().unwrap();
    if let Some(rule) = access.members.get(&(class.into(), member.into())) {
        if !rule.public && rule.owner != at.source {
            return Err(at.error(format!(
                "`{class}.{member}` is private to its defining module"
            )));
        }
    }
    Ok(())
}

fn data_public_types(t: &TypeRef, params: &[(String, Option<TypeRef>)], out: &mut Vec<TypeRef>) {
    fn mentions(t: &TypeRef, params: &[(String, Option<TypeRef>)]) -> bool {
        params.iter().any(|(n, _)| t.name.as_ref() == Some(n))
            || t.args.iter().any(|t| mentions(t, params))
    }
    if params.iter().any(|(n, _)| t.name.as_ref() == Some(n)) {
        return;
    }
    if !mentions(t, params) {
        out.push(t.clone());
    } else {
        if t.name
            .as_ref()
            .is_some_and(|n| !builtin(n) && !matches!(n.as_str(), "&" | "&mut"))
        {
            let mut head = t.clone();
            head.args.clear();
            out.push(head);
        }
        for arg in &t.args {
            data_public_types(arg, params, out);
        }
    }
}

pub(super) fn check_api(refs: &[TypeRef], aliases: &TypeAliases, access: &AccessMap) -> Result<()> {
    fn visible(ty: &Ty, at: &Token, access: &AccessMap, aliases: &TypeAliases) -> Result<()> {
        if let Some((name, arguments)) = aliases.data.arguments(ty) {
            if access.types.get(&name).is_some_and(|rule| !rule.public) {
                return Err(at.error(format!("public signature exposes private type `{name}`")));
            }
            for argument in &arguments {
                visible(argument, at, access, aliases)?;
            }
            return Ok(());
        }
        let nominal = match ty {
            Ty::Class(c) => Some(c.name.as_str()),
            Ty::Enum(e) => Some(e.name.as_str()),
            Ty::ForeignPtr(name) => Some(name.as_ref()),
            _ => None,
        };
        if let Some(name) = nominal {
            if let Some(rule) = access.types.get(name) {
                if !rule.public {
                    return Err(at.error(format!("public signature exposes private type `{name}`")));
                }
                return Ok(());
            }
        }
        match ty {
            Ty::Closure(t) => {
                for ty in t.signature.inputs.iter().chain(t.signature.output.iter()) {
                    visible(ty, at, access, aliases)?;
                }
            }
            Ty::Callable(sig) | Ty::BoxedCallable(sig) => {
                for ty in sig.inputs.iter().chain(sig.output.iter()) {
                    visible(ty, at, access, aliases)?;
                }
            }
            Ty::List(t)
            | Ty::Set(t)
            | Ty::Ref(t, _)
            | Ty::Channel(t, _)
            | Ty::Future(t)
            | Ty::Box(t) => visible(t, at, access, aliases)?,
            Ty::Generator(t) => visible(&t.element, at, access, aliases)?,
            Ty::Dict(k, v) => {
                visible(k, at, access, aliases)?;
                visible(v, at, access, aliases)?;
            }
            Ty::Enum(e) => {
                for t in e.get().variants.iter().flat_map(|v| &v.fields) {
                    visible(t, at, access, aliases)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    for t in refs {
        if let Some(name) = t
            .name
            .as_ref()
            .filter(|n| aliases.data.contains(n) && t.args.is_empty())
        {
            if access.types.get(name).is_some_and(|rule| !rule.public) {
                return Err(t
                    .at
                    .error(format!("public signature exposes private type `{name}`")));
            }
            continue;
        }
        if generics::numeric_bound(t).is_some() && t.args.is_empty() {
            continue;
        }
        if let Some(name) = t.name.as_ref().filter(|n| access.protocols.contains(*n)) {
            if !access.types[name].public {
                return Err(t.at.error(format!(
                    "public signature exposes private protocol `{name}`"
                )));
            }
            for argument in &t.args {
                if let Some(ty) = argument.resolve(aliases)? {
                    visible(&ty, &argument.at, access, aliases)?;
                }
            }
            continue;
        }
        if let Some(ty) = t.resolve(aliases)? {
            visible(&ty, &t.at, access, aliases)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a parse produced, as far as two parses of one module can differ.
    /// None when the parser asked for a token it was not holding.
    fn outcome(source: &str, lookahead: usize) -> Option<std::result::Result<Vec<String>, String>> {
        match parse_buffered(source, None, "m".into(), lookahead) {
            Ok(None) => None,
            Ok(Some((module, _))) => Some(Ok(module
                .functions
                .iter()
                .map(|f| format!("{}/{}/{}", f.name, f.inputs.len(), f.body.len()))
                .chain(module.classes.iter().map(|c| c.name.clone()))
                .chain(module.enums.iter().map(|e| e.name.clone()))
                .collect())),
            Err(e) => Some(Err(e.to_string())),
        }
    }

    /// Plenty files and the book's code fences under `dir`. Fences in other
    /// languages are kept: they are modules that fail to parse.
    fn sources(dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let extension = path.extension().and_then(|e| e.to_str());
            if path.is_dir() {
                sources(&path, out);
            } else if matches!(extension, Some("plenty" | "plentyi")) {
                out.push(std::fs::read_to_string(path).unwrap());
            } else if extension == Some("md") {
                let text = std::fs::read_to_string(path).unwrap();
                let mut fence: Option<String> = None;
                for line in text.lines() {
                    let delimiter = line.trim_start().starts_with("```");
                    match &mut fence {
                        Some(code) if delimiter => {
                            out.push(std::mem::take(code));
                            fence = None;
                        }
                        Some(code) => {
                            code.push_str(line);
                            code.push('\n');
                        }
                        None if delimiter => fence = Some(String::new()),
                        None => {}
                    }
                }
            }
        }
    }

    #[test]
    fn parsing_item_by_item_matches_parsing_with_every_token() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut modules = Vec::new();
        for dir in ["examples", "stdlib", "tests", "book/src"] {
            sources(&root.join(dir), &mut modules);
        }
        assert!(modules.len() > 500, "found {} modules", modules.len());
        for module in &modules {
            // A module cut off after a line is one that ends inside an item.
            let lines: Vec<usize> = module.match_indices('\n').map(|(i, _)| i + 1).collect();
            let step = lines.len() / 40 + 1;
            for &cut in lines.iter().step_by(step).chain([&module.len()]) {
                let source = &module[..cut];
                let whole = outcome(source, usize::MAX).expect("every token is held");
                // A module that needed more than LOOKAHEAD would be parsed
                // twice and hold all its tokens: issue #54.
                assert_eq!(outcome(source, LOOKAHEAD), Some(whole.clone()), "{source}");
                if let Some(narrow) = outcome(source, 0) {
                    assert_eq!(narrow, whole, "{source}");
                }
            }
        }
    }

    #[test]
    fn a_missing_block_is_reported_at_the_next_item() {
        let source = "def f() -> i64:\ndef g() -> i64:\n    1\n";
        assert_eq!(outcome(source, 0), None);
        let whole = outcome(source, usize::MAX).unwrap();
        assert_eq!(whole, Err("2:1: expected an indented block".into()));
        assert_eq!(outcome(source, LOOKAHEAD), Some(whole));
    }

    #[test]
    fn a_lexical_error_precedes_an_earlier_syntax_error() {
        let source = "def f( -> i64:\n    1\n\ndef g() -> i64:\n    $\n";
        let whole = outcome(source, usize::MAX).unwrap();
        assert_eq!(whole, Err("5:5: unexpected character `$`".into()));
        assert_eq!(outcome(source, LOOKAHEAD), Some(whole));
    }

    #[test]
    fn an_item_ends_at_the_left_margin_outside_brackets() {
        let source = "def f(\na: i64,\n) -> i64:\n    a + (\n1\n)\n# note\n\n    # note\ndef g() -> i64:\n    2";
        let mut lexer = Lexer::new(source);
        let mut items = Vec::new();
        while !lexer.done {
            let mut tokens = VecDeque::new();
            lexer.item(&mut tokens).unwrap();
            items.push(tokens.len());
        }
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(
            outcome(source, LOOKAHEAD),
            Some(Ok(vec!["f/1/1".into(), "g/0/1".into()]))
        );
    }
}
