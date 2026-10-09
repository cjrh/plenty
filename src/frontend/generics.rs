//! Cached monomorphization shared by explicit and inferred calls during lowering.
use super::*;
use std::collections::VecDeque;

pub(super) fn callable_bound(bound: &TypeRef) -> bool {
    matches!(bound.name.as_deref(), Some("Callable" | "OnceCallable"))
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
        return Err(at.error(format!("{ty} does not satisfy {requirement}")));
    }
    Ok(())
}

pub(super) fn prepare(
    mut functions: Vec<Function>,
    aliases: &TypeAliases,
    protocols: Vec<protocols::Protocol>,
) -> Result<Engine> {
    aliases.validate_data_bounds()?;
    functions.extend(aliases.data.factories());
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
        let mut validation = aliases.validation();
        validation.insert(protocol.name.clone(), Some(Ty::I64));
        for (name, _) in &protocol.type_params {
            validation.insert(name.clone(), Some(Ty::I64));
        }
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
            engine.validate_template(&f, aliases)?;
            engine.templates.insert(f.name.clone(), Rc::new(f));
        }
    }
    Ok(engine)
}

pub(super) struct Engine {
    protocols: HashMap<String, protocols::Protocol>,
    pub(super) methods: HashMap<String, (Vec<Ty>, Type)>,
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
        Ty::Closure(_) => {
            return TypeRef {
                at: at.clone(),
                concrete: Some(ty.clone()),
                name: None,
                args: vec![],
            }
        }
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
        concrete: Some(ty.clone()),
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
    if let Some(expected) = &pattern.concrete {
        if crate::generator::refine(expected, actual).is_none() {
            return Err(at.error(format!("expected {expected}, got {actual}")));
        }
        return Ok(());
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
        if crate::generator::refine(&expected, actual).is_none() {
            return Err(at.error(format!("expected {expected}, got {actual}")));
        }
        return Ok(());
    }
    if let Some((name, arguments)) = aliases.data.arguments(actual) {
        if pattern.name.as_ref() == Some(&name) && pattern.args.len() == arguments.len() {
            for (pattern, argument) in pattern.args.iter().zip(&arguments) {
                infer(pattern, argument, template, inferred, aliases, at)?;
            }
            return Ok(());
        }
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
        (Some("list"), Ty::List(t)) | (Some("set"), Ty::Set(t)) | (Some("range"), Ty::Range(t)) => {
            Some(vec![t])
        }
        (Some("Generator"), Ty::Generator(t)) => Some(vec![&t.element]),
        (Some("dict"), Ty::Dict(k, v)) => Some(vec![k, v]),
        (Some("&"), Ty::Ref(t, false)) | (Some("&mut"), Ty::Ref(t, true)) => Some(vec![t]),
        (Some("Option"), Ty::Enum(t)) if t.is_option() => {
            Some(vec![&t.local().variants[1].fields[0]])
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
    for (pattern, concrete) in pattern.args.iter().zip(children) {
        infer(pattern, concrete, template, inferred, aliases, at)?;
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
                &Ty::Callable(Rc::new(signature)),
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
        let mut inferred = HashMap::new();
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
        self.sync_data()?;
        self.generics
            .infer_bounds(&template, &mut inferred, self.aliases, self.access, at)?;
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

pub(super) fn substitute(t: &mut TypeRef, replacements: &Substitution) -> Result<()> {
    if t.concrete.is_some() {
        return Ok(());
    }
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
                let Some(actual @ Ty::Class(_)) = inferred.get(parameter).cloned() else {
                    continue;
                };
                let Ty::Class(class) = &actual else {
                    unreachable!()
                };
                let mut substitutions: Substitution = protocol
                    .type_params
                    .iter()
                    .map(|(n, _)| n.clone())
                    .zip(bound.args.iter().cloned())
                    .collect();
                substitutions.insert(protocol.name.clone(), type_ref(&actual, at));
                for requirement in &protocol.methods {
                    modules::check_member(access, &class.name, &requirement.name, &bound.at)?;
                    let (inputs, output) = self
                        .methods
                        .get(&crate::record::method(&class.name, &requirement.name))
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
                    for (pattern, actual) in requirement
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
                        infer(&pattern, actual, template, inferred, aliases, at)?;
                    }
                }
            }
            if before == inferred.len() {
                return Ok(());
            }
        }
    }

    pub(super) fn validate_template(&self, f: &Function, aliases: &TypeAliases) -> Result<()> {
        for bound in f.type_params.iter().filter_map(|(_, b)| b.as_ref()) {
            if callable_bound(bound) && !bound.args.is_empty() {
                let mut validation = aliases.validation();
                for (name, _) in &f.type_params {
                    validation.insert(name.clone(), Some(Ty::I64));
                }
                callable_pattern(bound).resolve(&validation)?;
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
            } else if (bound.name.as_deref() != Some("IntType")
                && !bound
                    .name
                    .as_ref()
                    .is_some_and(|n| self.protocols.contains_key(n)))
                || !bound.args.is_empty()
            {
                return Err(bound.at.error("generic constraints require IntType, Callable, OnceCallable, or a declared protocol"));
            }
        }
        Ok(())
    }

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
        let replacements: Substitution = template
            .type_params
            .iter()
            .map(|(n, _)| n.clone())
            .zip(actual.iter().map(|ty| type_ref(ty, at)))
            .collect();
        for ((_, bound), ty) in template.type_params.iter().zip(&actual) {
            if *ty == Ty::Unit {
                return Err(at.error("unit type arguments are not supported yet"));
            }
            let borrowed_callable =
                matches!(ty, Ty::Closure(_)) && bound.as_ref().is_some_and(callable_bound);
            if ty.contains_reference() && !borrowed_callable {
                return Err(at.error(
                    "reference type arguments are not supported; borrow T in the signature",
                ));
            }
            if let Some(bound) = bound {
                if bound.name.as_deref() == Some("IntType") {
                    if !ty.is_int() {
                        return Err(at.error(format!("{ty} does not satisfy IntType")));
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
                    protocols::check(&protocol, ty, &self.methods, aliases, access, &bound.at, at)?;
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
        Expression::GenericMethod(base, _, types, args) => {
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
                e.kind = Expression::Constructor(t.clone(), std::mem::take(args));
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
