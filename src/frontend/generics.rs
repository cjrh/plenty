//! Cached monomorphization shared by explicit and inferred calls during lowering.
use super::*;
use crate::ownership::Instantiation;
use std::collections::VecDeque;
use std::fmt::Write as _;

/// Repeats allowed along a dependency chain. A chain in which no declaration
/// repeats is no longer than the program's number of generic functions and
/// generator consumers, so the depth limit adds that number: only steps that
/// repeat a declaration can exceed it, and unbounded expansion repeats one
/// without end (issue #435).
const MAX_SPECIALIZATION_REPEATS: usize = 256;
/// Bounds expansion that branches within the depth limit, such as `f[T]`
/// requiring both `f[list[T]]` and `f[Box[T]]`. A program has finitely many
/// templates, so unbounded expansion must grow one of them without bound
/// (issue #428).
const MAX_TEMPLATE_INSTANCES: usize = 16_384;
/// Bounds the work of expansion that drags other templates along, each of
/// which can approach the per-template limit before one reaches it. Breadth
/// stays well below it: the 200x200 generic compile benchmark needs 40,000
/// (issue #428).
const MAX_FUNCTION_INSTANCES: usize = 262_144;

/// The type parser rejects `Callable` and `OnceCallable` without a signature,
/// so a callable bound always carries at least its return type in `args`.
pub(super) fn callable_bound(bound: &TypeRef) -> bool {
    matches!(bound.name.as_deref(), Some("Callable" | "OnceCallable"))
}

/// The test a type must pass to satisfy the built-in numeric constraint
/// `bound` names.
pub(super) fn numeric_bound(bound: &TypeRef) -> Option<fn(&Ty) -> bool> {
    match bound.name.as_deref() {
        Some("IntType") => Some(Ty::is_int),
        Some("FloatType") => Some(Ty::is_float),
        _ => None,
    }
}

pub(super) fn callable_pattern(bound: &TypeRef) -> TypeRef {
    let mut pattern = bound.clone();
    pattern.name = Some("Callable".into());
    pattern
}

pub(super) fn check_callable_bound(
    bound: &TypeRef,
    ty: &Ty,
    replacements: &Substitution,
    aliases: &TypeAliases,
    at: &Token,
) -> Result<()> {
    let mut pattern = callable_pattern(bound);
    substitute(&mut pattern, replacements)?;
    let Some(Ty::Callable(expected)) = pattern.resolve(aliases)? else {
        unreachable!("Callable constraint resolves to a callable signature")
    };
    if callables::signature(ty) != Some(expected.as_ref())
        || (bound.name.as_deref() == Some("Callable") && matches!(ty, Ty::Closure(t) if t.once))
    {
        let requirement =
            expected
                .to_string()
                .replacen("Callable", bound.name.as_deref().unwrap(), 1);
        let hint = if callables::signature(ty) == Some(expected.as_ref())
            && matches!(ty, Ty::Closure(t) if t.once)
        {
            if matches!(ty, Ty::Closure(t) if t.borrowed) {
                "; a one-shot closure with borrowed captures can only be consumed locally".into()
            } else {
                format!(
                    "; a one-shot closure is consumed by its call; use `F: {}` with an owned `F` parameter for a consuming callback API",
                    expected.to_string().replacen("Callable", "OnceCallable", 1)
                )
            }
        } else {
            String::new()
        };
        return Err(at.error(format!("{ty} does not satisfy {requirement}{hint}")));
    }
    Ok(())
}

/// The parameter and return types of a function, resolved.
pub(super) type Signature = (Vec<Ty>, Type);

/// Declares every function and validates every protocol. Also returns the
/// resolved types of the functions queued in `Engine::pending`, in queue
/// order, so that registering their signatures does not resolve them again.
#[cfg_attr(feature = "profile", hotpath::measure)]
pub(super) fn prepare(
    mut functions: Vec<Function>,
    aliases: &TypeAliases,
    protocols: Vec<protocols::Protocol>,
    kept: &[&Function],
    table: &mut functions::Functions,
) -> Result<(Engine, Vec<Signature>)> {
    aliases.validate_data_bounds()?;
    functions.extend(aliases.data.factories());
    let mut retention = retention::Retention::new(&functions, aliases);
    for f in kept {
        retention.keep(&f.body);
    }
    let mut declarations = HashMap::default();
    let conflict = |protocol: &protocols::Protocol| {
        protocol.at.error(format!(
            "protocol `{}` is already defined or conflicts with another declaration",
            protocol.name
        ))
    };
    for protocol in &protocols {
        if aliases.contains_key(&protocol.name)
            || declarations
                .insert(protocol.name.clone(), protocol.clone())
                .is_some()
        {
            return Err(conflict(protocol));
        }
        // Validate requirements even if no function uses this protocol. The
        // receiver placeholder is replaced by the implementing class at use.
        for method in &protocol.methods {
            aliases.probe(
                std::iter::once(&protocol.name)
                    .chain(protocol.type_params.iter().map(|(n, _)| n))
                    .chain(method.type_params.iter().map(|(n, _)| n)),
                |validation| protocols::signature(method, validation).map(drop),
            )?;
        }
    }
    // Requirement parameters are constrained by protocols, which may be
    // declared after the protocol that uses them.
    for protocol in &protocols {
        for method in &protocol.methods {
            for bound in method.type_params.iter().filter_map(|(_, b)| b.as_ref()) {
                if !bound
                    .name
                    .as_ref()
                    .is_some_and(|name| declarations.contains_key(name))
                {
                    return Err(bound
                        .at
                        .error("requirement parameters take only protocol constraints"));
                }
            }
        }
    }
    let mut methods = HashMap::default();
    let mut signatures = Vec::with_capacity(functions.len());
    for f in functions.iter().filter(|f| f.type_params.is_empty()) {
        // Every signature is resolved here, so its errors are reported
        // first, but only a method's can satisfy a protocol. A program-wide
        // table would cost a name copy and a cache miss per function
        // (issue #57); the list kept for registration is in queue order.
        let signature = protocols::signature(f, aliases)?;
        if crate::record::is_method(&f.name) {
            methods.insert(f.name.clone(), signature.clone());
        }
        signatures.push(signature);
    }
    let mut engine = Engine {
        protocols: declarations,
        methods,
        templates: HashMap::default(),
        defaults: HashMap::default(),
        call_wrappers: HashMap::default(),
        deferred_reporting: HashMap::default(),
        unused_methods: HashMap::default(),
        cache: HashMap::default(),
        template_ids: HashMap::default(),
        instantiated: Vec::new(),
        instances: 0,
        specializable: 0,
        specialization_depths: HashMap::default(),
        // One entry per function: sized up front, these are not rehashed
        // key by key at every doubling (issue #57).
        pending: VecDeque::with_capacity(functions.len()),
        compiled: Vec::new(),
        tail_candidates: HashMap::default(),
        active: HashSet::default(),
        frames: HashMap::default(),
        frame_instances: HashMap::default(),
        lent: Vec::new(),
        lent_ids: HashMap::default(),
        required: HashMap::default(),
        unsubstituted: HashMap::default(),
        protocol_instances: HashSet::default(),
        source_root: None,
        standard_sources: HashSet::default(),
        source_facts: None,
        retention,
    };
    for f in functions {
        if table.declared(&f.name).is_some()
            || engine.templates.contains_key(&f.name)
            || aliases.contains_key(&f.name)
        {
            return Err(f.at.error(format!(
                "function `{}` is already defined or conflicts with a type alias",
                f.name
            )));
        }
        if f.type_params.is_empty() {
            engine.declare(f, table);
        } else {
            if f.name == "main" {
                return Err(f.at.error("main cannot have type parameters"));
            }
            engine.validate_template(&f, aliases)?;
            engine.record_defaults(&f);
            engine.retention.template(&f);
            // The builtin `OverflowError[T]` factory calls nothing generic, so
            // it adds no depth; counting it would change every program's
            // reported limit.
            if f.name != "OverflowError" {
                engine.specializable += 1;
            }
            engine.templates.insert(f.name.clone(), Arc::new(f));
        }
    }
    // Checked against the function tables, which exist anyway: a table of
    // every function name for a few protocols cost time per function in any
    // program importing a module that declares one (issue #57).
    if let Some(protocol) = protocols
        .iter()
        .find(|p| table.declared(&p.name).is_some() || engine.templates.contains_key(&p.name))
    {
        return Err(conflict(protocol));
    }
    Ok((engine, signatures))
}

pub(super) struct Engine {
    protocols: HashMap<String, protocols::Protocol>,
    pub(super) methods: HashMap<String, Signature>,
    pub(super) templates: HashMap<String, Arc<Function>>,
    /// Retained after lowering without keeping complete function bodies alive.
    pub(super) defaults: HashMap<String, Vec<Option<Expr>>>,
    pub(super) call_wrappers: HashMap<(String, Vec<usize>), String>,
    /// Conditional Context methods retain their declarations without lowering
    /// an unsupported cause's formatting body merely by constructing Context.
    deferred_reporting: HashMap<String, (functions::Symbol, Arc<Function>)>,
    /// Methods of generic data instances that no lowered code has used yet,
    /// by function name. Their signatures are registered; their bodies are
    /// checked only once a use queues them (`demand_method`).
    unused_methods: HashMap<String, UnusedMethod>,
    /// Function instance symbols by template (its index in `instantiated`)
    /// and type arguments.
    cache: HashMap<(u32, Vec<Ty>), String>,
    /// Indices of templates with instances in `instantiated`. Numbering
    /// them spares the cache a name copy per call (issue #436).
    template_ids: HashMap<Arc<str>, u32>,
    /// Each template with instances, and how many it has, which
    /// `MAX_TEMPLATE_INSTANCES` limits. Counted here rather than by keying
    /// the cache by template: most templates have one instance, and a table
    /// per template costs more than that instance.
    instantiated: Vec<(Arc<str>, usize)>,
    /// Function instances created, which number their symbols.
    instances: usize,
    /// Generic functions and methods declared so far, and generator consumers
    /// specialized so far, which extend the depth limit. Counted apart from
    /// `templates` so that releasing a template cannot lower the limit.
    pub(super) specializable: usize,
    /// First-requiring dependency depth, also inherited by lifted code.
    /// Generator consumer frames have entries too. Keyed by symbol, like
    /// `required`, so an instance costs no name copy or string hash here
    /// (issue #436).
    pub(super) specialization_depths: HashMap<functions::Symbol, usize>,
    /// Functions to lower, with their symbols in the function table.
    pub(super) pending: VecDeque<(functions::Symbol, Arc<Function>)>,
    pub(super) compiled: Vec<Op>,
    pub(super) tail_candidates: HashMap<String, Vec<tail_calls::Candidate>>,
    pub(super) active: HashSet<functions::Symbol>,
    pub(super) frames: HashMap<(String, Vec<Ty>), String>,
    /// Frame specializations of each generator consumer.
    pub(super) frame_instances: HashMap<String, usize>,
    /// Pipelines lent to `&mut Iter[T]` parameters, by identity, and each
    /// identity by lender and plan index. Relowering a lender replaces its
    /// entries.
    pub(super) lent: Vec<Arc<super::pipelines::Lent>>,
    pub(super) lent_ids: HashMap<(functions::Symbol, usize), usize>,
    /// The use that first required each generic instance, by function symbol.
    /// Functions of ordinary code have no entry.
    pub(super) required: HashMap<functions::Symbol, Arc<Instantiation>>,
    /// Queued instances whose bodies are still their templates' bodies,
    /// with the replacements for their type parameters.
    unsubstituted: HashMap<functions::Symbol, (Arc<Function>, Substitution)>,
    /// Specializations checked under protocol contracts, including lifted code.
    /// Method calls in these bodies cannot inherit implementation-only defaults.
    pub(super) protocol_instances: HashSet<String>,
    pub(super) source_root: Option<std::path::PathBuf>,
    pub(super) standard_sources: HashSet<Arc<str>>,
    /// The last source `source_facts` described, and the result. Functions
    /// arrive module by module, so this spares a path copy and a path hash
    /// per function.
    source_facts: Option<(Arc<str>, Arc<str>, bool)>,
    pub(super) retention: retention::Retention,
}

/// What later calls need from a concrete function after it joined the queue.
pub(super) struct Declared {
    /// Whether `f.new(...)` builds a generator frame.
    pub(super) yields: bool,
    /// The declaration, shared with `pending`. A generator factory is lowered
    /// ahead of the queue, or copied under a frame symbol, through this handle.
    /// It is `None` once the function is lowered: its syntax tree is released
    /// then, not at the end of the program (issue #54).
    pub(super) function: Option<Arc<Function>>,
    /// Whether the function is lowered. Kept here, not in a table of its own:
    /// each whole-program table costs a cache miss per function (issue #57).
    pub(super) lowered: bool,
}

/// A method of a generic data instance, declared but not queued.
pub(super) struct UnusedMethod {
    symbol: functions::Symbol,
    function: Arc<Function>,
    /// `Type[Arguments].method`, for diagnostics.
    display: Arc<str>,
    generator_input: bool,
}

impl Engine {
    pub(super) fn defer_reporting_method(
        &mut self,
        function: Function,
        table: &mut functions::Functions,
    ) {
        let function = Arc::new(function);
        let symbol = self.record(&function, table);
        self.deferred_reporting
            .insert(function.name.clone(), (symbol, function));
    }

    pub(super) fn require_reporting_method(&mut self, name: &str) -> Option<Arc<Function>> {
        let (symbol, function) = self.deferred_reporting.remove(name)?;
        self.pending.push_back((symbol, Arc::clone(&function)));
        Some(function)
    }

    /// Records a method of a generic data instance without queueing its body.
    /// A body that only some type arguments support is then rejected only
    /// for an instance that uses it (issue #515).
    pub(super) fn defer_method(
        &mut self,
        function: Function,
        signature: &Signature,
        display: Arc<str>,
        table: &mut functions::Functions,
    ) {
        let function = Arc::new(function);
        let symbol = self.record(&function, table);
        self.unused_methods.insert(
            function.name.clone(),
            UnusedMethod {
                symbol,
                function,
                display,
                generator_input: signature.0.iter().any(Ty::unresolved_generator),
            },
        );
    }

    pub(super) fn is_unused_method(&self, name: &str) -> bool {
        self.unused_methods.contains_key(name)
    }

    /// Queues `name` if it is an unused instance method. Errors in its body
    /// are reported at `at`, the use, or at the use that first required
    /// `outer` when the use is itself in generic code.
    pub(super) fn demand_method(
        &mut self,
        name: &str,
        at: impl FnOnce() -> Arc<str>,
        outer: Option<&Arc<Instantiation>>,
    ) {
        let Some(UnusedMethod {
            symbol,
            function,
            display,
            generator_input,
        }) = self.unused_methods.remove(name)
        else {
            return;
        };
        // The owner's instance recorded its type arguments under the method.
        let parameters = self
            .required
            .get(&symbol)
            .map(|owner| Arc::clone(&owner.parameters))
            .unwrap_or_default();
        let required = match outer {
            Some(outer) => Instantiation {
                instance: display,
                parameters,
                ..(**outer).clone()
            },
            None => Instantiation {
                at: at(),
                requested: Arc::clone(&display),
                instance: display,
                parameters,
                unused_method: None,
            },
        };
        self.required.insert(symbol, Arc::new(required));
        // Each frame specialization copies such a declaration, which is
        // never lowered itself.
        if !generator_input {
            self.pending.push_back((symbol, function));
        }
    }

    /// Queues every unused instance method of a type the program's own
    /// package declares, for `--check-all-methods`, and returns whether there
    /// were any. No use exists, so an error is reported where the method's
    /// type was instantiated (issue #517).
    pub(super) fn demand_unused_methods(&mut self) -> bool {
        // Types from `std` and dependencies are skipped: their authors chose
        // which methods each instance supports, such as a copying `get` on a
        // container whose values cannot be copied.
        let own = |source: &Arc<str>| {
            !self.standard_sources.contains(source)
                && self
                    .source_root
                    .as_deref()
                    .is_none_or(|root| std::path::Path::new(&**source).starts_with(root))
        };
        // Symbols number methods as their types were instantiated, so the
        // first error reported does not depend on hash order.
        let mut unused: Vec<_> = self
            .unused_methods
            .iter()
            // A standalone source has no path.
            .filter(|(_, method)| method.function.at.source().is_none_or(|s| own(&s)))
            .map(|(name, method)| (method.symbol, name.clone()))
            .collect();
        unused.sort_unstable();
        for (symbol, name) in &unused {
            let method = &self.unused_methods[name];
            let display = Arc::clone(&method.display);
            let required = match self.required.get(symbol) {
                Some(owner) => {
                    let mut required = (**owner).clone();
                    if required.requested == required.instance {
                        required.requested = Arc::clone(&display);
                    }
                    required.instance = Arc::clone(&display);
                    required
                }
                None => Instantiation {
                    at: method.function.at.location().into(),
                    requested: Arc::clone(&display),
                    instance: Arc::clone(&display),
                    parameters: Arc::default(),
                    unused_method: None,
                },
            };
            let required = Arc::new(Instantiation {
                unused_method: Some(display),
                ..required
            });
            self.demand_method(name, || unreachable!(), Some(&required));
        }
        !unused.is_empty()
    }

    /// Queues the unused instance methods that `compiled[start..]` names.
    /// Within one function, the earliest use in source order is reported.
    pub(super) fn demand_references(&mut self, start: usize) {
        if self.unused_methods.is_empty() {
            return;
        }
        let mut uses = Vec::new();
        for op in &self.compiled[start..] {
            let Op::DefineFn(caller, f) = op else {
                continue;
            };
            let first = uses.len();
            crate::threading::walk_sited(&f.body, &mut |op, site| {
                crate::threading::function_references(op, &mut |name| {
                    if self.unused_methods.contains_key(name) {
                        uses.push((site.map(|s| (s.line, s.column)), name.to_owned(), caller, f));
                    }
                });
            });
            // Positionless uses sort first and report the function itself.
            uses[first..].sort_by_key(|(site, ..)| *site);
        }
        let uses: Vec<_> = uses
            .into_iter()
            .map(|(site, name, caller, f)| {
                let at: Arc<str> = match (site, &f.origins.source) {
                    (Some((line, column)), Some(source)) => {
                        format!("{source}:{line}:{column}").into()
                    }
                    (Some((line, column)), None) => format!("{line}:{column}").into(),
                    (None, _) => f.location.clone().unwrap_or_else(|| caller.as_str().into()),
                };
                (name, at, f.origins.required.clone())
            })
            .collect();
        for (name, at, outer) in uses {
            self.demand_method(&name, || at, outer.as_ref());
        }
    }

    pub(super) fn reporting_protocol(&self) -> Option<&protocols::Protocol> {
        self.protocols
            .get("std.report.Formattable")
            .filter(|protocol| protocols::reporting(protocol, &self.standard_sources))
    }
    fn record_defaults(&mut self, f: &Function) {
        if f.defaults.iter().any(Option::is_some) {
            self.retention.keep_defaults(&f.defaults);
            self.defaults.insert(f.name.clone(), f.defaults.clone());
        }
    }
    /// Adds a concrete function to the table without queueing it.
    pub(super) fn record(
        &mut self,
        f: &Arc<Function>,
        table: &mut functions::Functions,
    ) -> functions::Symbol {
        self.record_defaults(f);
        let symbol = table.intern(&f.name);
        self.retention.record(symbol, f);
        table.declare(
            symbol,
            Declared {
                yields: generators::yields(&f.body),
                function: Some(Arc::clone(f)),
                lowered: false,
            },
        );
        symbol
    }

    /// Adds a concrete function to the table and to the lowering queue.
    pub(super) fn declare(&mut self, f: Function, table: &mut functions::Functions) {
        let f = Arc::new(f);
        let symbol = self.record(&f, table);
        self.pending.push_back((symbol, f));
    }

    /// `source` relative to the source root (unchanged without a root), and
    /// whether it is a standard library module.
    pub(super) fn source_facts(&mut self, source: &Arc<str>) -> (Arc<str>, bool) {
        if let Some((last, relative, standard)) = &self.source_facts
            && Arc::ptr_eq(last, source)
        {
            return (Arc::clone(relative), *standard);
        }
        let relative: Arc<str> = self
            .source_root
            .as_deref()
            .and_then(|root| std::path::Path::new(&**source).strip_prefix(root).ok())
            .map_or_else(
                || Arc::clone(source),
                |relative| relative.to_string_lossy().as_ref().into(),
            );
        let standard = self.standard_sources.contains(source);
        self.source_facts = Some((Arc::clone(source), Arc::clone(&relative), standard));
        (relative, standard)
    }
}

/// Type parameters with their replacements. A declaration has few type
/// parameters, so a scan beats allocating a hash table per instance and
/// hashing every name substitution probes (issue #428).
#[derive(Default)]
pub(super) struct Substitution(Vec<(String, TypeRef)>);

impl Substitution {
    pub(super) fn get(&self, name: &str) -> Option<&TypeRef> {
        self.0
            .iter()
            .find_map(|(parameter, t)| (parameter == name).then_some(t))
    }

    /// Adds a replacement, or replaces the one `name` has.
    pub(super) fn insert(&mut self, name: String, replacement: TypeRef) {
        match self.0.iter_mut().find(|(parameter, _)| *parameter == name) {
            Some((_, t)) => *t = replacement,
            None => self.0.push((name, replacement)),
        }
    }
}

impl FromIterator<(String, TypeRef)> for Substitution {
    fn from_iter<I: IntoIterator<Item = (String, TypeRef)>>(pairs: I) -> Self {
        let mut substitution = Self::default();
        for (name, replacement) in pairs {
            substitution.insert(name, replacement);
        }
        substitution
    }
}

/// Reify concrete types without parsing their display names or losing nominal identity.
pub(super) fn type_ref(ty: &Ty, at: &Token) -> TypeRef {
    let (name, children): (Option<String>, Vec<&Ty>) = match ty {
        Ty::Unit => (None, vec![]),
        Ty::Closure(_) | Ty::BoxedCallable(_) | Ty::Iter(..) => {
            return TypeRef {
                at: at.clone(),
                concrete: Some(Box::new(ty.clone())),
                parameter: false,
                name: None,
                args: vec![],
            };
        }
        Ty::Callable(sig) => (
            Some("Callable".into()),
            sig.inputs
                .iter()
                .chain(std::iter::once(sig.output.as_ref().unwrap_or(&Ty::Unit)))
                .collect(),
        ),
        Ty::List(t) => (Some("list".into()), vec![t]),
        Ty::Clist(t) => (Some("clist".into()), vec![t]),
        Ty::Future(t) => (Some("Future".into()), vec![t]),
        Ty::Channel(t, sender) => (
            Some(if *sender { "Sender" } else { "Receiver" }.into()),
            vec![t],
        ),
        Ty::Set(t) => (Some("set".into()), vec![t]),
        Ty::Box(t) => (Some("Box".into()), vec![t]),
        Ty::Shared(t) => (Some("Shared".into()), vec![t]),
        Ty::Dict(k, v) => (Some("dict".into()), vec![k, v]),
        Ty::Range(t) => (Some("range".into()), vec![t]),
        Ty::Generator(t) => (Some("Generator".into()), vec![&t.element]),
        Ty::Ref(t, mutable) => (Some(if *mutable { "&mut" } else { "&" }.into()), vec![t]),
        Ty::View(t, mutable) => (
            Some(if *mutable { MUT_VIEW_TYPE } else { VIEW_TYPE }.into()),
            vec![t],
        ),
        Ty::Enum(t) if t.is_option() => (
            Some("Option".into()),
            vec![&t.local().variants[1].fields[0]],
        ),
        Ty::Enum(t) if t.propagatable() => (
            Some("Result".into()),
            vec![
                &t.local().variants[0].fields[0],
                &t.local().variants[1].fields[0],
            ],
        ),
        Ty::Enum(t) if t.tuple() => (
            Some("tuple".into()),
            t.local().variants[0].fields.iter().collect(),
        ),
        _ => (Some(ty.to_string()), vec![]),
    };
    TypeRef {
        // Unit has no expression operand: resolving its type reference must
        // produce None, just like a source-level `()` annotation.
        concrete: (*ty != Ty::Unit).then(|| Box::new(ty.clone())),
        parameter: false,
        at: at.clone(),
        name,
        args: children.into_iter().map(|t| type_ref(t, at)).collect(),
    }
}

fn mentions_parameter(pattern: &TypeRef, template: &Function) -> bool {
    if pattern.concrete.is_some() {
        return false;
    }
    template
        .type_params
        .iter()
        .any(|(name, _)| pattern.name.as_ref() == Some(name))
        || pattern.args.iter().any(|p| mentions_parameter(p, template))
}

/// Infer from concrete argument types without searching for implementations.
fn infer(
    pattern: &TypeRef,
    actual: &Ty,
    template: &Function,
    inferred: &mut HashMap<String, Ty>,
    aliases: &TypeAliases,
    at: &Token,
) -> Result<()> {
    infer_inner(pattern, actual, template, inferred, aliases, at, true)
}

/// Check an argument against a parameter type that mentions no type parameter.
fn check_expected(expected: &Ty, actual: &Ty, coercible: bool, at: &Token) -> Result<()> {
    if crate::generator::refine(expected, actual).is_none()
        && !(coercible && crate::sum::accepts(expected, actual))
    {
        let hint = callables::mismatch_hint(expected, actual).unwrap_or_default();
        return Err(at.error(format!("expected {expected}, got {actual}{hint}")));
    }
    Ok(())
}

fn infer_inner(
    pattern: &TypeRef,
    actual: &Ty,
    template: &Function,
    inferred: &mut HashMap<String, Ty>,
    aliases: &TypeAliases,
    at: &Token,
    coercible: bool,
) -> Result<()> {
    if let Some(expected) = &pattern.concrete {
        return check_expected(expected, actual, coercible, at);
    }
    if let Some(name) = pattern
        .name
        .as_ref()
        .filter(|name| template.type_params.iter().any(|(n, _)| n == *name))
    {
        if !pattern.args.is_empty() {
            return Err(pattern
                .at
                .error("a type parameter cannot take type arguments"));
        }
        if let Some(previous) = inferred.get(name) {
            if previous != actual {
                return Err(at.error(format!("conflicting types for `{name}` in `{}`: {previous} and {actual}; use matching argument types or explicit type arguments", template.name)));
            }
        } else {
            inferred.insert(name.clone(), actual.clone());
        }
        return Ok(());
    }
    if !mentions_parameter(pattern, template) {
        let expected = pattern.resolve(aliases)?.unwrap_or(Ty::Unit);
        return check_expected(&expected, actual, coercible, at);
    }
    if pattern.name.as_deref() == Some("|") {
        let mut candidates = vec![inferred.clone()];
        for member in crate::sum::members(actual) {
            let mut next = Vec::new();
            for candidate in &candidates {
                for alternative in &pattern.args {
                    let mut attempt = candidate.clone();
                    if infer_inner(
                        alternative,
                        &member,
                        template,
                        &mut attempt,
                        aliases,
                        at,
                        false,
                    )
                    .is_ok()
                        && !next.contains(&attempt)
                    {
                        next.push(attempt);
                    }
                }
            }
            if next.len() > 64 {
                return Err(
                    at.error("ambiguous anonymous sum inference; supply explicit type arguments")
                );
            }
            candidates = next;
        }
        if candidates.len() != 1 {
            return Err(at.error(
                "ambiguous or incompatible anonymous sum inference; supply explicit type arguments",
            ));
        }
        *inferred = candidates.pop().unwrap();
        return Ok(());
    }
    if let Some((name, arguments)) = aliases.data.arguments(actual)
        && pattern.name.as_ref() == Some(&name)
        && pattern.args.len() == arguments.len()
    {
        for (pattern, argument) in pattern.args.iter().zip(&arguments) {
            infer_inner(pattern, argument, template, inferred, aliases, at, false)?;
        }
        return Ok(());
    }
    let children: Option<Vec<&Ty>> = match (pattern.name.as_deref(), actual) {
        (Some("Closure" | "OnceClosure"), Ty::Closure(t))
            if t.once == (pattern.name.as_deref() == Some("OnceClosure")) =>
        {
            Some(
                t.signature
                    .inputs
                    .iter()
                    .chain(std::iter::once(
                        t.signature.output.as_ref().unwrap_or(&Ty::Unit),
                    ))
                    .collect(),
            )
        }
        (Some("Callable"), Ty::Callable(sig)) => Some(
            sig.inputs
                .iter()
                .chain(std::iter::once(sig.output.as_ref().unwrap_or(&Ty::Unit)))
                .collect(),
        ),
        (Some("BoxedCallable"), Ty::BoxedCallable(sig)) => Some(
            sig.inputs
                .iter()
                .chain(std::iter::once(sig.output.as_ref().unwrap_or(&Ty::Unit)))
                .collect(),
        ),
        (Some("list"), Ty::List(t))
        | (Some("clist"), Ty::Clist(t))
        | (Some("set"), Ty::Set(t))
        | (Some("range"), Ty::Range(t))
        | (Some("Iter"), Ty::Iter(t, _)) => Some(vec![t]),
        (Some("Generator"), Ty::Generator(t)) => Some(vec![&t.element]),
        (Some("Future"), Ty::Future(t))
        | (Some("Box"), Ty::Box(t))
        | (Some("Shared"), Ty::Shared(t)) => Some(vec![t]),
        (Some("Sender"), Ty::Channel(t, true)) | (Some("Receiver"), Ty::Channel(t, false)) => {
            Some(vec![t])
        }
        (Some("dict"), Ty::Dict(k, v)) => Some(vec![k, v]),
        (Some("&"), Ty::Ref(t, false)) | (Some("&mut"), Ty::Ref(t, true)) => Some(vec![t]),
        (Some(VIEW_TYPE), Ty::View(t, false)) | (Some(MUT_VIEW_TYPE), Ty::View(t, true)) => {
            Some(vec![t])
        }
        (Some("Option"), Ty::Enum(t)) if t.is_option() => {
            Some(vec![&t.local().variants[1].fields[0]])
        }
        (Some("SpawnError"), Ty::Enum(t)) if t.name.starts_with("SpawnError[") => {
            Some(vec![&t.local().variants[0].fields[0]])
        }
        (Some("SendError"), Ty::Enum(t)) if t.name.starts_with("SendError[") => {
            Some(vec![&t.local().variants[0].fields[0]])
        }
        (Some("AppendError"), Ty::Enum(t)) if t.name.starts_with("AppendError[") => {
            Some(vec![&t.local().variants[0].fields[0]])
        }
        (Some("SendTimeoutError"), Ty::Enum(t)) if t.name.starts_with("SendTimeoutError[") => {
            Some(vec![&t.local().variants[0].fields[0]])
        }
        (Some("Selected"), Ty::Enum(t)) if t.name.starts_with("Selected[") => Some(vec![
            &t.local().variants[0].fields[0],
            &t.local().variants[1].fields[0],
        ]),
        (Some("SubmitError"), Ty::Enum(t)) if t.name.starts_with("SubmitError[") => {
            Some(vec![&t.local().variants[0].fields[0]])
        }
        (Some("ParallelError"), Ty::Enum(t)) if t.name.starts_with("ParallelError[") => {
            Some(vec![&t.local().variants[2].fields[0]])
        }
        (Some("Result"), Ty::Enum(t)) if t.propagatable() && !t.is_option() => Some(vec![
            &t.local().variants[0].fields[0],
            &t.local().variants[1].fields[0],
        ]),
        (Some("tuple"), Ty::Enum(t)) if t.tuple() => {
            Some(t.local().variants[0].fields.iter().collect())
        }
        _ => None,
    };
    let Some(children) = children.filter(|c| c.len() == pattern.args.len()) else {
        return Err(at.error(format!("argument type {actual} does not match parameter type in `{}`; supply matching arguments", template.name)));
    };
    for (index, (child, concrete)) in pattern.args.iter().zip(children).enumerate() {
        let child_coercible = coercible && index == 1 && pattern.name.as_deref() == Some("Result");
        infer_inner(
            child,
            concrete,
            template,
            inferred,
            aliases,
            at,
            child_coercible,
        )?;
    }
    Ok(())
}

/// A callable argument carries concrete signature evidence. Follow that evidence
/// to a fixed point; bounds never invent a type or search for an implementation.
fn infer_callable_bounds(
    template: &Function,
    inferred: &mut HashMap<String, Ty>,
    aliases: &TypeAliases,
    at: &Token,
) -> Result<()> {
    loop {
        let before = inferred.len();
        for (name, bound) in &template.type_params {
            let Some(bound) = bound
                .as_ref()
                .filter(|b| callable_bound(b) && mentions_parameter(b, template))
            else {
                continue;
            };
            let Some(signature) = inferred.get(name).and_then(callables::signature).cloned() else {
                continue;
            };
            infer(
                &callable_pattern(bound),
                &Ty::Callable(Arc::new(signature)),
                template,
                inferred,
                aliases,
                at,
            )?;
        }
        if before == inferred.len() {
            return Ok(());
        }
    }
}

impl Lower<'_> {
    pub(super) fn infer_function_value(
        &mut self,
        name: &str,
        signature: &crate::op::CallableSig,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let template = self.generics.templates[name].clone();
        if template.inputs.len() != signature.inputs.len() {
            return Err(at.error("generic function arity does not match the expected Callable"));
        }
        let mut inferred = HashMap::default();
        for ((_, pattern), actual) in template.inputs.iter().zip(&signature.inputs) {
            infer(pattern, actual, &template, &mut inferred, self.aliases, at)?;
        }
        infer(
            &template.output,
            signature.output.as_ref().unwrap_or(&Ty::Unit),
            &template,
            &mut inferred,
            self.aliases,
            at,
        )?;
        self.sync_data()?;
        self.generics
            .infer_bounds(&template, &mut inferred, self.aliases, self.access, at)?;
        let actual = template.type_params.iter().map(|(param, _)| {
            inferred.remove(param).ok_or_else(|| at.error(format!("cannot infer type parameter `{param}` for `{name}` from the expected Callable; supply explicit type arguments")))
        }).collect::<Result<_>>()?;
        let symbol = self.specialize(name, actual, at)?;
        self.function_value(&symbol, at, ops)
    }

    pub(super) fn specialize(&mut self, name: &str, actual: Vec<Ty>, at: &Token) -> Result<String> {
        if self.aliases.data.is_class(name) {
            modules::check_member(self.access, name, "__new__", at)?;
        }
        self.sync_data()?;
        let (symbol, function) =
            self.generics
                .instantiate(name, actual, at, self.aliases, self.access, self.symbol)?;
        if let Some(new) = function {
            let symbol_id = self.sigs.intern(&new.function.name);
            self.generics
                .specialization_depths
                .insert(symbol_id, new.depth);
            let instance: Arc<str> = crate::record::method_display(&new.display).into();
            let parameters = Arc::clone(&new.parameters);
            let required = Arc::new(match self.generics.required.get(&self.symbol) {
                Some(outer) => Instantiation {
                    instance,
                    parameters,
                    ..(**outer).clone()
                },
                None => Instantiation {
                    at: at.location().into(),
                    requested: instance.clone(),
                    instance,
                    parameters,
                    unused_method: None,
                },
            });
            self.generics.required.insert(symbol_id, required.clone());
            // The signature can name data instances that only this instance uses.
            let outer = self
                .aliases
                .data
                .attribute_within(|| data::Requiring::Instance(required.clone()));
            // Substitution changes only types, so the template body says
            // whether the instance yields and what reference it returns.
            let registered = register_with_body(
                symbol_id,
                &new.function,
                new.template_body(),
                None,
                self.aliases,
                self.sigs,
            );
            self.aliases.data.attribute(outer);
            registered?;
            if self.sigs[&symbol]
                .inputs
                .iter()
                .any(|(_, t)| t.unresolved_generator())
            {
                // Each frame specialization copies this declaration, which
                // therefore needs its body now, and it is never queued.
                let f = Arc::new(new.complete()?);
                self.generics.record(&f, self.sigs);
            } else {
                self.generics.defer_instance(new, self.sigs);
            }
        }
        Ok(symbol)
    }

    pub(super) fn generic_call(
        &mut self,
        name: &str,
        types: Option<&[TypeRef]>,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        self.generic_call_inner(name, types, args, at, false, ops)
    }

    pub(super) fn generic_method_call(
        &mut self,
        name: &str,
        types: Option<&[TypeRef]>,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        self.generic_call_inner(name, types, args, at, true, ops)
    }

    #[allow(clippy::too_many_arguments)]
    fn generic_call_inner(
        &mut self,
        name: &str,
        types: Option<&[TypeRef]>,
        args: &[Expr],
        at: &Token,
        method: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if self.names.contains_key(name) {
            return Err(at.error(format!("binding `{name}` is not callable")));
        }
        if let Some(types) = types {
            let actual = types
                .iter()
                .map(|t| Ok(t.resolve(self.aliases)?.unwrap_or(Ty::Unit)))
                .collect::<Result<_>>()?;
            let symbol = self.specialize(name, actual, at)?;
            return self.call_named_with_receiver(&symbol, args, at, method, ops);
        }
        if let Some((callee, arguments)) = self.prepare_named_call(name, args, at, method)? {
            return self.generic_call_inner(&callee, None, &arguments, at, method, ops);
        }
        let template = self.generics.templates[name].clone();
        if args.len() != template.inputs.len() {
            return Err(at.error(format!(
                "`{name}` expects {} arguments, got {}",
                template.inputs.len(),
                args.len()
            )));
        }
        if let Some(symbol) = self.lending_instance(name, &template, args, at)? {
            return self.call_named_with_receiver(&symbol, args, at, method, ops);
        }
        let mut inferred = HashMap::default();
        let mut loans = Vec::new();
        let mut arguments = Vec::new();
        let mut argument_ends = Vec::new();
        let start = ops.len();
        // Lower once, left to right, using ordinary ownership and borrow rules.
        // Generic positions get no contextual type: literals use their defaults.
        let mut reservation = None;
        for (index, (arg, (_, pattern))) in args.iter().zip(&template.inputs).enumerate() {
            let callback_bound = template.type_params.iter().find_map(|(name, bound)| {
                (pattern.name.as_ref() == Some(name))
                    .then_some(bound.as_ref())
                    .flatten()
                    .filter(|bound| callable_bound(bound) && !mentions_parameter(bound, &template))
            });
            let generic_function = matches!(&ungroup(arg).kind, Expression::Name(name)
                if !self.names.contains_key(name) && self.generics.templates.contains_key(name));
            let ty = if let Some(bound) = callback_bound.filter(|_| generic_function) {
                let expected = callable_pattern(bound).resolve(self.aliases)?;
                self.expr_expected(arg, expected, ops)?
                    .ok_or_else(|| arg.at.error("expected a callable value"))?
            } else if !mentions_parameter(pattern, &template)
                && !pattern
                    .resolve(self.aliases)?
                    .as_ref()
                    .is_some_and(Ty::unresolved_generator)
            {
                let expected = pattern
                    .resolve(self.aliases)?
                    .ok_or_else(|| pattern.at.error("unit parameters are not supported yet"))?;
                loans.extend(self.call_arguments(
                    std::slice::from_ref(arg),
                    &[(String::new(), expected.clone())],
                    ops,
                )?);
                expected
            } else if matches!(pattern.name.as_deref(), Some(VIEW_TYPE | MUT_VIEW_TYPE)) {
                let mutable = pattern.name.as_deref() == Some(MUT_VIEW_TYPE);
                let (ty, loan) = self.view_argument(arg, mutable, false, ops)?;
                if ty == text_view() {
                    return Err(arg.at.error(
                        "a `&[T]` parameter takes a list or buffer view; `&s[a:b]` is a `&substr`, which a `&substr` parameter takes",
                    ));
                }
                loans.push(loan);
                ty
            } else if matches!(pattern.name.as_deref(), Some("&" | "&mut")) {
                let (ty, loan) =
                    self.call_borrow(arg, pattern.name.as_deref() == Some("&mut"), None, ops)?;
                loans.push(loan);
                ty
            } else {
                self.value(arg, ops)?
            };
            infer(
                pattern,
                &ty,
                &template,
                &mut inferred,
                self.aliases,
                &arg.at,
            )?;
            arguments.push(ty);
            argument_ends.push(ops.len());
            if method && index == 0 && matches!(arguments[0], Ty::Ref(_, true)) {
                reservation = Some(loans[0]);
                self.reserve_receiver(loans[0], ops);
            }
        }
        self.sync_data()?;
        self.generics
            .infer_bounds(&template, &mut inferred, self.aliases, self.access, at)?;
        let actual = template.type_params.iter().map(|(param, _)| {
            inferred.remove(param).ok_or_else(|| at.error(format!("cannot infer type parameter `{param}` for `{name}` from its arguments; supply explicit type arguments")))
        }).collect::<Result<_>>()?;
        let symbol = self.specialize(name, actual, at)?;
        let callee = self.specialize_frames(&symbol, arguments.clone(), at)?;
        let sig = self.sigs[&callee].clone();
        for ((source, (_, target)), end) in
            arguments.iter().zip(&sig.inputs).zip(argument_ends).rev()
        {
            if source != target && crate::sum::accepts(target, source) {
                ops.insert(
                    end,
                    Op::Enum(crate::sum::EnumOp::Widen(
                        Box::new(source.clone()),
                        Box::new(target.clone()),
                    )),
                );
            }
        }
        if let Some(loan) = reservation {
            self.mark(at, ops);
            loans[0] = self.activate_receiver(loan, ops);
        }
        self.record_direct_call(&callee, at, &loans, ops);
        self.passes(start, &sig.inputs, sig.outputs.first(), ops);
        self.push_call(Op::Call(callee.clone()), &sig.inputs, &loans, ops);
        self.call_reference_result(&callee, &sig, &loans, ops);
        Self::end_reads(loans, ops);
        Ok(sig.outputs.first().cloned())
    }
}

impl Lower<'_> {
    /// The instance a call lending a pipeline to generic `name` names. Lending
    /// needs the borrower's concrete signature before any argument is lowered,
    /// so the type parameters come from the lent pipelines' item types alone.
    fn lending_instance(
        &mut self,
        name: &str,
        template: &Function,
        args: &[Expr],
        at: &Token,
    ) -> Result<Option<String>> {
        let mut inferred = HashMap::default();
        let mut lending = false;
        for (arg, (_, pattern)) in args.iter().zip(&template.inputs) {
            if pattern.name.as_deref() != Some("&mut")
                || pattern.args.first().and_then(|t| t.name.as_deref()) != Some("Iter")
            {
                continue;
            }
            if let Some(ty) = self.lendable_type(arg) {
                infer(pattern, &ty, template, &mut inferred, self.aliases, &arg.at)?;
                lending = true;
            }
        }
        if !lending {
            return Ok(None);
        }
        let actual = template
            .type_params
            .iter()
            .map(|(param, _)| {
                inferred.remove(param).ok_or_else(|| {
                    at.error(format!("cannot infer type parameter `{param}` for `{name}` from its lent pipelines; supply explicit type arguments"))
                })
            })
            .collect::<Result<_>>()?;
        self.specialize(name, actual, at).map(Some)
    }
}

/// `name[types]`, as diagnostics name an instance.
fn instance_name(name: &str, types: &[Ty]) -> String {
    let mut out = String::with_capacity(name.len() + 2 + 8 * types.len());
    out.push_str(name);
    out.push('[');
    for (i, ty) in types.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write!(out, "{ty}").unwrap();
    }
    out.push(']');
    out
}

pub(super) fn substitute(t: &mut TypeRef, replacements: &Substitution) -> Result<()> {
    if t.concrete.is_some() {
        return Ok(());
    }
    if let Some(replacement) = t.name.as_ref().and_then(|n| replacements.get(n)) {
        if !t.args.is_empty() {
            return Err(t.at.error("a type parameter cannot take type arguments"));
        }
        *t = replacement.clone();
        t.parameter = true;
    } else {
        for arg in &mut t.args {
            substitute(arg, replacements)?;
        }
    }
    Ok(())
}

impl Engine {
    /// Code lifted out of `parent`, or copied from it, is checked on behalf of
    /// the same use.
    pub(super) fn inherit_requirement(
        &mut self,
        table: &mut functions::Functions,
        parent: &str,
        child: &str,
    ) {
        if self.protocol_instances.contains(parent) {
            self.protocol_instances.insert(child.into());
        }
        let Some(parent) = table.symbol(parent) else {
            return;
        };
        let depth = self.specialization_depths.get(&parent).copied();
        let required = self.required.get(&parent).cloned();
        if depth.is_none() && required.is_none() {
            return;
        }
        let child = table.intern(child);
        if let Some(depth) = depth {
            self.specialization_depths.insert(child, depth);
        }
        if let Some(required) = required {
            self.required.insert(child, required);
        }
    }

    /// The use that first required `symbol`, if it is generic code.
    pub(super) fn requirement(&self, symbol: functions::Symbol) -> Option<&Arc<Instantiation>> {
        self.required.get(&symbol)
    }

    /// `requirement` by name, for the few callers without a symbol.
    pub(super) fn requirement_of(
        &self,
        table: &functions::Functions,
        name: &str,
    ) -> Option<&Arc<Instantiation>> {
        if self.required.is_empty() {
            return None;
        }
        self.requirement(table.symbol(name)?)
    }

    /// Scans every instance, so it is for diagnostics only: lowering takes a
    /// new instance's name from `instantiate`.
    pub(super) fn diagnostic_name(&self, symbol: &str) -> String {
        self.cache
            .iter()
            .map(|((id, types), instance)| (&*self.instantiated[*id as usize].0, types, instance))
            .chain(
                self.frames
                    .iter()
                    .map(|((name, types), instance)| (name.as_str(), types, instance)),
            )
            .find_map(|(name, types, instance)| {
                (instance == symbol).then(|| instance_name(name, types))
            })
            .unwrap_or_else(|| symbol.into())
    }

    fn infer_bounds(
        &self,
        template: &Function,
        inferred: &mut HashMap<String, Ty>,
        aliases: &TypeAliases,
        access: &modules::AccessMap,
        at: &Token,
    ) -> Result<()> {
        loop {
            let before = inferred.len();
            infer_callable_bounds(template, inferred, aliases, at)?;
            for (parameter, bound) in &template.type_params {
                let Some(bound) = bound.as_ref().filter(|b| mentions_parameter(b, template)) else {
                    continue;
                };
                let Some(protocol) = bound.name.as_ref().and_then(|n| self.protocols.get(n)) else {
                    continue;
                };
                let Some(actual) = inferred.get(parameter).cloned() else {
                    continue;
                };
                let Some(owner) = classes::method_owner(&actual) else {
                    continue;
                };
                let mut substitutions: Substitution = protocol
                    .type_params
                    .iter()
                    .map(|(n, _)| n.clone())
                    .zip(bound.args.iter().cloned())
                    .collect();
                substitutions.insert(protocol.name.clone(), type_ref(&actual, at));
                for requirement in &protocol.methods {
                    modules::check_member_at(access, owner, &requirement.name, &bound.at, at)?;
                    let (inputs, output) = self
                        .methods
                        .get(&crate::record::method(owner, &requirement.name))
                        .ok_or_else(|| {
                            at.error(format!(
                                "{actual} does not satisfy {}: missing method `{}`",
                                protocol.name, requirement.name
                            ))
                        })?;
                    if inputs.len() != requirement.inputs.len() {
                        return Err(at.error(format!(
                            "signature of `{}` does not match {}",
                            requirement.name, protocol.name
                        )));
                    }
                    for (pattern, found) in requirement
                        .inputs
                        .iter()
                        .map(|(_, t)| t)
                        .chain(std::iter::once(&requirement.output))
                        .zip(
                            inputs
                                .iter()
                                .chain(std::iter::once(output.as_ref().unwrap_or(&Ty::Unit))),
                        )
                    {
                        let mut pattern = pattern.clone();
                        substitute(&mut pattern, &substitutions)?;
                        let Err(error) = infer(&pattern, found, template, inferred, aliases, at)
                        else {
                            continue;
                        };
                        // A method type that no type arguments can fit, such
                        // as `-> ()` for `-> Result[(), E]`, fails the protocol
                        // as `protocols::check` reports it; the shape error is
                        // worded for call arguments. A type that fits alone
                        // but disagrees with other evidence keeps the error
                        // naming that conflict.
                        if mentions_parameter(&pattern, template)
                            && infer(
                                &pattern,
                                found,
                                template,
                                &mut HashMap::default(),
                                aliases,
                                at,
                            )
                            .is_err()
                        {
                            return Err(at.error(format!("{actual} does not satisfy {}: signature of `{}` does not match (including receiver borrowing, type parameters, and return type)", protocol.name, requirement.name)));
                        }
                        return Err(error);
                    }
                }
            }
            if before == inferred.len() {
                return Ok(());
            }
        }
    }

    pub(super) fn validate_template(&self, f: &Function, aliases: &TypeAliases) -> Result<()> {
        if f.type_params.iter().any(|(_, bound)| {
            bound
                .as_ref()
                .and_then(|b| b.name.as_ref())
                .is_some_and(|n| self.protocols.contains_key(n))
        }) {
            // Specialization erases protocol provenance, including aliases of
            // parameters. Until it is tracked, do not let an implementation's
            // parameter names accidentally become a protocol contract.
            positional_protocol_methods(&f.body)?;
        }
        for bound in f.type_params.iter().filter_map(|(_, b)| b.as_ref()) {
            if callable_bound(bound) {
                aliases.probe(f.type_params.iter().map(|(name, _)| name), |validation| {
                    callable_pattern(bound).resolve(validation).map(drop)
                })?;
            } else if let Some(protocol) = bound
                .name
                .as_ref()
                .and_then(|name| self.protocols.get(name))
            {
                if bound.args.len() != protocol.type_params.len() {
                    return Err(bound.at.error(format!(
                        "protocol `{}` requires {} type arguments",
                        protocol.name,
                        protocol.type_params.len()
                    )));
                }
            } else if numeric_bound(bound).is_none() || !bound.args.is_empty() {
                return Err(bound.at.error("generic constraints require IntType, FloatType, Callable, OnceCallable, or a declared protocol"));
            }
        }
        Ok(())
    }

    pub(super) fn explicit_output(
        &self,
        name: &str,
        types: &[TypeRef],
        aliases: &TypeAliases,
        at: &Token,
    ) -> Type {
        let template = self.templates.get(name)?;
        if types.len() != template.type_params.len() {
            return None;
        }
        let replacements = template
            .type_params
            .iter()
            .map(|(n, _)| n.clone())
            .zip(types.iter().cloned())
            .collect();
        let mut output = template.output.clone();
        substitute(&mut output, &replacements).ok()?;
        // The template's output is written at its declaration, but a data
        // instance it names for these arguments is required by this use.
        let outer = aliases
            .data
            .attribute_within(|| data::Requiring::Use(at.clone()));
        let output = output.resolve(aliases).ok().flatten();
        aliases.data.attribute(outer);
        output
    }

    /// The dependency depth of an instance first required by `parent`,
    /// checked against the depth limit.
    pub(super) fn dependency_depth(&self, parent: functions::Symbol, at: &Token) -> Result<usize> {
        let depth = self
            .specialization_depths
            .get(&parent)
            .copied()
            .unwrap_or(0)
            + 1;
        let limit = MAX_SPECIALIZATION_REPEATS + self.specializable;
        if depth > limit {
            return Err(at.error(format!(
                "specialization dependency depth limit of {limit} ({MAX_SPECIALIZATION_REPEATS} plus one per generic function or generator consumer) exceeded; check for expanding recursion"
            )));
        }
        Ok(depth)
    }

    pub(super) fn instantiate(
        &mut self,
        name: &str,
        actual: Vec<Ty>,
        at: &Token,
        aliases: &TypeAliases,
        access: &modules::AccessMap,
        parent: functions::Symbol,
    ) -> Result<(String, Option<Instance>)> {
        if self.retention.released(name) {
            return Err(at.error(format!(
                "internal error: `{name}` was released while a call to it remained"
            )));
        }
        let template = self
            .templates
            .get(name)
            .ok_or_else(|| at.error(format!("`{name}` is not a generic function")))?;
        if actual.len() != template.type_params.len() {
            return Err(at.error(format!(
                "generic function `{name}` requires {} type arguments",
                template.type_params.len()
            )));
        }
        let id = self.template_ids.get(name).copied();
        let mut key = (id.unwrap_or(u32::MAX), actual);
        if id.is_some()
            && let Some(symbol) = self.cache.get(&key)
        {
            return Ok((symbol.clone(), None));
        }
        // The cache keeps the type arguments, which callers collect with
        // spare capacity.
        key.1.shrink_to_fit();
        let actual = &key.1;
        let count = id.map_or(0, |id| self.instantiated[id as usize].1);
        let replacements: Substitution = template
            .type_params
            .iter()
            .map(|(n, _)| n.clone())
            .zip(actual.iter().map(|ty| type_ref(ty, at)))
            .collect();
        let mut demanded = Vec::new();
        for ((_, bound), ty) in template.type_params.iter().zip(actual) {
            if ty.key_view() {
                return Err(at.error("dictionary key views are local-only and cannot be user-function type arguments; materialise keys_snapshot() or to_set()"));
            }
            let borrowed_callable =
                matches!(ty, Ty::Closure(_)) && bound.as_ref().is_some_and(callable_bound);
            if ty.contains_reference() && !borrowed_callable {
                return Err(at.error(
                    "reference type arguments are not supported; borrow T in the signature",
                ));
            }
            if let Some(bound) = bound {
                if let Some(satisfies) = numeric_bound(bound) {
                    if !satisfies(ty) {
                        return Err(at.error(format!(
                            "{ty} does not satisfy {}",
                            bound.name.as_deref().unwrap()
                        )));
                    }
                } else if callable_bound(bound) {
                    check_callable_bound(bound, ty, &replacements, aliases, at)?;
                } else {
                    let mut application = bound.clone();
                    substitute(&mut application, &replacements)?;
                    let protocol = protocols::instantiate(
                        &self.protocols[bound.name.as_ref().unwrap()],
                        &application.args,
                        aliases,
                        at,
                    )?;
                    protocols::check(
                        &protocol,
                        ty,
                        &self.methods,
                        &self.templates,
                        aliases,
                        access,
                        &self.standard_sources,
                        &bound.at,
                        at,
                    )?;
                    // Satisfying a bound uses the methods it requires, whether
                    // or not this instance's body calls them.
                    if let Some(owner) = classes::method_owner(ty) {
                        demanded.extend(
                            protocol
                                .methods
                                .iter()
                                .map(|requirement| crate::record::method(owner, &requirement.name)),
                        );
                    }
                }
            }
        }
        // Depth bounds chains of new instances and the per-template count
        // bounds branching; neither counts unrelated templates, which only
        // the total resource budget does. Cache hits above consume no limit,
        // including recursive calls.
        let depth = self.dependency_depth(parent, at)?;
        if count >= MAX_TEMPLATE_INSTANCES {
            return Err(at.error(format!(
                "generic specialization limit of {MAX_TEMPLATE_INSTANCES} instances of `{}` exceeded; check for expanding recursion",
                crate::record::method_display(name)
            )));
        }
        if self.instances >= MAX_FUNCTION_INSTANCES {
            return Err(at.error(format!(
                "total generic function specialization budget of {MAX_FUNCTION_INSTANCES} exceeded"
            )));
        }
        let mut symbol = String::with_capacity(24);
        write!(symbol, "__plenty_generic_{}", self.instances).unwrap();
        if self.protocol_instances.contains(name)
            || template.type_params.iter().any(|(_, bound)| {
                bound
                    .as_ref()
                    .and_then(|b| b.name.as_ref())
                    .is_some_and(|n| self.protocols.contains_key(n))
            })
        {
            self.protocol_instances.insert(symbol.clone());
        }
        // Every field but the name and type parameters, which the instance
        // replaces, and the body, which `Instance` fills: cloning those here
        // would allocate only to free.
        let mut f = Function {
            once: template.once,
            captures: template.captures.clone(),
            foreign: template.foreign.clone(),
            export: template.export.clone(),
            name: symbol.clone(),
            type_params: Vec::new(),
            at: template.at.clone(),
            inputs: template.inputs.clone(),
            defaults: template.defaults.clone(),
            output: template.output.clone(),
            doc: template.doc.clone(),
            doc_at: template.doc_at.clone(),
            body: Vec::new(),
        };
        for (_, ty) in &mut f.inputs {
            substitute(ty, &replacements)?;
        }
        substitute(&mut f.output, &replacements)?;
        for default in f.defaults.iter_mut().flatten() {
            substitute_expr(default, &replacements)?;
        }
        let display = instance_name(name, actual);
        key.0 = match id {
            Some(id) => id,
            None => {
                let id = u32::try_from(self.instantiated.len())
                    .expect("instances are limited to fewer than 2^32");
                let name: Arc<str> = name.into();
                self.template_ids.insert(Arc::clone(&name), id);
                self.instantiated.push((name, 0));
                id
            }
        };
        self.instantiated[key.0 as usize].1 += 1;
        let parameters = template
            .type_params
            .iter()
            .map(|(parameter, _)| Arc::from(parameter.as_str()))
            .zip(key.1.iter().cloned())
            .collect();
        // Cache before queueing the body, so recursive calls share this instance.
        self.cache.insert(key, symbol.clone());
        self.instances += 1;
        let instance = Instance {
            function: f,
            parameters,
            template: Arc::clone(template),
            replacements,
            display,
            depth,
        };
        if !demanded.is_empty() {
            // A body error is attributed to the bounded instance, as if the
            // instance had called the method.
            let outer = self.required.get(&parent).cloned().unwrap_or_else(|| {
                let requested: Arc<str> = crate::record::method_display(&instance.display).into();
                Arc::new(Instantiation {
                    at: at.location().into(),
                    instance: Arc::clone(&requested),
                    requested,
                    parameters: Arc::clone(&instance.parameters),
                    unused_method: None,
                })
            });
            for callee in demanded {
                self.demand_method(&callee, || unreachable!(), Some(&outer));
            }
        }
        Ok((symbol, Some(instance)))
    }

    /// Records a new instance and queues it for lowering. Its body is
    /// substituted when it is lowered (`complete_instance`), not here: by
    /// then the template has usually been released, so the body is moved
    /// rather than copied (issue #436). The template handle stays with the
    /// instance until then.
    pub(super) fn defer_instance(
        &mut self,
        instance: Instance,
        table: &mut functions::Functions,
    ) -> functions::Symbol {
        let Instance {
            function,
            template,
            replacements,
            ..
        } = instance;
        let f = Arc::new(function);
        self.record_defaults(&f);
        let symbol = table.intern(&f.name);
        // The instance body names what the template body names.
        self.retention.instance(symbol, &template);
        table.declare(
            symbol,
            Declared {
                yields: generators::yields(&template.body),
                function: Some(Arc::clone(&f)),
                lowered: false,
            },
        );
        self.unsubstituted.insert(symbol, (template, replacements));
        self.pending.push_back((symbol, f));
        symbol
    }

    /// `f` with its body, if it is an instance queued by `defer_instance`.
    pub(super) fn complete_instance(
        &mut self,
        symbol: functions::Symbol,
        f: Arc<Function>,
        table: &mut functions::Functions,
    ) -> Result<Arc<Function>> {
        if self.unsubstituted.is_empty() {
            return Ok(f);
        }
        let Some((template, replacements)) = self.unsubstituted.remove(&symbol) else {
            return Ok(f);
        };
        let mut body =
            Arc::try_unwrap(template).map_or_else(|shared| shared.body.clone(), |t| t.body);
        substitute_block(&mut body, &replacements)?;
        let declared = table
            .declared_mut(symbol)
            .expect("a queued instance is declared");
        // Dropping the table's handle first leaves `f` unshared, unless the
        // instance is also still queued.
        declared.function = None;
        let mut function = Arc::unwrap_or_clone(f);
        function.body = body;
        let f = Arc::new(function);
        declared.function = Some(Arc::clone(&f));
        Ok(f)
    }
}

/// A new function instance whose body is still the template's.
pub(super) struct Instance {
    /// The instance declaration, without a body.
    pub(super) function: Function,
    template: Arc<Function>,
    replacements: Substitution,
    /// The instance name in diagnostics, such as `f[i64]`.
    pub(super) display: String,
    /// The template's type parameters and their arguments, for diagnostics.
    pub(super) parameters: Arc<[(Arc<str>, Ty)]>,
    /// Its dependency depth, which the caller records under its symbol.
    pub(super) depth: usize,
}

impl Instance {
    pub(super) fn template_body(&self) -> &[Stmt] {
        &self.template.body
    }

    /// The declaration with its body, copied from the template now.
    pub(super) fn complete(self) -> Result<Function> {
        let mut f = self.function;
        f.body = self.template.body.clone();
        substitute_block(&mut f.body, &self.replacements)?;
        Ok(f)
    }
}

fn positional_protocol_methods(body: &[Stmt]) -> Result<()> {
    fn expression(e: &Expr) -> Result<()> {
        match &e.kind {
            Expression::Method(base, _, args, _)
            | Expression::GenericMethod(base, _, _, args, _) => {
                if let Some(arg) = args
                    .iter()
                    .find(|arg| matches!(arg.kind, Expression::NamedArgument(..)))
                {
                    return Err(arg.at.error("named method arguments are not supported in protocol-constrained generic functions; use positional arguments"));
                }
                expression(base)?;
                for arg in args {
                    expression(arg)?;
                }
            }
            Expression::Call(_, args)
            | Expression::GenericCall(_, _, args)
            | Expression::Constructor(_, args)
            | Expression::Tuple(args) => {
                for arg in args {
                    expression(arg)?;
                }
            }
            Expression::Format(format) => {
                for arg in format.operands() {
                    expression(arg)?;
                }
            }
            Expression::Invoke(base, args) => {
                expression(base)?;
                for arg in args {
                    expression(arg)?;
                }
            }
            Expression::NamedArgument(_, inner)
            | Expression::Member(inner, _, _)
            | Expression::Group(inner)
            | Expression::Unary(_, inner)
            | Expression::Try(inner)
            | Expression::ClassReady(_, inner) => expression(inner)?,
            Expression::Index(a, b) | Expression::Binary(_, a, b) => {
                expression(a)?;
                expression(b)?;
            }
            Expression::Slice(base, bounds) => {
                expression(base)?;
                for bound in bounds.iter() {
                    expression(bound)?;
                }
            }
            Expression::Conditional { condition, yes, no } => {
                expression(condition)?;
                expression(yes)?;
                expression(no)?;
            }
            Expression::Anonymous(f) => positional_protocol_methods(&f.body)?,
            Expression::Match(value, cases) => {
                expression(value)?;
                for case in cases {
                    positional_protocol_methods(&case.body)?;
                }
            }
            Expression::Checked(body) => positional_protocol_methods(body)?,
            Expression::Collection {
                entries, clauses, ..
            } => {
                for (key, value) in entries {
                    expression(key)?;
                    if let Some(value) = value {
                        expression(value)?;
                    }
                }
                for clause in clauses {
                    match clause {
                        Clause::For(_, value) | Clause::If(value) => expression(value)?,
                    }
                }
            }
            Expression::Ambiguous { generic, index } => {
                expression(generic)?;
                expression(index)?;
            }
            Expression::GenericValue(..)
            | Expression::ClassNew(..)
            | Expression::Type(..)
            | Expression::Number(..)
            | Expression::Text(..)
            | Expression::Bytes(..)
            | Expression::Bool(..)
            | Expression::Unit
            | Expression::Name(..) => {}
        }
        Ok(())
    }
    for stmt in body {
        match &stmt.kind {
            Statement::Assign { value, .. }
            | Statement::Expr(value)
            | Statement::Yield(value)
            | Statement::Unpack { value, .. } => expression(value)?,
            Statement::Return(value) => {
                if let Some(value) = value {
                    expression(value)?;
                }
            }
            Statement::SetIndex { target, value } | Statement::Augment { target, value, .. } => {
                expression(target)?;
                expression(value)?;
            }
            Statement::If {
                condition,
                yes,
                elifs,
                no,
            } => {
                expression(condition)?;
                positional_protocol_methods(yes)?;
                for elif in elifs {
                    expression(&elif.condition)?;
                    positional_protocol_methods(&elif.body)?;
                }
                positional_protocol_methods(no)?;
            }
            Statement::While { condition, body } => {
                expression(condition)?;
                positional_protocol_methods(body)?;
            }
            Statement::For { iterable, body, .. } => {
                expression(iterable)?;
                positional_protocol_methods(body)?;
            }
            Statement::With { manager, body, .. } => {
                expression(manager)?;
                positional_protocol_methods(body)?;
            }
            Statement::Match { value, cases } => {
                expression(value)?;
                for case in cases {
                    positional_protocol_methods(&case.body)?;
                }
            }
            Statement::Pass | Statement::Break | Statement::Continue => {}
        }
    }
    Ok(())
}

pub(super) fn substitute_block(body: &mut [Stmt], substitutions: &Substitution) -> Result<()> {
    for stmt in body {
        match &mut stmt.kind {
            Statement::Assign {
                annotation, value, ..
            } => {
                if let Some(t) = annotation {
                    substitute(t, substitutions)?;
                }
                substitute_expr(value, substitutions)?;
            }
            Statement::Expr(e) | Statement::Yield(e) | Statement::Unpack { value: e, .. } => {
                substitute_expr(e, substitutions)?
            }
            Statement::Return(e) => {
                if let Some(e) = e {
                    substitute_expr(e, substitutions)?;
                }
            }
            Statement::SetIndex { target, value } | Statement::Augment { target, value, .. } => {
                substitute_expr(value, substitutions)?;
                substitute_expr(target, substitutions)?;
            }
            Statement::If {
                condition,
                yes,
                elifs,
                no,
            } => {
                substitute_expr(condition, substitutions)?;
                substitute_block(yes, substitutions)?;
                for elif in elifs {
                    substitute_expr(&mut elif.condition, substitutions)?;
                    substitute_block(&mut elif.body, substitutions)?;
                }
                substitute_block(no, substitutions)?;
            }
            Statement::While { condition, body } => {
                substitute_expr(condition, substitutions)?;
                substitute_block(body, substitutions)?;
            }
            Statement::For { iterable, body, .. } => {
                substitute_expr(iterable, substitutions)?;
                substitute_block(body, substitutions)?;
            }
            Statement::With { manager, body, .. } => {
                substitute_expr(manager, substitutions)?;
                substitute_block(body, substitutions)?;
            }
            Statement::Match { value, cases } => substitute_match(value, cases, substitutions)?,
            Statement::Pass | Statement::Break | Statement::Continue => {}
        }
    }
    Ok(())
}
fn substitute_match(
    value: &mut Expr,
    cases: &mut [enums::Case],
    substitutions: &Substitution,
) -> Result<()> {
    substitute_expr(value, substitutions)?;
    for case in cases {
        case.pattern
            .qualifiers_mut(false, &mut |t| substitute(t, substitutions))?;
        substitute_block(&mut case.body, substitutions)?;
    }
    Ok(())
}
pub(super) fn substitute_expr(e: &mut Expr, substitutions: &Substitution) -> Result<()> {
    match &mut e.kind {
        Expression::Match(value, cases) => substitute_match(value, cases, substitutions)?,
        Expression::Checked(body) => substitute_block(body, substitutions)?,

        Expression::GenericMethod(base, _, types, args, _) => {
            substitute_expr(base, substitutions)?;
            for ty in types {
                substitute(ty, substitutions)?;
            }
            for arg in args {
                substitute_expr(arg, substitutions)?;
            }
        }
        Expression::Anonymous(function) => {
            for (_, ty) in &mut function.inputs {
                substitute(ty, substitutions)?;
            }
            substitute(&mut function.output, substitutions)?;
            substitute_block(&mut function.body, substitutions)?;
        }
        Expression::GenericValue(_, types) => {
            for ty in types {
                substitute(ty, substitutions)?;
            }
        }
        Expression::GenericCall(_, types, args) => {
            for t in types.iter_mut() {
                substitute(t, substitutions)?;
            }
            for arg in args.iter_mut() {
                substitute_expr(arg, substitutions)?;
            }
        }
        Expression::Call(name, args) => {
            for arg in args.iter_mut() {
                substitute_expr(arg, substitutions)?;
            }
            if let Some(t) = substitutions.get(name) {
                e.kind = Expression::Constructor(Box::new(t.clone()), std::mem::take(args));
            }
        }
        Expression::Type(t) => substitute(t, substitutions)?,
        Expression::Constructor(t, args) => {
            substitute(t, substitutions)?;
            for arg in args {
                substitute_expr(arg, substitutions)?;
            }
        }
        Expression::Tuple(args) => {
            for arg in args {
                substitute_expr(arg, substitutions)?;
            }
        }
        Expression::Format(format) => {
            for arg in format.operands_mut() {
                substitute_expr(arg, substitutions)?;
            }
        }
        Expression::Method(base, _, args, _) | Expression::Invoke(base, args) => {
            substitute_expr(base, substitutions)?;
            for arg in args {
                substitute_expr(arg, substitutions)?;
            }
        }
        Expression::Member(base, _, _)
        | Expression::NamedArgument(_, base)
        | Expression::Unary(_, base)
        | Expression::Group(base)
        | Expression::Try(base)
        | Expression::ClassReady(_, base) => substitute_expr(base, substitutions)?,
        Expression::Index(a, b) | Expression::Binary(_, a, b) => {
            substitute_expr(a, substitutions)?;
            substitute_expr(b, substitutions)?;
        }
        Expression::Slice(base, bounds) => {
            substitute_expr(base, substitutions)?;
            for bound in bounds.iter_mut() {
                substitute_expr(bound, substitutions)?;
            }
        }
        Expression::Conditional { condition, yes, no } => {
            substitute_expr(condition, substitutions)?;
            substitute_expr(yes, substitutions)?;
            substitute_expr(no, substitutions)?;
        }
        Expression::Collection {
            entries, clauses, ..
        } => {
            for clause in clauses {
                match clause {
                    Clause::For(_, e) | Clause::If(e) => substitute_expr(e, substitutions)?,
                }
            }
            for (k, v) in entries {
                substitute_expr(k, substitutions)?;
                if let Some(v) = v {
                    substitute_expr(v, substitutions)?;
                }
            }
        }
        Expression::Name(n) => {
            if let Some(t) = substitutions.get(n) {
                e.kind = Expression::Type(Box::new(t.clone()));
            }
        }
        Expression::Number(_)
        | Expression::Text(_)
        | Expression::Bytes(_)
        | Expression::Bool(_)
        | Expression::Unit
        | Expression::ClassNew(..) => {}
        Expression::Ambiguous { .. } => unreachable!("resolved with module names"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_generator_calls_share_consumer_specializations_and_resume_bodies() {
        let mut source = String::from(
            "def values() -> Generator[i64]:\n    yield 1\ndef factory() -> Generator[i64]:\n    values()\ndef consume(source: Generator[i64]) -> ():\n    drop(source)\ndef main() -> ():\n",
        );
        for _ in 0..100 {
            source.push_str("    consume(factory())\n    consume(values())\n");
        }
        source.push_str("    pass\n");
        let program = compile(&source, &mut Heap::default()).unwrap();
        let functions: Vec<_> = program
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::DefineFn(name, f) => Some((name, f)),
                _ => None,
            })
            .collect();
        assert_eq!(
            functions
                .iter()
                .filter(|(name, _)| name.starts_with("__plenty_frame_call_"))
                .count(),
            1
        );
        assert_eq!(
            functions
                .iter()
                .filter(|(_, f)| f.generator.is_some())
                .count(),
            1
        );
        assert_eq!(
            functions
                .iter()
                .filter(|(name, _)| *name == "factory")
                .count(),
            1
        );
    }

    #[test]
    fn explicit_inferred_alias_and_recursive_calls_share_one_specialization() {
        let mut source = String::from(
            "type Count = i64\ndef down[T: IntType](x: T) -> T:\n    if x == 0:\n        return x\n    down(x - 1)\ndef main() -> ():\n",
        );
        for _ in 0..100 {
            source.push_str("    down(3)\n    down[i64](3)\n    down[Count](3)\n");
        }
        source.push_str("    pass\n");
        let program = compile(&source, &mut Heap::default()).unwrap();
        let instances = program
            .ops
            .iter()
            .filter(
                |op| matches!(op, Op::DefineFn(name, _) if name.starts_with("__plenty_generic_")),
            )
            .count();
        assert_eq!(instances, 1);
    }

    /// The whole syntax tree is live when lowering starts, statements hold
    /// their expressions inline, and every node holds a token (issues #54,
    /// #422). Box a payload that would raise these bounds.
    #[test]
    fn syntax_nodes_stay_small() {
        assert!(std::mem::size_of::<Token>() <= 40);
        assert!(std::mem::size_of::<Expr>() <= 120);
        assert!(std::mem::size_of::<Stmt>() <= 344);
    }
}
