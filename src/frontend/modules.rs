//! Source loading, explicit name binding, and module privacy before lowering.
//!
//! The backend still consumes one program. Qualified declaration keys retain
//! nominal identity while aliases only affect the names visible in each source.
use super::ide::Find;
use super::*;
use std::path::{Path, PathBuf};

pub(super) mod harness;
pub(crate) mod inputs;

/// Toolchain sources have stable identities and never participate in filesystem
/// search. Keeping them in the compiler also makes installation relocatable.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ModuleSource {
    File(PathBuf),
    Standard(&'static str),
}

/// Every importable standard module, with its embedded source. Concurrency
/// modules are compiler-provided and have no Plenty source. Order is the order
/// the unknown-module diagnostic lists them.
const STANDARD_MODULES: &[(&str, &str)] = &[
    (
        "std.linalg",
        include_str!("../../stdlib/std/linalg.plentyi"),
    ),
    (
        "std.dataframe",
        include_str!("../../stdlib/std/dataframe.plentyi"),
    ),
    ("std.math", include_str!("../../stdlib/std/math.plenty")),
    ("std.simd", include_str!("../../stdlib/std/simd.plenty")),
    ("std.fs", include_str!("../../stdlib/std/fs.plenty")),
    (
        "std.fs.dirs",
        include_str!("../../stdlib/std/fs/dirs.plenty"),
    ),
    (
        "std.fs.watch",
        include_str!("../../stdlib/std/fs/watch.plenty"),
    ),
    ("std.path", include_str!("../../stdlib/std/path.plenty")),
    ("std.deque", include_str!("../../stdlib/std/deque.plenty")),
    (
        "std.cache.lru",
        include_str!("../../stdlib/std/cache/lru.plenty"),
    ),
    ("std.error", include_str!("../../stdlib/std/error.plenty")),
    ("std.bytes", include_str!("../../stdlib/std/bytes.plenty")),
    ("std.time", include_str!("../../stdlib/std/time.plenty")),
    ("std.cli", include_str!("../../stdlib/std/cli.plenty")),
    (
        "std.cli.progress",
        include_str!("../../stdlib/std/cli/progress.plenty"),
    ),
    ("std.text", include_str!("../../stdlib/std/text.plenty")),
    (
        "std.template",
        include_str!("../../stdlib/std/template.plenty"),
    ),
    ("std.diff", include_str!("../../stdlib/std/diff.plenty")),
    ("std.fmt", include_str!("../../stdlib/std/fmt.plenty")),
    ("std.report", include_str!("../../stdlib/std/report.plenty")),
    (
        "std.unicode",
        include_str!("../../stdlib/std/unicode.plenty"),
    ),
    ("std.thread", ""),
    ("std.channel", ""),
    ("std.concurrent", ""),
    ("std.sync", include_str!("../../stdlib/std/sync.plenty")),
    ("std.atomic", include_str!("../../stdlib/std/atomic.plenty")),
    ("std.io", include_str!("../../stdlib/std/io.plenty")),
    (
        "std.io.typed",
        include_str!("../../stdlib/std/io/typed.plenty"),
    ),
    (
        "std.encoding",
        include_str!("../../stdlib/std/encoding.plenty"),
    ),
    (
        "std.collections",
        include_str!("../../stdlib/std/collections.plenty"),
    ),
    ("std.url", include_str!("../../stdlib/std/url.plenty")),
    ("std.uuid", include_str!("../../stdlib/std/uuid.plenty")),
    ("std.sketch", include_str!("../../stdlib/std/sketch.plenty")),
    (
        "std.sketch.xor",
        include_str!("../../stdlib/std/sketch/xor.plenty"),
    ),
    (
        "std.sketch.cuckoo",
        include_str!("../../stdlib/std/sketch/cuckoo.plenty"),
    ),
    ("std.csv", include_str!("../../stdlib/std/csv.plenty")),
    ("std.random", include_str!("../../stdlib/std/random.plenty")),
    ("std.stat", include_str!("../../stdlib/std/stat.plenty")),
    ("std.graph", include_str!("../../stdlib/std/graph.plenty")),
    (
        "std.secrets",
        include_str!("../../stdlib/std/secrets.plenty"),
    ),
    ("std.crypto", include_str!("../../stdlib/std/crypto.plenty")),
    (
        "std.compress",
        include_str!("../../stdlib/std/compress.plenty"),
    ),
    (
        "std.archive",
        include_str!("../../stdlib/std/archive.plenty"),
    ),
    ("std.regex", include_str!("../../stdlib/std/regex.plenty")),
    ("std.json", include_str!("../../stdlib/std/json.plenty")),
    ("std.log", include_str!("../../stdlib/std/log.plenty")),
    ("std.toml", include_str!("../../stdlib/std/toml.plenty")),
    (
        "std.markdown",
        include_str!("../../stdlib/std/markdown.plenty"),
    ),
    ("std.env", include_str!("../../stdlib/std/env.plenty")),
    (
        "std.process",
        include_str!("../../stdlib/std/process.plenty"),
    ),
    ("std.signal", include_str!("../../stdlib/std/signal.plenty")),
    ("std.shlex", include_str!("../../stdlib/std/shlex.plenty")),
    ("std.system", include_str!("../../stdlib/std/system.plenty")),
    ("std.ready", include_str!("../../stdlib/std/ready.plenty")),
    ("std.net", include_str!("../../stdlib/std/net.plenty")),
    ("std.tls", include_str!("../../stdlib/std/tls.plenty")),
    ("std.http", include_str!("../../stdlib/std/http.plenty")),
    (
        "std.http.server",
        include_str!("../../stdlib/std/http/server.plenty"),
    ),
    (
        "std.http.tls",
        include_str!("../../stdlib/std/http/tls.plenty"),
    ),
    (
        "std.http.body",
        include_str!("../../stdlib/std/http/body.plenty"),
    ),
    (
        "std.http.typed",
        include_str!("../../stdlib/std/http/typed.plenty"),
    ),
    (
        "std.http.cookie",
        include_str!("../../stdlib/std/http/cookie.plenty"),
    ),
    (
        "std.http.proxy",
        include_str!("../../stdlib/std/http/proxy.plenty"),
    ),
    (
        "std.http.files",
        include_str!("../../stdlib/std/http/files.plenty"),
    ),
    (
        "std.http.client",
        include_str!("../../stdlib/std/http/client.plenty"),
    ),
    (
        "std.http.https",
        include_str!("../../stdlib/std/http/https.plenty"),
    ),
    (
        "std.http.websocket",
        include_str!("../../stdlib/std/http/websocket.plenty"),
    ),
    (
        "std.http.sse",
        include_str!("../../stdlib/std/http/sse.plenty"),
    ),
    (
        "std.http.wss",
        include_str!("../../stdlib/std/http/wss.plenty"),
    ),
    (
        "std.db.sqlite",
        include_str!("../../stdlib/std/db/sqlite.plenty"),
    ),
    ("std.db.kv", include_str!("../../stdlib/std/db/kv.plenty")),
    ("std.test", include_str!("../../stdlib/std/test.plenty")),
    (
        "std.tempfile",
        include_str!("../../stdlib/std/tempfile.plenty"),
    ),
];

impl ModuleSource {
    fn path(&self) -> PathBuf {
        match self {
            Self::File(path) => path.clone(),
            Self::Standard(name) => {
                // Engine facades declare the engine's C symbols, which only
                // a trusted interface module may do.
                let extension = if crate::toolchain::ENGINES
                    .iter()
                    .any(|engine| engine.module == *name)
                {
                    "plentyi"
                } else {
                    "plenty"
                };
                PathBuf::from(format!("{}.{extension}", name.replace('.', "/")))
            }
        }
    }

    fn read(&self) -> Result<String> {
        match self {
            Self::Standard(name) => Ok(STANDARD_MODULES
                .iter()
                .find(|(standard, _)| standard == name)
                .map(|(_, text)| *text)
                .expect("validated standard module")
                .into()),
            Self::File(path) if super::ide::active() => match super::ide::overlay(path) {
                Some(text) => Ok(text),
                None => std::fs::read_to_string(path)
                    .map_err(|e| format!("reading {}: {e}", path.display()).into()),
            },
            Self::File(path) => std::fs::read_to_string(path)
                .map_err(|e| format!("reading {}: {e}", path.display()).into()),
        }
    }
}

/// Every standard module's name, in diagnostic order, with the unqualified
/// names the compiler provides for it beside its Plenty source.
pub(super) fn standard_modules() -> Vec<(&'static str, Vec<&'static str>)> {
    STANDARD_MODULES
        .iter()
        .map(|&(name, _)| {
            let provided = facility_exports(name)
                .iter()
                .map(|export| export.rsplit('.').next().unwrap_or(export))
                .collect();
            (name, provided)
        })
        .collect()
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
        // Qualified so that the names resolve only through an import.
        "std.atomic" => &[crate::atomic::ATOMIC, crate::atomic::FENCE],
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
    /// `include_str`/`include_bytes` constants whose file the loader has not
    /// read yet.
    embeds: Vec<Embed>,
}

/// A constant initialized by `include_str("path")` or `include_bytes("path")`.
struct Embed {
    at: Token,
    /// Index of the constant's getter in `Module::functions`, set when the
    /// getter is added.
    function: usize,
    path: String,
    text: bool,
}

pub(super) struct Access {
    owner: Option<Arc<str>>,
    public: bool,
}

#[derive(Default)]
pub(super) struct AccessMap {
    members: HashMap<(String, String), Access>,
    types: HashMap<String, Access>,
    enums: HashSet<String>,
    protocols: HashSet<String>,
}

pub(super) struct Resolved {
    pub(super) standard_sources: HashSet<Arc<str>>,
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
    /// What the program depends on outside the compiler, for the program
    /// cache; empty for a single source.
    pub(super) inputs: inputs::Inputs,
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
    /// Constants use the ordinary typed function lowering, with a
    /// compiler-only getter name. Initializers cannot execute user code:
    /// `consts::fold` replaces each with its literal value after name
    /// resolution. An `include_str`/`include_bytes` initializer is returned
    /// separately, for the loader to read relative to the source file.
    fn constant(&mut self) -> Result<(Function, Option<Embed>)> {
        let at = self.take(); // const
        let name = self.name()?;
        self.expect(":")?;
        let output = self.ty()?;
        let type_name = output.name.as_deref().unwrap_or_default();
        if !output.args.is_empty()
            || !(numeric_type_name(type_name) || matches!(type_name, "bool" | "str" | "bytes"))
        {
            return Err(output.at.error(
                "module constants require a concrete numeric type, `bool`, `str`, or `bytes`",
            ));
        }
        self.expect("=")?;
        let value = self.expr(0)?;
        let mut embed = None;
        if let Expression::Call(function, args) = &value.kind
            && let Some(text) = match function.as_str() {
                "include_str" => Some(true),
                "include_bytes" => Some(false),
                _ => None,
            }
        {
            let [argument] = args.as_slice() else {
                return Err(value
                    .at
                    .error(format!("`{function}` takes one string literal path")));
            };
            let Expression::Text(path) = &argument.kind else {
                return Err(argument
                    .at
                    .error(format!("`{function}` takes one string literal path")));
            };
            let wanted = if text { "str" } else { "bytes" };
            if type_name != wanted {
                return Err(output
                    .at
                    .error(format!("`{function}` initializes a `{wanted}` constant")));
            }
            embed = Some(Embed {
                at: value.at.clone(),
                function: 0,
                path: path.clone(),
                text,
            });
        }
        self.kind(Kind::Newline, "the end of the constant declaration")?;
        let function = self.constant_getter(at, name, output, vec![value]);
        Ok((function, embed))
    }

    /// `assert CONDITION` or `assert CONDITION, MESSAGE` at module scope: a
    /// getter-shaped declaration that `consts::fold` evaluates and removes.
    fn static_assert(&mut self, index: usize) -> Result<Function> {
        let at = self.take(); // assert
        let mut body = vec![self.expr(0)?];
        if self.eat(",") {
            body.push(self.expr(0)?);
        }
        self.kind(Kind::Newline, "the end of the assertion")?;
        let output = TypeRef {
            at: at.clone(),
            name: None,
            args: vec![],
            concrete: Some(Ty::Bool),
        };
        Ok(self.constant_getter(
            at,
            format!("{}{index}", super::consts::ASSERT),
            output,
            body,
        ))
    }

    fn constant_getter(
        &self,
        at: Token,
        name: String,
        output: TypeRef,
        values: Vec<Expr>,
    ) -> Function {
        Function {
            once: false,
            captures: vec![],
            foreign: None,
            export: None,
            name,
            type_params: vec![],
            at: at.clone(),
            inputs: vec![],
            defaults: vec![],
            output,
            doc: String::new(),
            doc_at: None,
            body: values
                .into_iter()
                .map(|value| Stmt {
                    at: at.clone(),
                    kind: Statement::Expr(value),
                })
                .collect(),
        }
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

#[cfg_attr(feature = "profile", hotpath::measure)]
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
    let mut parser =
        Parser::new(path.map(|p| FileId::intern(&p.to_string_lossy().as_ref().into())));
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
    let mut declared = HashSet::default();
    loop {
        parser.fill(lexer, lookahead).map_err(located)?;
        if parser.peek().kind == Kind::Eof {
            break;
        }
        let mut items = Items {
            parser: &mut *parser,
            lexer: &mut *lexer,
            path,
            lookahead,
            located,
        };
        items.item(&mut module, &mut declared)?;
    }
    if parser.formats {
        module.imports.push(Import {
            at: parser.peek().clone(),
            module: "std.fmt".into(),
            item: None,
            binding: fstrings::FMT_BINDING.into(),
        });
    }
    Ok((module, parser.peek().clone()))
}

/// Parsing state for module items, shared with the items of `if const`
/// branches.
struct Items<'a, 's> {
    parser: &'a mut Parser,
    lexer: &'a mut Lexer<'s>,
    path: Option<&'a Path>,
    lookahead: usize,
    located: &'a dyn Fn(Box<dyn Error>) -> Box<dyn Error>,
}

impl Items<'_, '_> {
    fn item(&mut self, module: &mut Module, declared: &mut HashSet<String>) -> Result<()> {
        if self.parser.at_const_if() {
            self.parser.depth += 1;
            let result = if self.parser.depth > MAX_NESTING {
                Err(self.parser.peek().error(NESTING))
            } else {
                self.const_items(module, declared)
            };
            self.parser.depth -= 1;
            return result;
        }
        if self.parser.peek().is("assert") {
            let f = self.parser.static_assert(module.functions.len())?;
            module.functions.push(f);
            return Ok(());
        }
        let public = self.parser.eat("pub");
        let at = self.parser.peek().clone();
        let name = if self.parser.peek().is("extern") || self.parser.peek().is("opaque") {
            if !self
                .path
                .is_some_and(|p| p.extension().is_some_and(|e| e == "plentyi"))
            {
                return Err(at.error("C declarations require a trusted .plentyi interface module"));
            }
            if self.parser.eat("extern") {
                self.parser.expect("def")?;
                self.parser.pos -= 1;
                let f = self.parser.function_header(None, true, false, false)?;
                let name = f.name.clone();
                module.functions.push(f);
                name
            } else {
                self.parser.expect("opaque")?;
                let name = self.parser.name()?;
                self.parser
                    .kind(Kind::Newline, "the end of the opaque pointer declaration")?;
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
        } else if self.parser.eat("export") {
            self.parser.expect("def")?;
            self.parser.pos -= 1;
            let f = self.parser.function_header(None, false, true, false)?;
            let name = f.name.clone();
            module.functions.push(f);
            name
        } else if self.parser.peek().is("def") {
            let f = self.parser.function()?;
            let name = f.name.clone();
            module.functions.push(f);
            name
        } else if self.parser.peek().is("const") {
            let (f, embed) = self.parser.constant()?;
            let name = f.name.clone();
            if let Some(embed) = embed {
                module.embeds.push(Embed {
                    function: module.functions.len(),
                    ..embed
                });
            }
            module.constants.insert(name.clone());
            module.functions.push(f);
            name
        } else if self.parser.peek().is("class") {
            let c = self.parser.class_decl()?;
            let name = c.name.clone();
            module.classes.push(c);
            name
        } else if self.parser.peek().is("protocol") {
            let p = self.parser.protocol_decl()?;
            let name = p.name.clone();
            module.protocols.push(p);
            name
        } else if self.parser.peek().is("enum") {
            let e = self.parser.enum_decl()?;
            let name = e.name.clone();
            module.enums.push(e);
            name
        } else if self.parser.peek().is("type") {
            let a = self.parser.alias()?;
            let name = a.name.clone();
            module.declarations.push(a);
            name
        } else if !public && (self.parser.peek().is("import") || self.parser.peek().is("from")) {
            let imports = self.parser.imports()?;
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
            return Ok(());
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
        Ok(())
    }

    /// An `if const` chain at module scope. The selected branch's items
    /// join the module; the others are parsed into a discarded module, so
    /// they must be well formed but are never resolved or checked.
    fn const_items(&mut self, module: &mut Module, declared: &mut HashSet<String>) -> Result<()> {
        let mut first = true;
        let mut taken = false;
        loop {
            let (holds, else_arm) = self.parser.const_arm(first)?;
            first = false;
            self.parser.expect(":")?;
            self.parser.kind(Kind::Newline, "a newline after `:`")?;
            self.parser.kind(Kind::Indent, "an indented block")?;
            let selected = holds && !taken;
            taken |= selected;
            let mut discarded = Module {
                name: module.name.clone(),
                ..Module::default()
            };
            let mut discarded_names = HashSet::default();
            while !matches!(self.parser.peek().kind, Kind::Dedent | Kind::Eof) {
                if self.parser.eat("pass") {
                    self.parser.kind(Kind::Newline, "the end of `pass`")?;
                } else if selected {
                    self.item(module, declared)?;
                } else {
                    self.item(&mut discarded, &mut discarded_names)?;
                }
            }
            self.parser
                .kind(Kind::Dedent, "the end of an indented block")?;
            self.parser
                .fill(self.lexer, self.lookahead)
                .map_err(self.located)?;
            if else_arm || !(self.parser.peek().is("elif") || self.parser.peek().is("else")) {
                return Ok(());
            }
        }
    }
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

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(super) fn single(source: &str) -> Result<Resolved> {
    let (module, at) = parse(source, None, String::new())?;
    // Only the standard library is reachable without a source root.
    if let Some(import) = module
        .imports
        .iter()
        .find(|import| import.module.split('.').next() != Some("std"))
    {
        return Err(import.at.error(format!(
            "cannot import `{}` without a source root; source-only compilation can import only `std` modules, so compile a file to import project modules",
            import.module
        )));
    }
    let mut loader = Loader {
        packages: vec![],
        active: vec![],
        loaded: HashMap::default(),
        modules: vec![],
        dependencies: vec![],
        appendix: None,
        inputs: vec![],
    };
    let mut entry = vec![];
    for import in &module.imports {
        let (dependency, _) = loader.import_path(import, 0)?;
        let index = match loader.loaded.get(&dependency) {
            Some(&index) => index,
            None => {
                let ModuleSource::Standard(name) = dependency else {
                    unreachable!("non-std imports were rejected above")
                };
                loader
                    .file(dependency, 0, name.into())
                    .map_err(|e| import.at.error(e))?
                    .0
            }
        };
        entry.push(index);
    }
    let standard_sources = loader
        .loaded
        .keys()
        .map(|source| source.path().to_string_lossy().into_owned().into())
        .collect();
    let mut modules = loader.modules;
    let mut dependencies = loader.dependencies;
    modules.push(module);
    dependencies.push(entry);
    let mut result = resolve(modules, dependencies, at, true)?;
    result.standard_sources = standard_sources;
    Ok(result)
}

pub(super) fn generated_interface(source: &str) -> Result<Resolved> {
    let (module, at) = parse(source, Some(Path::new("generated.plentyi")), String::new())?;
    if !module.imports.is_empty() {
        return Err(at.error("generated runtime interfaces must be self-contained"));
    }
    resolve(vec![module], vec![vec![]], at, false)
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(super) fn load(path: &Path, root: Option<&Path>, require_main: bool) -> Result<Resolved> {
    let (entry, graph) = file_graph(path, root)?;
    load_graph(&entry, &graph, require_main)
}

/// The canonical entry and the one-package graph that [`load`] compiles.
pub(super) fn file_graph(
    path: &Path,
    root: Option<&Path>,
) -> Result<(PathBuf, crate::project::SourceGraph)> {
    let entry = path
        .canonicalize()
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    let root = root
        .unwrap_or_else(|| entry.parent().unwrap())
        .canonicalize()
        .map_err(|e| format!("source root: {e}"))?;
    let graph = crate::project::SourceGraph {
        packages: vec![crate::project::SourcePackage {
            identity: String::new(),
            root,
            library: None,
            dependencies: vec![],
        }],
    };
    Ok((entry, graph))
}

/// The canonical entry and packages [`load_graph`] would use: what names a
/// build of them in the program cache. `None` when loading would fail.
pub(super) fn identity(path: &Path, graph: &crate::project::SourceGraph) -> Option<String> {
    let entry = path.canonicalize().ok()?;
    let packages = prepare_packages(graph).ok()?;
    Some(format!("{entry:?}\n{packages:?}"))
}

/// A file's path below its source root as a dotted module path, if every
/// component is an identifier and the file is a `.plenty` source or interface.
fn module_path_of(relative: &Path) -> Option<String> {
    if relative
        .extension()
        .is_none_or(|e| e != "plenty" && e != "plentyi")
    {
        return None;
    }
    let names = relative
        .with_extension("")
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names
        .iter()
        .all(|name| {
            let mut chars = name.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .then(|| names.join("."))
}

struct Loader {
    packages: Vec<crate::project::SourcePackage>,
    active: Vec<ModuleSource>,
    loaded: HashMap<ModuleSource, usize>,
    modules: Vec<Module>,
    dependencies: Vec<Vec<usize>>,
    /// Taken by the first file read, which is the entry.
    appendix: Option<String>,
    /// Each source file read, with its imports' resolutions.
    inputs: Vec<inputs::File>,
}
impl Loader {
    /// Reads the files named by `include_str` and `include_bytes` in the
    /// module at `path`, making each constant's initializer the literal it
    /// denotes. Each file is a program input, so the program cache notices
    /// an edit.
    fn embed(&mut self, path: &Path, package: usize, module: &mut Module) -> Result<()> {
        for embed in std::mem::take(&mut module.embeds) {
            let relative = Path::new(&embed.path);
            if relative.is_absolute() {
                return Err(embed.at.error(format!(
                    "included path `{}` must be relative to the source file",
                    embed.path
                )));
            }
            let joined = path.parent().unwrap_or(Path::new("")).join(relative);
            let root = &self.packages[package].root;
            let file = joined
                .canonicalize()
                .map_err(|e| embed.at.error(format!("reading `{}`: {e}", embed.path)))?;
            if !file.starts_with(root) {
                return Err(embed.at.error(format!(
                    "included path `{}` is outside the source root {}",
                    embed.path,
                    root.display()
                )));
            }
            let data = std::fs::read(&file)
                .map_err(|e| embed.at.error(format!("reading `{}`: {e}", embed.path)))?;
            self.inputs.push(inputs::File::new(&joined, package, &data));
            let kind = if embed.text {
                Expression::Text(String::from_utf8(data).map_err(|e| {
                    embed.at.error(format!(
                        "`include_str` file `{}` is not valid UTF-8 at byte {}; use `include_bytes`",
                        embed.path,
                        e.utf8_error().valid_up_to()
                    ))
                })?)
            } else {
                Expression::Bytes(data)
            };
            module.functions[embed.function].body = vec![Stmt {
                at: embed.at.clone(),
                kind: Statement::Expr(Expr { at: embed.at, kind }),
            }];
        }
        Ok(())
    }

    fn file(
        &mut self,
        source: ModuleSource,
        package: usize,
        name: String,
    ) -> Result<(usize, Token)> {
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
                "module graph exceeds the implementation limit (128 depth, 4096 modules)".into(),
            );
        }
        self.active.push(source.clone());
        let mut text = source.read()?;
        let input = match &source {
            ModuleSource::File(path) => {
                self.inputs
                    .push(inputs::File::new(path, package, text.as_bytes()));
                Some(self.inputs.len() - 1)
            }
            ModuleSource::Standard(_) => None,
        };
        if let Some(appendix) = self.appendix.take() {
            text.push_str(&appendix);
        }
        super::ide::loaded(
            &source.path(),
            matches!(source, ModuleSource::Standard(_)),
            &text,
        );
        let (mut module, at) = parse(&text, Some(&source.path()), name)?;
        if let ModuleSource::Standard(name) = &source {
            module.facility_exports = facility_exports(name);
        }
        if let ModuleSource::File(path) = &source {
            self.embed(path, package, &mut module)?;
        }
        let mut deps = Vec::new();
        for import in &module.imports {
            let (dependency, owner) = self.import_path(import, package)?;
            if let Some(input) = input {
                self.inputs[input].import(&import.module, &dependency, owner);
            }
            // The implicit `std.fmt` import of an f-string has no source span.
            if super::ide::active() && import.binding != fstrings::FMT_BINDING {
                super::ide::module(
                    &import.module,
                    &dependency.path(),
                    matches!(dependency, ModuleSource::Standard(_)),
                    &import.at,
                );
            }
            let index = if let Some(index) = self.loaded.get(&dependency) {
                *index
            } else {
                // Canonical relative paths make alternate spellings refer to one module.
                let identity = match &dependency {
                    ModuleSource::Standard(name) => (*name).into(),
                    ModuleSource::File(path) => {
                        let package = &self.packages[owner];
                        let relative =
                            module_path_of(path.strip_prefix(&package.root).unwrap()).unwrap();
                        // `::` cannot occur in a source module path, so
                        // dependency modules never share a key with entry
                        // package modules.
                        if owner == 0 {
                            relative
                        } else {
                            format!("{}::{relative}", package.identity)
                        }
                    }
                };
                self.file(dependency, owner, identity)
                    .map_err(|e| import.at.error(e))?
                    .0
            };
            deps.push(index);
        }
        self.active.pop();
        if self.modules.len() >= 4096 {
            return Err(at.error("module graph exceeds the implementation limit of 4096 modules"));
        }
        let index = self.modules.len();
        self.loaded.insert(source, index);
        self.modules.push(module);
        self.dependencies.push(deps);
        Ok((index, at))
    }

    fn import_path(&self, import: &Import, package: usize) -> Result<(ModuleSource, usize)> {
        let first = import.module.split('.').next().unwrap();
        if first == "std" {
            let Some(&(name, _)) = STANDARD_MODULES
                .iter()
                .find(|(name, _)| *name == import.module)
            else {
                let available: Vec<_> = STANDARD_MODULES.iter().map(|(name, _)| *name).collect();
                return Err(import.at.error(format!(
                    "unknown standard library module `{}`; available modules: {}",
                    import.module,
                    available.join(", ")
                )));
            };
            return Ok((ModuleSource::Standard(name), package));
        }
        let parts: Vec<_> = import.module.split('.').collect();
        let current = &self.packages[package];
        if let Some(&(_, owner)) = current
            .dependencies
            .iter()
            .find(|(alias, _)| alias == first)
        {
            let dependency = &self.packages[owner];
            if parts.len() == 1 {
                let library = dependency.library.clone().ok_or_else(|| {
                    import.at.error(format!(
                        "dependency `{first}` (package `{}`) has no library target",
                        dependency.identity
                    ))
                })?;
                return Ok((ModuleSource::File(library), owner));
            }
            return Ok((
                ModuleSource::File(Self::local(&dependency.root, &parts[1..], import)?),
                owner,
            ));
        }
        Ok((
            ModuleSource::File(Self::local(&current.root, &parts, import).map_err(|e| {
                if self.packages.len() > 1 && !current.root.join(first).exists() {
                    format!(
                        "{e}\nnote: `{first}` is neither a local module nor a dependency declared by package `{}`",
                        if current.identity.is_empty() { "<entry>" } else { &current.identity }
                    )
                    .into()
                } else {
                    e
                }
            })?),
            package,
        ))
    }

    /// Resolve `parts` below `root`, confined to that root.
    fn local(root: &Path, parts: &[&str], import: &Import) -> Result<PathBuf> {
        let mut path = root.to_path_buf();
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
        if !canonical.starts_with(root) {
            return Err(import.at.error("import resolves outside the source root"));
        }
        if module_path_of(canonical.strip_prefix(root).unwrap()).is_none() {
            return Err(import.at.error(
                "canonical imported paths must be .plenty files or .plentyi interfaces with identifier components",
            ));
        }
        Ok(canonical)
    }
}

/// Load the program whose entry file belongs to `graph.packages[0]`.
///
/// Each package resolves imports against its own source root, `std`, and the
/// aliases of its direct dependencies only. Modules of packages other than the
/// entry package are qualified by the package identity, so equal module and
/// type names in unrelated packages stay distinct and an alias never becomes
/// part of a declaration's identity.
#[cfg_attr(feature = "profile", hotpath::measure)]
pub(super) fn load_graph(
    path: &Path,
    graph: &crate::project::SourceGraph,
    require_main: bool,
) -> Result<Resolved> {
    load_graph_with(path, graph, require_main, None)
}

/// [`load_graph`], with `appendix` added to the end of the entry file's text.
/// `plenty test` appends a generated `main`; the entry's own lines keep their
/// positions in diagnostics.
pub(super) fn load_graph_with(
    path: &Path,
    graph: &crate::project::SourceGraph,
    require_main: bool,
    appendix: Option<&str>,
) -> Result<Resolved> {
    let entry = path
        .canonicalize()
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    let packages = prepare_packages(graph)?;
    let root = packages[0].root.clone();
    if !entry.starts_with(&root) {
        return Err(format!(
            "entry {} must be inside source root {}",
            entry.display(),
            root.display()
        )
        .into());
    }
    let mut loader = Loader {
        packages,
        active: vec![],
        loaded: HashMap::default(),
        modules: vec![],
        dependencies: vec![],
        appendix: appendix.map(str::to_owned),
        inputs: vec![],
    };
    let (_, at) = loader.file(ModuleSource::File(entry), 0, String::new())?;
    let inputs = inputs::Inputs::new(std::mem::take(&mut loader.inputs));
    let mut result = resolve(loader.modules, loader.dependencies, at, require_main)?;
    result.inputs = inputs;
    result.standard_sources = loader
        .loaded
        .keys()
        .filter_map(|source| match source {
            ModuleSource::Standard(_) => Some(source.path().to_string_lossy().into_owned().into()),
            ModuleSource::File(_) => None,
        })
        .collect();
    result.source_paths = loader
        .loaded
        .into_keys()
        .filter_map(|source| match source {
            ModuleSource::File(path) => Some(path),
            ModuleSource::Standard(_) => None,
        })
        .collect();
    result.source_paths.sort();
    result.source_root = Some(root);
    Ok(result)
}

/// `graph`'s packages with canonical roots and library paths, after the checks
/// every load makes: reserved and colliding names, and overlapping roots.
fn prepare_packages(
    graph: &crate::project::SourceGraph,
) -> Result<Vec<crate::project::SourcePackage>> {
    let mut packages = graph.packages.clone();
    for package in &mut packages {
        package.root = package
            .root
            .canonicalize()
            .map_err(|e| format!("source root {}: {e}", package.root.display()))?;
        let root = &package.root;
        if !root.is_dir() {
            return Err(format!("source root {} is not a directory", root.display()).into());
        }
        if ["std", "std.plenty", "std.plentyi"]
            .iter()
            .any(|name| root.join(name).exists())
        {
            return Err(format!(
                "`std` is reserved for the toolchain standard library; rename the local module or namespace in {}",
                root.display()
            )
            .into());
        }
        for (alias, _) in &package.dependencies {
            if alias == "std" {
                return Err("`std` is reserved for the toolchain standard library and cannot name a dependency".into());
            }
            if let Some(local) = [
                alias.clone(),
                format!("{alias}.plenty"),
                format!("{alias}.plentyi"),
            ]
            .iter()
            .map(|name| root.join(name))
            .find(|path| path.exists())
            {
                return Err(format!(
                    "dependency `{alias}` collides with the local module {}; rename the module or the dependency",
                    local.display()
                )
                .into());
            }
        }
        if let Some(library) = &mut package.library {
            *library = library
                .canonicalize()
                .map_err(|e| format!("library {}: {e}", library.display()))?;
            if library
                .strip_prefix(root)
                .ok()
                .and_then(module_path_of)
                .is_none()
            {
                return Err(format!(
                    "library {} must be a .plenty module with identifier components inside {}",
                    library.display(),
                    root.display()
                )
                .into());
            }
        }
    }
    for (i, a) in packages.iter().enumerate() {
        for b in &packages[i + 1..] {
            if a.root.starts_with(&b.root) || b.root.starts_with(&a.root) {
                return Err(format!(
                    "package source roots {} and {} overlap",
                    a.root.display(),
                    b.root.display()
                )
                .into());
            }
        }
    }
    Ok(packages)
}

/// Local names in scope and where each was bound. The positions serve
/// editor queries; see `ide`.
type Locals = HashMap<String, Pos>;

/// Bind `names` at the statement or case `at`, after which each name is written.
fn bind(locals: &mut Locals, names: &[String], at: &Token) {
    let pos = Pos::of(at);
    for name in names {
        super::ide::bind_local(at, pos, name, false, Find::After(name.clone()));
        locals.insert(name.clone(), pos);
    }
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
    /// Type parameters in scope: a declaration has few, so a scan beats
    /// building a hash set per function (issue #436).
    type_params: Vec<String>,
}

impl Scope {
    fn type_param(&self, name: &str) -> bool {
        self.type_params.iter().any(|p| p == name)
    }
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
        if let Some((prefix, member)) = path.rsplit_once('.')
            && let Some(exports) = self.namespaces.get(prefix)
        {
            return exports.get(member).cloned().map(Some).ok_or_else(|| {
                at.error(format!("`{path}` is private or is not a declared export"))
            });
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
            if self.type_param(name) {
                return Ok(());
            }
            // Imports can bind a builtin spelling only to that same identity.
            // Avoid probing the module table for these common types (issue #57).
            if builtin(name) {
                return Ok(());
            }
            if let Some(symbol) = self.symbol(name, &ty.at)? {
                if super::ide::active() {
                    let written = name.chars().count() as u32;
                    super::ide::refer(&ty.at, Find::At(written), super::ide::global(&symbol));
                }
                *name = symbol;
            } else if !matches!(name.as_str(), "&" | "&mut" | "|") {
                return Err(ty.at.error(format!("unknown type `{name}`")));
            }
        }
        Ok(())
    }
    /// Resolves a class's bounds, fields, and methods, in that order.
    fn class(&self, c: &mut classes::ClassDecl) -> Result<()> {
        let mut scope = self.clone();
        scope
            .type_params
            .extend(c.type_params.iter().map(|(n, _)| n.clone()));
        for bound in c.type_params.iter_mut().filter_map(|(_, b)| b.as_mut()) {
            scope.ty(bound)?;
        }
        for (_, t) in &mut c.fields {
            scope.ty(t)?;
        }
        for f in &mut c.methods {
            scope.function(f)?;
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
        for default in f.defaults.iter_mut().flatten() {
            scope.expr(default, &Locals::default())?;
        }
        let mut locals = Locals::default();
        for (name, ty) in &f.inputs {
            let pos = Pos::of(&ty.at);
            super::ide::bind_local(&f.at, pos, name, true, Find::Before(name.clone()));
            locals.insert(name.clone(), pos);
        }
        for capture in &f.captures {
            let pos = Pos::of(&f.at);
            super::ide::bind_local(
                &f.at,
                pos,
                &capture.name,
                false,
                Find::After(capture.name.clone()),
            );
            locals.insert(capture.name.clone(), pos);
        }
        scope.block(&mut f.body, &mut locals)
    }
    fn block(&self, body: &mut [Stmt], locals: &mut Locals) -> Result<()> {
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
                    bind(&mut inner, name.as_slice(), &stmt.at);
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
                    match locals.get(name) {
                        Some(&pos) => super::ide::refer(
                            &stmt.at,
                            Find::After(name.clone()),
                            super::ide::local_key(&stmt.at, pos, name),
                        ),
                        None => bind(locals, std::slice::from_ref(name), &stmt.at),
                    }
                }
                Statement::Unpack { names, value, .. } => {
                    self.fresh_capture_binding(names.iter(), &stmt.at)?;
                    self.expr(value, locals)?;
                    bind(locals, names, &stmt.at);
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
                                match locals.get(name) {
                                    Some(&pos) => super::ide::refer_local(&target.at, pos, name),
                                    None => bind(locals, std::slice::from_ref(name), &target.at),
                                }
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
                    bind(&mut inner, name, &stmt.at);
                    self.block(body, &mut inner)?;
                }
                Statement::Match { value, cases } => {
                    self.match_block(value, cases, &stmt.at, locals)?
                }
                Statement::Pass | Statement::Break | Statement::Continue => {}
            }
        }
        Ok(())
    }
    fn match_block(
        &self,
        value: &mut Expr,
        cases: &mut [enums::Case],
        at: &Token,
        locals: &Locals,
    ) -> Result<()> {
        self.expr(value, locals)?;
        for case in cases {
            let mut inner = locals.clone();
            case.pattern.qualifiers_mut(&mut |ty| self.ty(ty))?;
            let bindings = case.pattern.bindings();
            self.fresh_capture_binding(bindings.iter().copied(), at)?;
            let bindings: Vec<String> = bindings.into_iter().cloned().collect();
            bind(&mut inner, &bindings, &case.at);
            self.block(&mut case.body, &mut inner)?;
        }
        Ok(())
    }
    fn expr(&self, e: &mut Expr, locals: &Locals) -> Result<()> {
        fn root(e: &Expr) -> Option<&str> {
            match &e.kind {
                Expression::Name(n)
                | Expression::Call(n, _)
                | Expression::GenericCall(n, _, _)
                | Expression::GenericValue(n, _) => n.split('.').next(),
                Expression::Member(base, _, _)
                | Expression::Method(base, _, _, _)
                | Expression::GenericMethod(base, _, _, _, _) => root(base),
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
            let local = |name: &str| locals.contains_key(name.split('.').next().unwrap());
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
                Expression::GenericCall(_, _, args)
                | Expression::GenericMethod(_, _, _, args, _) => Expression::Invoke(index, args),
                _ => index.kind,
            };
        }
        if let Some(name) =
            root(e).filter(|n| !locals.contains_key(*n) && self.uncaptured.contains(*n))
        {
            return Err(e.at.error(format!(
                "anonymous functions cannot capture `{name}`; pass it as a parameter"
            )));
        }
        if let Expression::Name(name) | Expression::Call(name, _) = &e.kind
            && self.type_param(name)
            && locals.contains_key(name)
        {
            return Err(e
                .at
                .error("a type parameter name cannot also name a value binding"));
        }
        // Brackets on a local always index it. These had no index reading.
        if let Expression::GenericCall(name, types, _) | Expression::GenericValue(name, types) =
            &e.kind
        {
            let base = name.split('.').next().unwrap();
            if locals.contains_key(base) {
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
                Expression::Member(base, n, _) => Some(format!("{}.{n}", path(base)?)),
                _ => None,
            }
        }
        let candidate = match &e.kind {
            Expression::Method(base, name, _, _) => path(base).map(|p| format!("{p}.{name}")),
            Expression::GenericMethod(base, name, _, _, _) => {
                path(base).map(|p| format!("{p}.{name}"))
            }
            Expression::Member(..) => path(e),
            _ => None,
        };
        if let Some(candidate) =
            candidate.filter(|p| !locals.contains_key(p.split('.').next().unwrap()))
            && let Some(symbol) = self.symbol(&candidate, &e.at)?
        {
            let member = match &e.kind {
                Expression::Member(_, name, pos) | Expression::Method(_, name, _, pos) => {
                    Some((name.as_str(), *pos))
                }
                Expression::GenericMethod(_, name, _, _, pos) => Some((&**name, *pos)),
                _ => None,
            };
            if let Some((name, pos)) = member.filter(|_| super::ide::active()) {
                let target = self.constants.get(&symbol).unwrap_or(&symbol);
                super::ide::refer_member(&e.at, pos, name, super::ide::global(target));
            }
            if let Some(getter) = self.constants.get(&symbol) {
                if !matches!(e.kind, Expression::Member(..)) {
                    return Err(e.at.error("a module constant is a value, not a callable"));
                }
                e.kind = Expression::Call(getter.clone(), vec![]);
                return Ok(());
            }
            e.kind = match &mut e.kind {
                Expression::Method(_, _, args, _) => Expression::Call(symbol, std::mem::take(args)),
                Expression::GenericMethod(_, _, types, args, _) => {
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
            if let Expression::Call(_, args) | Expression::GenericCall(_, _, args) = &mut e.kind {
                for arg in args {
                    self.expr(arg, locals)?;
                }
            }
            canonical_facility_expression(e);
            return Ok(());
        }
        match &mut e.kind {
            Expression::Name(n)
            | Expression::Call(n, _)
            | Expression::GenericCall(n, _, _)
            | Expression::GenericValue(n, _)
                if !locals.contains_key(n) && !self.type_param(n) =>
            {
                let written = n.chars().count() as u32;
                if let Some(symbol) = self.symbol(n, &e.at)? {
                    *n = symbol;
                } else if n != "replace"
                    && !matches!(
                        n.as_str(),
                        "iter" | "enumerate" | "zip" | "reversed" | "any" | "all"
                    )
                    && !builtin(n)
                    && !self.namespace_root(n)
                {
                    *n = qualified(&self.module, n);
                }
                if super::ide::active() {
                    let target = self.constants.get(n).unwrap_or(n);
                    super::ide::refer(&e.at, Find::At(written), super::ide::global(target));
                }
            }
            Expression::Name(n)
            | Expression::Call(n, _)
            | Expression::GenericCall(n, _, _)
            | Expression::GenericValue(n, _)
                if super::ide::active() =>
            {
                if let Some(&pos) = locals.get(n) {
                    super::ide::refer_local(&e.at, pos, n);
                }
            }
            _ => {}
        }
        if let Expression::Name(n)
        | Expression::Call(n, _)
        | Expression::GenericCall(n, _, _)
        | Expression::GenericValue(n, _) = &e.kind
            && !locals.contains_key(n)
            && !self.type_param(n)
            && let Some(getter) = self.constants.get(n)
        {
            if !matches!(e.kind, Expression::Name(_)) {
                return Err(e.at.error("a module constant is a value, not a callable"));
            }
            e.kind = Expression::Call(getter.clone(), vec![]);
            return Ok(());
        }
        match &mut e.kind {
            Expression::GenericMethod(base, _, types, args, _) => {
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
                    if !locals.contains_key(&capture.name) {
                        return Err(e.at.error(format!(
                            "capture requires a local binding `{}`",
                            capture.name
                        )));
                    }
                }
                let mut scope = self.clone();
                scope.uncaptured.extend(locals.keys().cloned());
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
            Expression::Format(format) => {
                for arg in format.operands_mut() {
                    self.expr(arg, locals)?;
                }
            }
            Expression::Member(base, _, _)
            | Expression::NamedArgument(_, base)
            | Expression::Group(base)
            | Expression::Unary(_, base)
            | Expression::Try(base) => self.expr(base, locals)?,
            Expression::Method(base, _, args, _) | Expression::Invoke(base, args) => {
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
            Expression::Match(value, cases) => {
                let at = e.at.clone();
                self.match_block(value, cases, &at, locals)?
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
                            for name in n.iter() {
                                let pos = Pos::of(&e.at);
                                super::ide::bind_local(
                                    &e.at,
                                    pos,
                                    name,
                                    false,
                                    Find::Before(name.clone()),
                                );
                                inner.insert(name.clone(), pos);
                            }
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
            | Expression::Bytes(_)
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
    if let Expression::GenericCall(name, types, args) = &mut e.kind
        && name == "channel"
    {
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
    if let Expression::GenericValue(name, types) = &mut e.kind
        && matches!(
            name.as_str(),
            "SpawnError"
                | "AppendError"
                | "SendError"
                | "SendTimeoutError"
                | "Selected"
                | "SubmitError"
                | "ParallelError"
                | "Sender"
                | "Receiver"
                | "Future"
        )
    {
        e.kind = Expression::Type(Box::new(TypeRef {
            at: e.at.clone(),
            concrete: None,
            name: Some(name.clone()),
            args: std::mem::take(types),
        }));
    }
}

/// Modules with fewer functions or classes resolve them on the calling
/// thread, which is faster than starting helpers for them.
const PARALLEL_FUNCTIONS: usize = 256;
const PARALLEL_CLASSES: usize = 64;

#[cfg_attr(feature = "profile", hotpath::measure)]
fn resolve(
    mut modules: Vec<Module>,
    dependencies: Vec<Vec<usize>>,
    at: Token,
    require_main: bool,
) -> Result<Resolved> {
    if let Some(embed) = modules.iter().flat_map(|m| &m.embeds).next() {
        return Err(embed.at.error(
            "`include_str` and `include_bytes` read a file relative to the source file, so they require compiling a file",
        ));
    }
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
                    .chain(m.facility_exports.iter().map(|n| {
                        let name = n.rsplit('.').next().unwrap_or(n);
                        (name.to_string(), n.to_string())
                    }))
                    .collect(),
            )
        })
        .collect();
    let mut result = Resolved {
        standard_sources: HashSet::default(),
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
        inputs: inputs::Inputs::default(),
    };
    for (i, m) in modules.iter_mut().enumerate() {
        let mut scope = Scope {
            captures: HashSet::default(),
            uncaptured: HashSet::default(),
            type_params: Vec::new(),
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
                if super::ide::active() {
                    let symbol = constants.get(name).unwrap_or(name);
                    super::ide::refer(
                        &import.at,
                        super::ide::Find::Imported(item.clone()),
                        super::ide::global(symbol),
                    );
                }
                Arc::make_mut(&mut scope.imported).insert(import.binding.clone(), name.clone());
            } else {
                Arc::make_mut(&mut scope.namespaces)
                    .insert(import.binding.clone(), exports[*dependency].clone());
            }
        }
        // Resolving one function reads only the scope, so functions resolve
        // on several threads; the first error is still the first in source.
        // An editor check records what names refer to on this thread.
        let workers = if m.functions.len() < PARALLEL_FUNCTIONS || super::ide::active() {
            1
        } else {
            crate::parallel::workers()
        };
        let resolved = crate::parallel::map_mut(&mut m.functions, workers, |f| {
            scope.function(f).map_err(sendable)
        });
        if super::ide::active() {
            // A function whose body does not resolve keeps its signature, so
            // the functions that call it can still be checked.
            let mut failed = HashSet::default();
            for (f, result) in m.functions.iter_mut().zip(resolved) {
                if let Err(error) = result {
                    super::ide::recover(&*error);
                    f.body.clear();
                    f.defaults.clear();
                    if scope.function(f).is_err() {
                        failed.insert(f.name.clone());
                    } else {
                        super::ide::skip(&qualified(&m.name, &f.name));
                    }
                }
            }
            m.functions.retain(|f| !failed.contains(&f.name));
        } else {
            resolved
                .into_iter()
                .collect::<std::result::Result<(), _>>()
                .map_err(|e| -> Box<dyn Error> { e })?;
        }
        for f in &mut m.functions {
            let public = m.exports.contains(&f.name);
            if public {
                signature_public_types(f, &[], false, &mut result.public_api);
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
                    owner: p.at.source(),
                    public,
                },
            );
            for method in &mut p.methods {
                scope.function(method)?;
                if public {
                    let parameters: Vec<_> = parameters
                        .iter()
                        .chain(&method.type_params)
                        .cloned()
                        .collect();
                    for t in method
                        .inputs
                        .iter()
                        .map(|(_, t)| t)
                        .chain(std::iter::once(&method.output))
                        .chain(method.type_params.iter().filter_map(|(_, b)| b.as_ref()))
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
                        owner: a.at.source(),
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
            result.access.enums.insert(e.name.clone());
            result.access.types.insert(
                e.name.clone(),
                Access {
                    owner: e.at.source(),
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
            for f in &mut e.methods {
                scope.function(f)?;
                let exposed = public && e.public_members.contains(&f.name);
                result.access.members.insert(
                    (e.name.clone(), f.name.clone()),
                    Access {
                        owner: e.at.source(),
                        public: exposed,
                    },
                );
                if exposed {
                    signature_public_types(f, &e.type_params, true, &mut result.public_api);
                }
            }
        }
        let workers = if m.classes.len() < PARALLEL_CLASSES || super::ide::active() {
            1
        } else {
            crate::parallel::workers()
        };
        crate::parallel::map_mut(&mut m.classes, workers, |c| {
            scope.class(c).map_err(sendable)
        })
        .into_iter()
        .collect::<std::result::Result<(), _>>()
        .map_err(|e| -> Box<dyn Error> { e })?;
        for c in &mut m.classes {
            let public = m.exports.contains(&c.name);
            c.name = qualified(&m.name, &c.name);
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
                    owner: c.at.source(),
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
                    owner: c.at.source(),
                    public: public && constructor,
                },
            );
            for (n, t) in &c.fields {
                let exposed = public && c.public_members.contains(n);
                result.access.members.insert(
                    (c.name.clone(), n.clone()),
                    Access {
                        owner: c.at.source(),
                        public: exposed,
                    },
                );
                if exposed {
                    data_public_types(t, &c.type_params, &mut result.public_api);
                }
            }
            for f in &c.methods {
                let exposed = public && c.public_members.contains(&f.name);
                result.access.members.insert(
                    (c.name.clone(), f.name.clone()),
                    Access {
                        owner: c.at.source(),
                        public: exposed,
                    },
                );
                if exposed {
                    signature_public_types(f, &c.type_params, true, &mut result.public_api);
                }
            }
        }
        result.functions.append(&mut m.functions);
        result.declarations.append(&mut m.declarations);
        result.enums.append(&mut m.enums);
        result.classes.append(&mut m.classes);
        result.protocols.append(&mut m.protocols);
    }
    super::consts::fold(&mut result.functions)?;
    Ok(result)
}

pub(super) fn check_member(
    access: &AccessMap,
    class: &str,
    member: &str,
    at: &Token,
) -> Result<()> {
    check_member_at(access, class, member, at, at)
}

/// Protocol bounds check access in their defining module, but failures belong
/// to the call that supplied the concrete type, not to the bound declaration.
pub(super) fn check_member_at(
    access: &AccessMap,
    class: &str,
    member: &str,
    context: &Token,
    diagnostic: &Token,
) -> Result<()> {
    let class = class.split('[').next().unwrap();
    if let Some(rule) = access.members.get(&(class.into(), member.into()))
        && !rule.public
        && rule.owner != context.source()
    {
        if access.types.get(class).is_some_and(|ty| !ty.public) {
            let kind = if access.enums.contains(class) {
                "enum"
            } else {
                "class"
            };
            return Err(diagnostic.error(format!(
                    "{kind} `{class}` is private to its defining module; declare it `pub {kind}` to allow access to its public members from another module, including a generic function defined there"
                )));
        }
        if class == crate::mutex::CLASS {
            return Err(diagnostic.error(
                "a Mutex's fields are private; reach its value with `with mutex.lock() as value:`",
            ));
        }
        return Err(diagnostic.error(format!(
                "`{class}.{member}` is private to its defining module; declare the member public to allow access from another module"
            )));
    }
    Ok(())
}

/// Collects the public-API types of a function signature: its inputs (minus
/// the receiver for methods), output, and type-parameter bounds. Callable
/// bounds contribute their argument and return types rather than the bound
/// itself. Protocol methods keep their own walk, which passes bounds whole.
fn signature_public_types(
    f: &Function,
    owner_params: &[(String, Option<TypeRef>)],
    skip_receiver: bool,
    out: &mut Vec<TypeRef>,
) {
    let params: Vec<_> = owner_params.iter().chain(&f.type_params).cloned().collect();
    for t in f
        .inputs
        .iter()
        .skip(usize::from(skip_receiver))
        .map(|(_, t)| t)
        .chain(std::iter::once(&f.output))
    {
        data_public_types(t, &params, out);
    }
    for bound in f.type_params.iter().filter_map(|(_, b)| b.as_ref()) {
        if matches!(bound.name.as_deref(), Some("Callable" | "OnceCallable")) {
            for part in &bound.args {
                data_public_types(part, &params, out);
            }
        } else {
            data_public_types(bound, &params, out);
        }
    }
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
            .is_some_and(|n| !builtin(n) && !matches!(n.as_str(), "&" | "&mut" | "|"))
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
            Ty::Class(c) => Some(&*c.name),
            Ty::Enum(e) => Some(&*e.name),
            Ty::ForeignPtr(name) => Some(name.as_ref()),
            _ => None,
        };
        if let Some(name) = nominal
            && let Some(rule) = access.types.get(name)
        {
            if !rule.public {
                return Err(at.error(format!("public signature exposes private type `{name}`")));
            }
            return Ok(());
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
            | Ty::Clist(t)
            | Ty::Set(t)
            | Ty::Ref(t, _)
            | Ty::Channel(t, _)
            | Ty::Future(t)
            | Ty::Box(t)
            | Ty::Shared(t) => visible(t, at, access, aliases)?,
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
    fn sources(dir: &Path, out: &mut Vec<(std::path::PathBuf, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let extension = path.extension().and_then(|e| e.to_str());
            if path.is_dir() {
                sources(&path, out);
            } else if matches!(extension, Some("plenty" | "plentyi")) {
                let text = std::fs::read_to_string(&path).unwrap();
                out.push((path, text));
            } else if extension == Some("md") {
                let text = std::fs::read_to_string(&path).unwrap();
                let mut fence: Option<String> = None;
                for line in text.lines() {
                    let delimiter = line.trim_start().starts_with("```");
                    match &mut fence {
                        Some(code) if delimiter => {
                            out.push((path.clone(), std::mem::take(code)));
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

    /// Every repository module goes to one of this many cases, which the
    /// harness runs in parallel (issue #412). The `#[values]` list below
    /// must name every shard from 0 to this count.
    const PARSE_SHARDS: usize = 8;

    #[rstest::rstest]
    fn parsing_item_by_item_matches_parsing_with_every_token(
        #[values(0, 1, 2, 3, 4, 5, 6, 7)] shard: usize,
    ) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut modules = Vec::new();
        for dir in ["examples", "stdlib", "tests", "book/src"] {
            sources(&root.join(dir), &mut modules);
        }
        assert!(modules.len() > 500, "found {} modules", modules.len());
        // Sorting by path makes the split independent of directory order; a
        // stable sort keeps one page's fences in order.
        modules.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, module) in modules.iter().skip(shard).step_by(PARSE_SHARDS) {
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
