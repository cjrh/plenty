//! Source loading, explicit name binding, and module privacy before lowering.
//!
//! The backend still consumes one program. Qualified declaration keys retain
//! nominal identity while aliases only affect the names visible in each source.
use super::*;
use std::path::{Path, PathBuf};

struct Import {
    at: Token,
    module: String,
    item: Option<String>,
    binding: String,
}

#[derive(Default)]
struct Module {
    name: String,
    functions: Vec<Function>,
    declarations: Vec<TypeAlias>,
    enums: Vec<enums::EnumDecl>,
    classes: Vec<classes::ClassDecl>,
    protocols: Vec<protocols::Protocol>,
    imports: Vec<Import>,
    exports: HashSet<String>,
}

pub(super) struct Access {
    owner: Option<Rc<str>>,
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

impl Parser {
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

fn parse(source: &str, path: Option<&Path>, name: String) -> Result<(Module, Token)> {
    let mut tokens = lex(source).map_err(|e| -> Box<dyn Error> {
        match path {
            Some(p) => format!("{}:{e}", p.display()).into(),
            None => e,
        }
    })?;
    let origin = path.map(|p| Rc::<str>::from(p.to_string_lossy().as_ref()));
    for token in &mut tokens {
        token.source = origin.clone();
    }
    let mut parser = Parser {
        tokens,
        pos: 0,
        type_depth: 0,
    };
    let mut module = Module {
        name,
        ..Module::default()
    };
    while parser.peek().kind != Kind::Eof {
        let public = parser.eat("pub");
        let at = parser.peek().clone();
        let name = if parser.peek().is("extern") || parser.peek().is("opaque") {
            if !path.is_some_and(|p| p.extension().is_some_and(|e| e == "plentyi")) {
                return Err(at.error("C declarations require a trusted .plentyi interface module"));
            }
            if parser.eat("extern") {
                parser.expect("def")?;
                parser.pos -= 1;
                let f = parser.function_header(None, true, false)?;
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
            let f = parser.function_header(None, false, true)?;
            let name = f.name.clone();
            module.functions.push(f);
            name
        } else if parser.peek().is("def") {
            let f = parser.function()?;
            let name = f.name.clone();
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
            module.imports.extend(parser.imports()?);
            continue;
        } else {
            return Err(at.error(if public {
                "pub requires a function, class, enum, or type declaration; re-exports are not supported"
            } else {
                "executable statements are not allowed at module scope; put them inside `def main() -> ():`"
            }));
        };
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
    fn names(&self) -> HashSet<String> {
        self.functions
            .iter()
            .map(|f| &f.name)
            .chain(self.declarations.iter().map(|a| &a.name))
            .chain(self.enums.iter().map(|e| &e.name))
            .chain(self.classes.iter().map(|c| &c.name))
            .chain(self.protocols.iter().map(|p| &p.name))
            .cloned()
            .collect()
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
    struct Loader {
        root: PathBuf,
        active: Vec<PathBuf>,
        loaded: HashMap<PathBuf, usize>,
        modules: Vec<Module>,
        dependencies: Vec<Vec<usize>>,
    }
    impl Loader {
        fn file(&mut self, path: PathBuf, name: String) -> Result<(usize, Token)> {
            if let Some(start) = self.active.iter().position(|p| p == &path) {
                let cycle = self.active[start..]
                    .iter()
                    .chain(std::iter::once(&path))
                    .map(|p| p.display().to_string())
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
            self.active.push(path.clone());
            let source = std::fs::read_to_string(&path)
                .map_err(|e| format!("reading {}: {e}", path.display()))?;
            let (module, at) = parse(&source, Some(&path), name)?;
            let mut deps = Vec::new();
            for import in &module.imports {
                let dependency = self.import_path(import)?;
                let index = if let Some(index) = self.loaded.get(&dependency) {
                    *index
                } else {
                    // Canonical relative paths make alternate spellings refer to one module.
                    let relative = dependency
                        .strip_prefix(&self.root)
                        .unwrap()
                        .with_extension("");
                    let identity = relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(".");
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
            self.loaded.insert(path, index);
            self.modules.push(module);
            self.dependencies.push(deps);
            Ok((index, at))
        }

        fn import_path(&self, import: &Import) -> Result<PathBuf> {
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
            Ok(canonical)
        }
    }
    let mut loader = Loader {
        root,
        active: vec![],
        loaded: HashMap::new(),
        modules: vec![],
        dependencies: vec![],
    };
    let (_, at) = loader.file(entry, String::new())?;
    let mut result = resolve(loader.modules, loader.dependencies, at, require_main)?;
    result.source_paths = loader.loaded.into_keys().collect();
    result.source_paths.sort();
    Ok(result)
}

#[derive(Clone)]
struct Scope {
    module: String,
    symbols: HashMap<String, String>,
    namespaces: HashMap<String, Rc<HashMap<String, String>>>,
    type_params: HashSet<String>,
}

impl Scope {
    fn symbol(&self, path: &str, at: &Token) -> Result<Option<String>> {
        if let Some(symbol) = self.symbols.get(path) {
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
            if let Some(symbol) = self.symbol(name, &ty.at)? {
                *name = symbol;
            } else if !builtin(name) && !matches!(name.as_str(), "&" | "&mut") {
                return Err(ty.at.error(format!("unknown type `{name}`")));
            }
        }
        Ok(())
    }
    fn function(&self, f: &mut Function) -> Result<()> {
        let mut scope = self.clone();
        scope
            .type_params
            .extend(f.type_params.iter().map(|(n, _)| n.clone()));
        for (_, bound) in &mut f.type_params {
            if let Some(bound) = bound {
                self.ty(bound)?;
            }
        }
        for (_, ty) in &mut f.inputs {
            scope.ty(ty)?;
        }
        scope.ty(&mut f.output)?;
        let mut locals = f.inputs.iter().map(|(n, _)| n.clone()).collect();
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
                    ..
                } => {
                    if let Some(ty) = annotation {
                        self.ty(ty)?;
                    }
                    self.expr(value, locals)?;
                    locals.insert(name.clone());
                }
                Statement::Unpack { names, value, .. } => {
                    self.expr(value, locals)?;
                    locals.extend(names.iter().cloned());
                }
                Statement::SetIndex { target, value } => {
                    self.expr(target, locals)?;
                    self.expr(value, locals)?;
                }
                Statement::If { condition, yes, no } => {
                    self.expr(condition, locals)?;
                    self.block(yes, &mut locals.clone())?;
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
                    let mut inner = locals.clone();
                    inner.extend(name.iter().cloned());
                    self.block(body, &mut inner)?;
                }
                Statement::Match { value, cases } => {
                    self.expr(value, locals)?;
                    for case in cases {
                        let mut inner = locals.clone();
                        if let Some((ty, _, bindings)) = &mut case.pattern {
                            if let Some(ty) = ty {
                                self.ty(ty)?;
                            }
                            if let Some(bindings) = bindings {
                                inner.extend(bindings.iter().cloned());
                            }
                        }
                        self.block(&mut case.body, &mut inner)?;
                    }
                }
                Statement::Pass | Statement::Break | Statement::Continue => {}
            }
        }
        Ok(())
    }
    fn expr(&self, e: &mut Expr, locals: &HashSet<String>) -> Result<()> {
        if let Expression::Name(name) | Expression::Call(name, _) = &e.kind {
            if self.type_params.contains(name) && locals.contains(name) {
                return Err(e
                    .at
                    .error("a type parameter name cannot also name a value binding"));
            }
        }
        if let Expression::GenericCall(name, _, _) | Expression::GenericValue(name, _) = &e.kind {
            if locals.contains(name.split('.').next().unwrap()) {
                let (name, types, args) = match &mut e.kind {
                    Expression::GenericCall(name, types, args) => {
                        (name, types, Some(std::mem::take(args)))
                    }
                    Expression::GenericValue(name, types) => (name, types, None),
                    _ => unreachable!(),
                };
                let [index] = types.as_slice() else {
                    return Err(e.at.error("indexing takes one expression"));
                };
                let Some(index_name) = index.name.as_ref().filter(|_| index.args.is_empty()) else {
                    return Err(e.at.error("a local binding shadows this generic function"));
                };
                fn name_path(name: &str, at: &Token) -> Expr {
                    let mut parts = name.split('.');
                    let mut e = Expr {
                        at: at.clone(),
                        kind: Expression::Name(parts.next().unwrap().into()),
                    };
                    for part in parts {
                        e = Expr {
                            at: at.clone(),
                            kind: Expression::Member(Box::new(e), part.into()),
                        };
                    }
                    e
                }
                let callee = Expr {
                    at: e.at.clone(),
                    kind: Expression::Index(
                        Box::new(name_path(name, &e.at)),
                        Box::new(name_path(index_name, &index.at)),
                    ),
                };
                e.kind = if let Some(args) = args {
                    Expression::Invoke(Box::new(callee), args)
                } else {
                    callee.kind
                };
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
            Expression::Method(base, name, _) => path(base).map(|p| format!("{p}.{name}")),
            Expression::Member(..) => path(e),
            _ => None,
        };
        if let Some(candidate) =
            candidate.filter(|p| !locals.contains(p.split('.').next().unwrap()))
        {
            if let Some(symbol) = self.symbol(&candidate, &e.at)? {
                e.kind = if let Expression::Method(_, _, args) = &mut e.kind {
                    Expression::Call(symbol, std::mem::take(args))
                } else {
                    Expression::Name(symbol)
                };
                // The resolved name must not pass through local resolution a second time.
                if let Expression::Call(_, args) = &mut e.kind {
                    for arg in args {
                        self.expr(arg, locals)?;
                    }
                }
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
                } else if !builtin(n) && !self.namespace_root(n) {
                    *n = qualified(&self.module, n);
                }
            }
            _ => {}
        }
        match &mut e.kind {
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
            Expression::Call(_, args) | Expression::Tuple(args, _) => {
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
            Expression::ClassNew(..)
            | Expression::ClassReady(..)
            | Expression::Number(_)
            | Expression::Text(_)
            | Expression::Bool(_)
            | Expression::Unit
            | Expression::Name(_) => {}
        }
        Ok(())
    }
}

fn resolve(
    mut modules: Vec<Module>,
    dependencies: Vec<Vec<usize>>,
    at: Token,
    require_main: bool,
) -> Result<Resolved> {
    let exports: Vec<Rc<HashMap<String, String>>> = modules
        .iter()
        .map(|m| {
            Rc::new(
                m.exports
                    .iter()
                    .map(|n| (n.clone(), qualified(&m.name, n)))
                    .collect(),
            )
        })
        .collect();
    let mut result = Resolved {
        source_paths: vec![],
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
            type_params: HashSet::new(),
            module: m.name.clone(),
            symbols: m
                .names()
                .into_iter()
                .map(|n| (n.clone(), qualified(&m.name, &n)))
                .collect(),
            namespaces: HashMap::new(),
        };
        for (import, dependency) in m.imports.iter().zip(&dependencies[i]) {
            let root = import.binding.split('.').next().unwrap();
            if builtin(root)
                || scope.symbols.contains_key(root)
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
                scope.symbols.insert(import.binding.clone(), name.clone());
            } else {
                scope
                    .namespaces
                    .insert(import.binding.clone(), exports[*dependency].clone());
            }
        }
        for f in &mut m.functions {
            let public = m.exports.contains(&f.name);
            scope.function(f)?;
            if public {
                result
                    .public_api
                    .extend(f.type_params.iter().filter_map(|(_, b)| b.clone()));
                fn concrete_parts(
                    t: &TypeRef,
                    params: &[(String, Option<TypeRef>)],
                    out: &mut Vec<TypeRef>,
                ) {
                    fn mentions(t: &TypeRef, params: &[(String, Option<TypeRef>)]) -> bool {
                        params.iter().any(|(n, _)| t.name.as_ref() == Some(n))
                            || t.args.iter().any(|t| mentions(t, params))
                    }
                    if !mentions(t, params) {
                        out.push(t.clone());
                    } else {
                        for arg in &t.args {
                            concrete_parts(arg, params, out);
                        }
                    }
                }
                for t in f
                    .inputs
                    .iter()
                    .map(|(_, t)| t)
                    .chain(std::iter::once(&f.output))
                {
                    concrete_parts(t, &f.type_params, &mut result.public_api);
                }
            }
            f.name = qualified(&m.name, &f.name);
        }
        for p in &mut m.protocols {
            let public = m.exports.contains(&p.name);
            p.name = qualified(&m.name, &p.name);
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
                        protocols::public_types(t, &p.name, &mut result.public_api);
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
            for (_, fields) in &mut e.variants {
                for t in fields {
                    scope.ty(t)?;
                    if public {
                        result.public_api.push(t.clone());
                    }
                }
            }
        }
        for c in &mut m.classes {
            let public = m.exports.contains(&c.name);
            c.name = qualified(&m.name, &c.name);
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
                    result.public_api.push(t.clone());
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
                    result
                        .public_api
                        .extend(f.inputs.iter().skip(1).map(|(_, t)| t.clone()));
                    result.public_api.push(f.output.clone());
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
    if let Some(rule) = access.members.get(&(class.into(), member.into())) {
        if !rule.public && rule.owner != at.source {
            return Err(at.error(format!(
                "`{class}.{member}` is private to its defining module"
            )));
        }
    }
    Ok(())
}

pub(super) fn check_api(refs: &[TypeRef], aliases: &TypeAliases, access: &AccessMap) -> Result<()> {
    fn visible(ty: &Ty, at: &Token, access: &AccessMap) -> Result<()> {
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
            Ty::Callable(sig) => {
                for ty in sig.inputs.iter().chain(sig.output.iter()) {
                    visible(ty, at, access)?;
                }
            }
            Ty::List(t) | Ty::Set(t) | Ty::Ref(t, _) => visible(t, at, access)?,
            Ty::Generator(t) => visible(&t.element, at, access)?,
            Ty::Dict(k, v) => {
                visible(k, at, access)?;
                visible(v, at, access)?;
            }
            Ty::Enum(e) => {
                for t in e.variants.iter().flat_map(|v| &v.fields) {
                    visible(t, at, access)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    for t in refs {
        if t.name.as_deref() == Some("IntType") && t.args.is_empty() {
            continue;
        }
        if let Some(name) = t.name.as_ref().filter(|n| access.protocols.contains(*n)) {
            if !access.types[name].public {
                return Err(t.at.error(format!(
                    "public signature exposes private protocol `{name}`"
                )));
            }
            continue;
        }
        if let Some(ty) = t.resolve(aliases)? {
            visible(&ty, &t.at, access)?;
        }
    }
    Ok(())
}
