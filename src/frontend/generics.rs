//! Cached monomorphization shared by explicit and inferred calls during lowering.
use super::*;
use std::collections::VecDeque;

pub(super) fn prepare(
    functions: Vec<Function>,
    aliases: &TypeAliases,
    protocols: Vec<protocols::Protocol>,
) -> Result<Engine> {
    let mut declarations = HashMap::new();
    for protocol in &protocols {
        if aliases.contains_key(&protocol.name)
            || functions.iter().any(|f| f.name == protocol.name)
            || declarations
                .insert(protocol.name.clone(), protocol.clone())
                .is_some()
        {
            return Err(protocol.at.error(format!(
                "protocol `{}` is already defined or conflicts with another declaration",
                protocol.name
            )));
        }
        // Validate requirements even if no function uses this protocol. The
        // receiver placeholder is replaced by the implementing class at use.
        let mut validation = aliases.clone();
        validation.insert(protocol.name.clone(), Some(Ty::I64));
        for method in &protocol.methods {
            protocols::signature(method, &validation)?;
        }
    }
    let mut engine = Engine {
        protocols: declarations,
        methods: functions
            .iter()
            .filter(|f| f.type_params.is_empty())
            .map(|f| Ok((f.name.clone(), protocols::signature(f, aliases)?)))
            .collect::<Result<_>>()?,
        templates: HashMap::new(),
        cache: HashMap::new(),
        pending: VecDeque::new(),
        functions: HashMap::new(),
        compiled: Vec::new(),
        active: HashSet::new(),
        completed: HashSet::new(),
        frames: HashMap::new(),
    };
    let mut names = HashSet::new();
    for f in functions {
        if !names.insert(f.name.clone()) || aliases.contains_key(&f.name) {
            return Err(f.at.error(format!(
                "function `{}` is already defined or conflicts with a type alias",
                f.name
            )));
        }
        if f.type_params.is_empty() {
            engine.functions.insert(f.name.clone(), f.clone());
            engine.pending.push_back(f);
        } else {
            if f.name == "main" {
                return Err(f.at.error("main cannot have type parameters"));
            }
            for (_, bound) in &f.type_params {
                if let Some(bound) = bound {
                    if (bound.name.as_deref() != Some("IntType")
                        && !bound
                            .name
                            .as_ref()
                            .is_some_and(|n| engine.protocols.contains_key(n)))
                        || !bound.args.is_empty()
                    {
                        return Err(bound
                            .at
                            .error("generic constraints require IntType or a declared protocol"));
                    }
                }
            }
            engine.templates.insert(f.name.clone(), Rc::new(f));
        }
    }
    Ok(engine)
}

pub(super) struct Engine {
    protocols: HashMap<String, protocols::Protocol>,
    methods: HashMap<String, (Vec<Ty>, Type)>,
    pub(super) templates: HashMap<String, Rc<Function>>,
    cache: HashMap<(String, Vec<Ty>), String>,
    pub(super) pending: VecDeque<Function>,
    pub(super) functions: HashMap<String, Function>,
    pub(super) compiled: Vec<Op>,
    pub(super) active: HashSet<String>,
    pub(super) completed: HashSet<String>,
    pub(super) frames: HashMap<(String, Vec<Ty>), String>,
}

type Substitution = HashMap<String, TypeRef>;

/// Reify concrete types without parsing their display names or losing nominal identity.
pub(super) fn type_ref(ty: &Ty, at: &Token) -> TypeRef {
    let (name, children): (Option<String>, Vec<&Ty>) = match ty {
        Ty::Unit => (None, vec![]),
        Ty::Callable(sig) => (
            Some("Callable".into()),
            sig.inputs
                .iter()
                .chain(std::iter::once(sig.output.as_ref().unwrap_or(&Ty::Unit)))
                .collect(),
        ),
        Ty::List(t) => (Some("list".into()), vec![t]),
        Ty::Set(t) => (Some("set".into()), vec![t]),
        Ty::Dict(k, v) => (Some("dict".into()), vec![k, v]),
        Ty::Range(t) => (Some("range".into()), vec![t]),
        Ty::Generator(t) => (Some("Generator".into()), vec![&t.element]),
        Ty::Ref(t, mutable) => (Some(if *mutable { "&mut" } else { "&" }.into()), vec![t]),
        Ty::Enum(t) if t.is_option() => (Some("Option".into()), vec![&t.variants[1].fields[0]]),
        Ty::Enum(t) if t.propagatable() => (
            Some("Result".into()),
            vec![&t.variants[0].fields[0], &t.variants[1].fields[0]],
        ),
        Ty::Enum(t) if t.tuple() => (Some("tuple".into()), t.variants[0].fields.iter().collect()),
        _ => (Some(ty.to_string()), vec![]),
    };
    TypeRef {
        concrete: Some(ty.clone()),
        at: at.clone(),
        name,
        args: children.into_iter().map(|t| type_ref(t, at)).collect(),
    }
}

fn mentions_parameter(pattern: &TypeRef, template: &Function) -> bool {
    template
        .type_params
        .iter()
        .any(|(name, _)| pattern.name.as_ref() == Some(name))
        || pattern.args.iter().any(|p| mentions_parameter(p, template))
}

/// Infer only from concrete argument types; bounds never select a candidate type.
fn infer(
    pattern: &TypeRef,
    actual: &Ty,
    template: &Function,
    inferred: &mut HashMap<String, Ty>,
    aliases: &TypeAliases,
    at: &Token,
) -> Result<()> {
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
        if crate::generator::refine(&expected, actual).is_none() {
            return Err(at.error(format!("expected {expected}, got {actual}")));
        }
        return Ok(());
    }
    let children: Option<Vec<&Ty>> = match (pattern.name.as_deref(), actual) {
        (Some("Callable"), Ty::Callable(sig)) => Some(
            sig.inputs
                .iter()
                .chain(std::iter::once(sig.output.as_ref().unwrap_or(&Ty::Unit)))
                .collect(),
        ),
        (Some("list"), Ty::List(t)) | (Some("set"), Ty::Set(t)) | (Some("range"), Ty::Range(t)) => {
            Some(vec![t])
        }
        (Some("Generator"), Ty::Generator(t)) => Some(vec![&t.element]),
        (Some("dict"), Ty::Dict(k, v)) => Some(vec![k, v]),
        (Some("&"), Ty::Ref(t, false)) | (Some("&mut"), Ty::Ref(t, true)) => Some(vec![t]),
        (Some("Option"), Ty::Enum(t)) if t.is_option() => Some(vec![&t.variants[1].fields[0]]),
        (Some("Result"), Ty::Enum(t)) if t.propagatable() && !t.is_option() => {
            Some(vec![&t.variants[0].fields[0], &t.variants[1].fields[0]])
        }
        (Some("tuple"), Ty::Enum(t)) if t.tuple() => Some(t.variants[0].fields.iter().collect()),
        _ => None,
    };
    let Some(children) = children.filter(|c| c.len() == pattern.args.len()) else {
        return Err(at.error(format!("argument type {actual} does not match parameter type in `{}`; supply matching arguments", template.name)));
    };
    for (pattern, concrete) in pattern.args.iter().zip(children) {
        infer(pattern, concrete, template, inferred, aliases, at)?;
    }
    Ok(())
}

impl Lower<'_> {
    pub(super) fn specialize(&mut self, name: &str, actual: Vec<Ty>, at: &Token) -> Result<String> {
        let (symbol, function) =
            self.generics
                .instantiate(name, actual, at, self.aliases, self.access)?;
        if let Some(f) = function {
            register_signature(&f, self.aliases, self.sigs, self.returned_fields)?;
            self.generics.functions.insert(f.name.clone(), f.clone());
            if !self.sigs[&f.name]
                .inputs
                .iter()
                .any(|(_, t)| t.unresolved_generator())
            {
                self.generics.pending.push_back(f);
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
        if self.names.contains_key(name) {
            return Err(at.error(format!("binding `{name}` is not callable")));
        }
        if let Some(types) = types {
            let actual = types
                .iter()
                .map(|t| {
                    t.resolve(self.aliases)?
                        .ok_or_else(|| t.at.error("unit type arguments are not supported yet"))
                })
                .collect::<Result<_>>()?;
            let symbol = self.specialize(name, actual, at)?;
            return self.call_named(&symbol, args, at, ops);
        }
        let template = self.generics.templates[name].clone();
        if args.len() != template.inputs.len() {
            return Err(at.error(format!(
                "`{name}` expects {} arguments, got {}",
                template.inputs.len(),
                args.len()
            )));
        }
        let mut inferred = HashMap::new();
        let mut loans = Vec::new();
        let mut arguments = Vec::new();
        // Lower once, left to right, using ordinary ownership and borrow rules.
        // Generic positions get no contextual type: literals use their defaults.
        for (arg, (_, pattern)) in args.iter().zip(&template.inputs) {
            let ty = if !mentions_parameter(pattern, &template)
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
            } else if matches!(pattern.name.as_deref(), Some("&" | "&mut")) {
                let (ty, loan) =
                    self.call_borrow(arg, pattern.name.as_deref() == Some("&mut"), ops)?;
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
        }
        let actual = template.type_params.iter().map(|(param, _)| {
            inferred.remove(param).ok_or_else(|| at.error(format!("cannot infer type parameter `{param}` for `{name}` from its arguments; supply explicit type arguments")))
        }).collect::<Result<_>>()?;
        let symbol = self.specialize(name, actual, at)?;
        let callee = self.specialize_frames(&symbol, arguments, at)?;
        let sig = self.sigs[&callee].clone();
        ops.push(Op::Call(callee.clone()));
        self.call_reference_result(&callee, &sig, &loans, ops);
        Self::end_reads(loans, ops);
        Ok(sig.outputs.first().cloned())
    }
}

fn substitute(t: &mut TypeRef, replacements: &Substitution) -> Result<()> {
    if let Some(replacement) = t.name.as_ref().and_then(|n| replacements.get(n)) {
        if !t.args.is_empty() {
            return Err(t.at.error("a type parameter cannot take type arguments"));
        }
        *t = replacement.clone();
    } else {
        for arg in &mut t.args {
            substitute(arg, replacements)?;
        }
    }
    Ok(())
}

impl Engine {
    pub(super) fn explicit_output(
        &self,
        name: &str,
        types: &[TypeRef],
        aliases: &TypeAliases,
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
        output.resolve(aliases).ok().flatten()
    }

    pub(super) fn instantiate(
        &mut self,
        name: &str,
        actual: Vec<Ty>,
        at: &Token,
        aliases: &TypeAliases,
        access: &modules::AccessMap,
    ) -> Result<(String, Option<Function>)> {
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
        let key = (name.to_owned(), actual.clone());
        if let Some(symbol) = self.cache.get(&key) {
            return Ok((symbol.clone(), None));
        }
        for ((_, bound), ty) in template.type_params.iter().zip(&actual) {
            if *ty == Ty::Unit {
                return Err(at.error("unit type arguments are not supported yet"));
            }
            if ty.contains_reference() {
                return Err(at.error(
                    "reference type arguments are not supported; borrow T in the signature",
                ));
            }
            if let Some(bound) = bound {
                if bound.name.as_deref() == Some("IntType") {
                    if !ty.is_int() {
                        return Err(at.error(format!("{ty} does not satisfy IntType")));
                    }
                } else {
                    protocols::check(
                        &self.protocols[bound.name.as_ref().unwrap()],
                        ty,
                        &self.methods,
                        aliases,
                        access,
                        &bound.at,
                        at,
                    )?;
                }
            }
        }
        if self.cache.len() >= 256 {
            return Err(at.error(
                "generic specialization limit of 256 exceeded; check for expanding recursion",
            ));
        }
        let symbol = format!("__plenty_generic_{}", self.cache.len());
        let mut f = (**template).clone();
        let replacements = f
            .type_params
            .iter()
            .map(|(n, _)| n.clone())
            .zip(actual.iter().map(|ty| type_ref(ty, at)))
            .collect();
        f.name = symbol.clone();
        f.type_params.clear();
        for (_, ty) in &mut f.inputs {
            substitute(ty, &replacements)?;
        }
        substitute(&mut f.output, &replacements)?;
        // Cache before queueing the body, so recursive calls share this instance.
        self.cache.insert(key, symbol.clone());
        substitute_block(&mut f.body, &replacements)?;
        Ok((symbol, Some(f)))
    }
}

fn substitute_block(body: &mut [Stmt], substitutions: &Substitution) -> Result<()> {
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
            Statement::SetIndex { target, value } => {
                substitute_expr(value, substitutions)?;
                substitute_expr(target, substitutions)?;
            }
            Statement::If { condition, yes, no } => {
                substitute_expr(condition, substitutions)?;
                substitute_block(yes, substitutions)?;
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
            Statement::Match { value, cases } => {
                substitute_expr(value, substitutions)?;
                for case in cases {
                    if let Some((Some(t), _, _)) = &mut case.pattern {
                        substitute(t, substitutions)?;
                    }
                    substitute_block(&mut case.body, substitutions)?;
                }
            }
            Statement::Pass | Statement::Break | Statement::Continue => {}
        }
    }
    Ok(())
}
fn substitute_expr(e: &mut Expr, substitutions: &Substitution) -> Result<()> {
    match &mut e.kind {
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
                if t.args.is_empty() {
                    *name = t.name.clone().unwrap();
                } else {
                    e.kind = Expression::Constructor(t.clone(), std::mem::take(args));
                }
            }
        }
        Expression::Type(t) => substitute(t, substitutions)?,
        Expression::Constructor(t, args) => {
            substitute(t, substitutions)?;
            for arg in args {
                substitute_expr(arg, substitutions)?;
            }
        }
        Expression::Tuple(args, _) => {
            for arg in args {
                substitute_expr(arg, substitutions)?;
            }
        }
        Expression::Method(base, _, args) | Expression::Invoke(base, args) => {
            substitute_expr(base, substitutions)?;
            for arg in args {
                substitute_expr(arg, substitutions)?;
            }
        }
        Expression::Member(base, _)
        | Expression::Unary(_, base)
        | Expression::Group(base)
        | Expression::Try(base)
        | Expression::ClassReady(_, base) => substitute_expr(base, substitutions)?,
        Expression::Index(a, b) | Expression::Binary(_, a, b) => {
            substitute_expr(a, substitutions)?;
            substitute_expr(b, substitutions)?;
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
                e.kind = Expression::Type(t.clone());
            }
        }
        Expression::Number(_)
        | Expression::Text(_)
        | Expression::Bool(_)
        | Expression::Unit
        | Expression::ClassNew(..) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_generator_calls_share_consumer_specializations_and_resume_bodies() {
        let mut source = String::from("def values() -> Generator[i64]:\n    yield 1\ndef factory() -> Generator[i64]:\n    values()\ndef consume(source: Generator[i64]) -> ():\n    drop(source)\ndef main() -> ():\n");
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
        let mut source = String::from("type Count = i64\ndef down[T: IntType](x: T) -> T:\n    if x == 0:\n        return x\n    down(x - 1)\ndef main() -> ():\n");
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
}
