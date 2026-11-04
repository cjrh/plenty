//! Structural method requirements, checked before generic specialization.
use super::*;

#[derive(Clone)]
pub(super) struct Protocol {
    pub(super) name: String,
    pub(super) at: Token,
    pub(super) type_params: Vec<(String, Option<TypeRef>)>,
    pub(super) methods: Vec<Function>,
    pub(super) arguments: Vec<Ty>,
}

impl Parser {
    pub(super) fn protocol_decl(&mut self) -> Result<Protocol> {
        let at = self.take();
        let name = self.name()?;
        let type_params = self.type_parameters()?;
        if type_params.iter().any(|(_, bound)| bound.is_some()) {
            return Err(at.error("protocol parameters do not take constraints; constrain the implementing function's parameters"));
        }
        if type_params.iter().any(|(n, _)| n == &name) {
            return Err(at.error("a protocol parameter cannot shadow its declaration name"));
        }
        self.expect(":")?;
        self.kind(Kind::Newline, "a newline after `:`")?;
        self.kind(Kind::Indent, "an indented protocol declaration")?;
        let mut methods = Vec::new();
        let mut seen = HashSet::default();
        while self.peek().kind != Kind::Dedent && self.peek().kind != Kind::Eof {
            self.misplaced_annotations("protocol members")?;
            if !self.peek().is("def") {
                return Err(self
                    .peek()
                    .error("protocols contain method signatures with pass bodies"));
            }
            let mut f = self.function_in(Some(&name))?;
            if f.defaults.iter().any(Option::is_some) {
                return Err(f
                    .at
                    .error("protocol requirements cannot declare parameter defaults"));
            }
            // Method-local parameters, such as `def map[U: Display](...)`,
            // must be declared alike by every implementation.
            for (parameter, bound) in &f.type_params {
                if parameter == &name || type_params.iter().any(|(n, _)| n == parameter) {
                    return Err(f.at.error(
                        "a requirement parameter cannot shadow its protocol or the protocol's parameters",
                    ));
                }
                if bound.as_ref().is_some_and(|b| {
                    generics::callable_bound(b) || generics::builtin_bound(b).is_some()
                }) {
                    return Err(f
                        .at
                        .error("requirement parameters take only protocol constraints"));
                }
            }
            if !seen.insert(f.name.clone()) {
                return Err(f.at.error("duplicate protocol method"));
            }
            if matches!(f.name.as_str(), "__init__" | "__del__" | "__new__") {
                return Err(f
                    .at
                    .error("lifecycle hooks cannot be protocol requirements"));
            }
            super::operators::reject_unsupported_hook(&f)?;
            if !matches!(
                f.body.as_slice(),
                [Stmt {
                    kind: Statement::Pass,
                    ..
                }]
            ) {
                return Err(f.at.error("protocol methods require a pass body; default implementations are not supported"));
            }
            let receiver = f
                .inputs
                .first()
                .filter(|(n, _)| n == "self")
                .map(|(_, t)| t);
            if !receiver.is_some_and(|t| {
                let owner = if matches!(t.name.as_deref(), Some("&" | "&mut")) {
                    &t.args[0]
                } else {
                    t
                };
                owner.name.as_deref() == Some(&name)
                    && (owner.args.is_empty()
                        || (owner.args.len() == type_params.len()
                            && owner.args.iter().zip(&type_params).all(|(t, (name, _))| {
                                t.name.as_ref() == Some(name) && t.args.is_empty()
                            })))
            }) {
                return Err(f.at.error(
                    "protocol methods require self: Protocol, self: &Protocol, or self: &mut Protocol",
                ));
            }
            classes::receiver_owner_mut(&mut f.inputs[0].1).args.clear();
            methods.push(f);
        }
        self.kind(Kind::Dedent, "the end of the protocol")?;
        if methods.is_empty() {
            return Err(at.error("a protocol requires at least one method"));
        }
        Ok(Protocol {
            name,
            at,
            type_params,
            methods,
            arguments: vec![],
        })
    }
}

pub(super) fn signature(f: &Function, aliases: &TypeAliases) -> Result<generics::Signature> {
    // Sized exactly: collecting through `Result` reserves room for at least
    // four types, and `generics::prepare` keeps every function's types until
    // they are registered.
    let mut inputs = Vec::with_capacity(f.inputs.len());
    for (_, t) in &f.inputs {
        inputs.push(
            t.resolve(aliases)?
                .ok_or_else(|| t.at.error("unit parameters are not supported yet"))?,
        );
    }
    Ok((inputs, f.output.resolve(aliases)?))
}

pub(super) fn instantiate(
    protocol: &Protocol,
    arguments: &[TypeRef],
    aliases: &TypeAliases,
    at: &Token,
) -> Result<Protocol> {
    if arguments.len() != protocol.type_params.len() {
        return Err(at.error(format!(
            "protocol `{}` requires {} type arguments",
            protocol.name,
            protocol.type_params.len()
        )));
    }
    let substitutions = protocol
        .type_params
        .iter()
        .map(|(n, _)| n.clone())
        .zip(
            arguments
                .iter()
                .map(|arg| {
                    let ty = arg
                        .resolve(aliases)?
                        .ok_or_else(|| arg.at.error("unit protocol arguments are not supported"))?;
                    if ty.restricted_storage() {
                        return Err(arg.at.error(
                            "protocol arguments cannot contain references, generators, or closures",
                        ));
                    }
                    Ok(generics::type_ref(&ty, &arg.at))
                })
                .collect::<Result<Vec<_>>>()?,
        )
        .collect();
    let mut concrete = protocol.clone();
    concrete.type_params.clear();
    concrete.arguments = arguments
        .iter()
        .map(|a| a.resolve(aliases).map(Option::unwrap))
        .collect::<Result<_>>()?;
    for method in &mut concrete.methods {
        for bound in method
            .type_params
            .iter_mut()
            .filter_map(|(_, b)| b.as_mut())
        {
            generics::substitute(bound, &substitutions)?;
        }
        for (_, ty) in &mut method.inputs {
            generics::substitute(ty, &substitutions)?;
        }
        generics::substitute(&mut method.output, &substitutions)?;
    }
    Ok(concrete)
}

/// The methods a built-in type brings to protocols, as `(name, signature)`.
/// `bytes` has `len`, so it satisfies a protocol requiring `len`.
fn builtin_methods(ty: &Ty) -> Option<Vec<(&'static str, generics::Signature)>> {
    match ty {
        Ty::ByteStr => Some(vec![(
            "len",
            (
                vec![Ty::Ref(std::sync::Arc::new(Ty::ByteStr), false)],
                Some(Ty::I64),
            ),
        )]),
        _ => None,
    }
}

/// A type no program can name, standing for one requirement parameter.
fn placeholder(name: &str) -> Ty {
    Ty::Class(crate::nominal::Nominal::new(crate::record::ClassType {
        name: format!("<requirement parameter {name}>"),
        fields: vec![],
        destructor: None,
        fallible_init: false,
        copy: false,
        forwards_operators: false,
        redacted: false,
        quantity: None,
        bytes: std::sync::OnceLock::new(),
        managed: std::sync::OnceLock::new(),
    }))
}

fn same_shape(a: &TypeRef, b: &TypeRef) -> bool {
    a.name == b.name
        && a.concrete == b.concrete
        && a.args.len() == b.args.len()
        && a.args.iter().zip(&b.args).all(|(a, b)| same_shape(a, b))
}

/// Whether the generic `method` declares `requirement`'s parameters, with the
/// same constraints, and its signature once they are renamed. Both resolve in
/// a shape probe, so the placeholders never become program types.
fn same_generic(
    requirement: &Function,
    method: &Function,
    implementing: (&str, &Ty),
    aliases: &TypeAliases,
) -> Result<bool> {
    if requirement.type_params.len() != method.type_params.len() {
        return Ok(false);
    }
    let renamed: generics::Substitution = method
        .type_params
        .iter()
        .zip(&requirement.type_params)
        .map(|((own, _), (name, _))| {
            let reference = TypeRef {
                concrete: None,
                metadata: None,
                at: method.at.clone(),
                name: Some(name.clone()),
                args: vec![],
            };
            (own.clone(), reference)
        })
        .collect();
    for ((_, own), (_, expected)) in method.type_params.iter().zip(&requirement.type_params) {
        match (own, expected) {
            (None, None) => {}
            (Some(own), Some(expected)) => {
                let mut own = own.clone();
                generics::substitute(&mut own, &renamed)?;
                if !same_shape(&own, expected) {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
    }
    let mut own = method.clone();
    for (_, ty) in &mut own.inputs {
        generics::substitute(ty, &renamed)?;
    }
    generics::substitute(&mut own.output, &renamed)?;
    let placeholders: Vec<_> = requirement
        .type_params
        .iter()
        .map(|(name, _)| (name, placeholder(name)))
        .collect();
    let protocol = implementing.0.to_owned();
    aliases.probe_bound(
        std::iter::once((&protocol, implementing.1.clone())).chain(placeholders),
        |probe| Ok(signature(&own, probe)? == signature(requirement, probe)?),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn check(
    protocol: &Protocol,
    actual: &Ty,
    methods: &HashMap<String, generics::Signature>,
    templates: &HashMap<String, std::sync::Arc<Function>>,
    aliases: &TypeAliases,
    access: &modules::AccessMap,
    standard_sources: &HashSet<Arc<str>>,
    bound: &Token,
    call: &Token,
) -> Result<()> {
    if let Ty::BoxedProtocol(boxed) = actual {
        return if boxed.name == protocol.name && boxed.arguments == protocol.arguments {
            Ok(())
        } else {
            Err(call.error(format!(
                "{actual} does not satisfy {}: erased protocol identity differs",
                protocol.name
            )))
        };
    }
    if protocol
        .methods
        .iter()
        .any(|method| method.name == "format")
        && let Some((cause, required)) = aliases.data.report_context(actual, standard_sources)
    {
        // A matching signature alone cannot advertise Context.format for
        // an unsupported cause. This also applies to user protocols with
        // the same method, including bounds whose body never calls it.
        check(
            &required,
            &cause,
            methods,
            templates,
            aliases,
            access,
            standard_sources,
            &required.at,
            call,
        )?;
    }
    if matches!(actual, Ty::Enum(sum) if sum.anonymous()) {
        if !reporting(protocol, standard_sources) {
            return Err(call.error(format!(
                "{actual} does not satisfy {}; anonymous sums only forward std.fmt.Formattable",
                protocol.name
            )));
        }
        // The forwarding method belongs to the reporting contract, so its
        // payload calls must be visible from that library even for a local bound.
        for member in crate::sum::members(actual) {
            check(
                protocol,
                &member,
                methods,
                templates,
                aliases,
                access,
                standard_sources,
                &protocol.at,
                call,
            )?;
        }
        return Ok(());
    }
    let mismatch = |name: &str| {
        call.error(format!("{actual} does not satisfy {}: signature of `{name}` does not match (including receiver borrowing, type parameters, and return type)", protocol.name))
    };
    let missing = |name: &str| {
        call.error(format!(
            "{actual} does not satisfy {}: missing method `{name}`",
            protocol.name
        ))
    };
    let mut aliases = aliases.clone();
    aliases.insert(protocol.name.clone(), Some(actual.clone()));
    if let Some(builtin) = builtin_methods(actual) {
        for requirement in &protocol.methods {
            let name = &requirement.name;
            let method = builtin
                .iter()
                .find(|(n, _)| n == name)
                .ok_or_else(|| missing(name))?;
            if !requirement.type_params.is_empty() || method.1 != signature(requirement, &aliases)?
            {
                return Err(mismatch(name));
            }
        }
        return Ok(());
    }
    let Some(owner) = classes::method_owner(actual) else {
        return Err(call.error(format!(
            "{actual} does not satisfy {}; protocols currently require classes, enums, or `bytes`",
            protocol.name
        )));
    };
    for requirement in &protocol.methods {
        let name = &requirement.name;
        let key = crate::record::method(owner, name);
        if requirement.type_params.is_empty() {
            let method = methods.get(&key).ok_or_else(|| {
                if templates.contains_key(&key) {
                    mismatch(name)
                } else {
                    missing(name)
                }
            })?;
            modules::check_member_at(access, owner, name, bound, call)?;
            if *method != signature(requirement, &aliases)? {
                return Err(mismatch(name));
            }
        } else {
            let method = templates.get(&key).ok_or_else(|| {
                if methods.contains_key(&key) {
                    mismatch(name)
                } else {
                    missing(name)
                }
            })?;
            modules::check_member_at(access, owner, name, bound, call)?;
            if !same_generic(requirement, method, (&protocol.name, actual), &aliases)? {
                return Err(mismatch(name));
            }
        }
    }
    Ok(())
}

pub(super) fn reporting(protocol: &Protocol, standard_sources: &HashSet<Arc<str>>) -> bool {
    protocol.name == "std.fmt.Formattable"
        && protocol
            .at
            .source()
            .as_ref()
            .is_some_and(|source| standard_sources.contains(source))
}

/// Resolve only the explicit erasure boundary; protocols remain non-value types.
pub(super) fn boxed_type(
    reference: &TypeRef,
    aliases: &TypeAliases,
) -> Result<Option<Arc<crate::boxed::ProtocolType>>> {
    // Generic and implementing-type substitutions shadow declarations during
    // static signature checks. Box[P] there is an ordinary concrete box.
    if reference
        .name
        .as_ref()
        .is_some_and(|name| aliases.contains_key(name))
    {
        return Ok(None);
    }
    let Some(declaration) = reference
        .name
        .as_ref()
        .and_then(|name| aliases.protocols.get(name))
    else {
        return Ok(None);
    };
    let key = reference.spelling();
    {
        let mut resolving = aliases.resolving_protocols.borrow_mut();
        if resolving.contains(&key) || resolving.len() >= 64 {
            return Err(reference
                .at
                .error("recursive boxed protocol signatures are not supported"));
        }
        resolving.push(key);
    }
    let result = resolve_boxed_type(reference, aliases, declaration);
    aliases.resolving_protocols.borrow_mut().pop();
    result
}

fn contains_implementing_type(ty: &Ty, marker: &Ty, aliases: &TypeAliases) -> bool {
    let mut pending = vec![ty.clone()];
    let mut seen = Vec::new();
    while let Some(ty) = pending.pop() {
        if &ty == marker {
            return true;
        }
        if seen.contains(&ty) {
            continue;
        }
        if let Some((_, arguments)) = aliases.data.arguments(&ty) {
            pending.extend(arguments);
        }
        match &ty {
            Ty::Callable(sig) | Ty::BoxedCallable(sig) => {
                pending.extend(sig.inputs.iter().chain(sig.output.iter()).cloned())
            }
            Ty::BoxedProtocol(p) => pending.extend(p.types().cloned()),
            // Nominal arguments above preserve even phantom occurrences; do
            // not inspect a declaration whose layout may still be deferred.
            Ty::Class(_) => {}
            Ty::Enum(t) if t.try_get().is_none() => {}
            _ => crate::threading::contained(&ty, &mut pending),
        }
        seen.push(ty);
    }
    false
}

fn resolve_boxed_type(
    reference: &TypeRef,
    aliases: &TypeAliases,
    declaration: &Protocol,
) -> Result<Option<Arc<crate::boxed::ProtocolType>>> {
    let protocol = instantiate(declaration, &reference.args, aliases, &reference.at)?;
    let marker = placeholder(&protocol.name);
    let mut scope = aliases.clone();
    scope.insert(protocol.name.clone(), Some(marker.clone()));
    let mut methods = Vec::new();
    for method in &protocol.methods {
        let invalid = |reason: &str| {
            reference.at.error(format!(
                "cannot box protocol `{}`: method `{}` {reason}",
                protocol.name, method.name
            ))
        };
        if !method.type_params.is_empty() {
            return Err(invalid("has method-local type parameters"));
        }
        fn mentions(ty: &TypeRef, name: &str) -> bool {
            ty.name.as_deref() == Some(name) || ty.args.iter().any(|arg| mentions(arg, name))
        }
        if method.inputs[1..]
            .iter()
            .map(|(_, ty)| ty)
            .chain(std::iter::once(&method.output))
            .any(|ty| mentions(ty, &protocol.name))
        {
            return Err(invalid("uses the implementing type outside its receiver"));
        }
        let (inputs, output) = signature(method, &scope)?;
        let Some(Ty::Ref(receiver, mutable)) = inputs.first() else {
            return Err(invalid("has a consuming receiver"));
        };
        if receiver.as_ref() != &marker {
            return Err(invalid("has an invalid receiver"));
        }
        // Resolve aliases before searching, so nested implementing-type uses
        // cannot be mistaken for an ordinary fixed method argument.
        if inputs[1..]
            .iter()
            .chain(output.iter())
            .any(|ty| contains_implementing_type(ty, &marker, aliases))
        {
            return Err(invalid("uses the implementing type outside its receiver"));
        }
        if output.as_ref().is_some_and(Ty::contains_reference) {
            return Err(invalid("returns a reference or view"));
        }
        if inputs.len() > 256 {
            return Err(invalid("exceeds the 255 argument limit"));
        }
        let signature = crate::op::CallableSig {
            inputs: inputs[1..].to_vec(),
            output,
        };
        callables::validate(&signature, &reference.at)?;
        methods.push(crate::boxed::ProtocolMethod {
            name: method.name.clone(),
            mutable: *mutable,
            signature,
        });
    }
    Ok(Some(Arc::new(crate::boxed::ProtocolType {
        name: protocol.name,
        arguments: reference
            .args
            .iter()
            .map(|arg| arg.resolve(aliases).map(Option::unwrap))
            .collect::<Result<_>>()?,
        methods,
    })))
}

impl Lower<'_> {
    pub(super) fn protocol_box(
        &mut self,
        types: &[TypeRef],
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let [reference] = types else {
            return Err(at.error("Box requires one protocol type argument"));
        };
        let protocol = boxed_type(reference, self.aliases)?.ok_or_else(|| {
            at.error(
                "explicit Box[P](value) requires a protocol; use Box(value) for a concrete value",
            )
        })?;
        let [value] = args else {
            return Err(at.error("Box[P] takes one owned concrete value"));
        };
        let source = self.value(value, ops)?;
        if !matches!(&source, Ty::Class(_) | Ty::ByteStr)
            && !matches!(&source, Ty::Enum(t) if !t.anonymous())
        {
            return Err(at.error("Box[P] requires a concrete class, enum, or bytes value; box the concrete owner directly"));
        }
        if let Some(barrier) = source.storage_barrier() {
            return Err(at.error(barrier.message("a protocol box")));
        }
        if crate::boxed::erasure_barrier(&source) {
            return Err(at.error(
                "protocol boxes cannot conceal executors, futures, or erased callback captures",
            ));
        }
        self.sync_data()?;
        let declaration = &self.aliases.protocols[&protocol.name];
        let required = instantiate(declaration, &reference.args, self.aliases, at)?;
        check(
            &required,
            &source,
            &self.generics.methods,
            &self.generics.templates,
            self.aliases,
            self.access,
            &self.generics.standard_sources,
            &declaration.at,
            at,
        )?;
        let mut adapters = Vec::new();
        for (index, method) in protocol.methods.iter().enumerate() {
            let adapter = format!(
                "__plenty_protocol_{}_{}_{}",
                source.identity_name(),
                Ty::BoxedProtocol(protocol.clone()).identity_name(),
                index
            );
            if !self.sigs.contains_key(&adapter) {
                let mut sig = method.signature.function();
                sig.inputs.insert(
                    0,
                    (
                        "self".into(),
                        Ty::Ref(Arc::new(source.clone()), method.mutable),
                    ),
                );
                let mut body = vec![Op::LoadLocal(0)];
                body.extend(method.signature.inputs.iter().enumerate().map(|(i, ty)| {
                    if ty.affine() {
                        Op::MoveLocal((i + 1) as u8)
                    } else {
                        Op::LoadLocal((i + 1) as u8)
                    }
                }));
                if source == Ty::ByteStr {
                    body.push(Op::ReadRef(Ty::ByteStr));
                    body.push(Op::Collection(CollectionOp::Len(Ty::ByteStr)));
                } else {
                    let callee = crate::record::method(
                        classes::method_owner(&source).unwrap(),
                        &method.name,
                    );
                    self.ensure_concrete_output(&callee, at)?;
                    self.generics
                        .demand_method(&callee, || Some(at.position()), None);
                    body.push(Op::Call(callee));
                }
                body.push(Op::Return);
                let sig = Arc::new(sig);
                self.sigs.insert(adapter.clone(), sig.clone());
                self.generics.compiled.push(Op::DefineFn(
                    adapter.clone(),
                    Box::new(CompiledFn {
                        location: Some(at.position()),
                        origins: Arc::default(),
                        generator: None,
                        sig,
                        doc: "".into(),
                        body: body.into(),
                        locals: Arc::default(),
                    }),
                ));
            }
            adapters.push(adapter);
        }
        let output = crate::sum::result(
            Ty::BoxedProtocol(protocol.clone()),
            crate::sum::alloc_error(),
        );
        ops.push(Op::ProtocolBoxNew(Box::new(source), adapters, protocol));
        Ok(output)
    }

    pub(super) fn protocol_method(
        &mut self,
        base: &Expr,
        protocol: Arc<crate::boxed::ProtocolType>,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        super::ide::method_call(&base.at, name, &format!("{}.{name}", protocol.name));
        let (index, method) = protocol
            .methods
            .iter()
            .enumerate()
            .find(|(_, m)| m.name == name)
            .ok_or_else(|| base.at.error(format!("{protocol} has no method `{name}`")))?;
        if args.len() != method.signature.inputs.len() {
            return Err(base.at.error(format!(
                "method `{name}` expects {} arguments",
                method.signature.inputs.len()
            )));
        }
        let mut sig = method.signature.function();
        sig.inputs.insert(
            0,
            (
                "self".into(),
                Ty::Ref(
                    Arc::new(Ty::BoxedProtocol(protocol.clone())),
                    method.mutable,
                ),
            ),
        );
        let start = ops.len();
        let (_, loan) = self.borrow(base, method.mutable, ops)?;
        if method.mutable {
            self.reserve_receiver(loan, ops);
        }
        let mut loans = vec![loan];
        loans.extend(self.call_arguments(args, &sig.inputs[1..], ops)?);
        if method.mutable {
            self.mark(&base.at, ops);
            loans[0] = self.activate_receiver(loan, ops);
        }
        self.mark_call(&base.at, ops);
        self.passes(start, &sig.inputs, sig.outputs.first(), ops);
        ops.push(Op::ProtocolBoxCall(protocol.clone(), index));
        Self::end_reads(loans, ops);
        Ok(method.signature.output.clone())
    }
}

impl Lower<'_> {
    pub(super) fn temporary_protocol_method(
        &mut self,
        base: &Expr,
        protocol: Arc<crate::boxed::ProtocolType>,
        name: &str,
        args: &[Expr],
        loans: Vec<usize>,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if protocol
            .methods
            .iter()
            .any(|method| method.name == name && method.mutable)
        {
            return Err(base
                .at
                .error("mutable protocol methods require a named binding or mutable place"));
        }
        let held = match ops.as_slice() {
            [.., Op::StoreLocal(a), Op::LoadLocal(b)]
                if a == b && self.expression_temps.last() == Some(a) =>
            {
                Some(*a)
            }
            _ => None,
        };
        let ty = Ty::BoxedProtocol(protocol.clone());
        let slot = if let Some(slot) = held {
            ops.pop();
            self.expression_temps.pop();
            slot
        } else {
            let slot = self.slot(ty.clone(), &base.at)?;
            ops.push(Op::StoreLocal(slot));
            slot
        };
        let name_local = format!("__plenty_protocol_receiver_{slot}");
        self.bind(
            name_local.clone(),
            Local {
                slot,
                ty,
                mutable: false,
            },
        );
        let expr = Expr {
            at: base.at.clone(),
            kind: Expression::Name(name_local.clone()),
        };
        let result = self.protocol_method(&expr, protocol, name, args, ops)?;
        self.names.remove(&name_local);
        self.expression_temps.push(slot);
        Self::end_reads(loans, ops);
        Ok(result)
    }
}
