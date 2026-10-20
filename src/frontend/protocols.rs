//! Structural method requirements, checked before generic specialization.
use super::*;

#[derive(Clone)]
pub(super) struct Protocol {
    pub(super) name: String,
    pub(super) at: Token,
    pub(super) type_params: Vec<(String, Option<TypeRef>)>,
    pub(super) methods: Vec<Function>,
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
            // Method-local parameters, such as `def write[S: ByteSource](...)`,
            // must be declared alike by every implementation.
            for (parameter, bound) in &f.type_params {
                if parameter == &name || type_params.iter().any(|(n, _)| n == parameter) {
                    return Err(f.at.error(
                        "a requirement parameter cannot shadow its protocol or the protocol's parameters",
                    ));
                }
                if bound.as_ref().is_some_and(|b| {
                    generics::callable_bound(b) || generics::numeric_bound(b).is_some()
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
                matches!(t.name.as_deref(), Some("&" | "&mut"))
                    && t.args.len() == 1
                    && t.args[0].name.as_deref() == Some(&name)
                    && (t.args[0].args.is_empty()
                        || (t.args[0].args.len() == type_params.len()
                            && t.args[0]
                                .args
                                .iter()
                                .zip(&type_params)
                                .all(|(t, (name, _))| {
                                    t.name.as_ref() == Some(name) && t.args.is_empty()
                                })))
            }) {
                return Err(f.at.error(
                    "protocol methods require self borrowed as &Protocol or &mut Protocol",
                ));
            }
            f.inputs[0].1.args[0].args.clear();
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
/// `bytes` has `len`, which `std.bytes.ByteSource` requires.
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
    if protocol
        .methods
        .iter()
        .any(|method| method.name == "format")
    {
        if let Some((cause, required)) = aliases.data.report_context(actual, standard_sources) {
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
    }
    if matches!(actual, Ty::Enum(sum) if sum.anonymous()) {
        if !reporting(protocol, standard_sources) {
            return Err(call.error(format!(
                "{actual} does not satisfy {}; anonymous sums only forward std.report.Formattable",
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
    protocol.name == "std.report.Formattable"
        && protocol
            .at
            .source
            .as_ref()
            .is_some_and(|source| standard_sources.contains(source))
}
