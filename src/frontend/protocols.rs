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
        let mut seen = HashSet::new();
        while self.peek().kind != Kind::Dedent && self.peek().kind != Kind::Eof {
            if !self.peek().is("def") {
                return Err(self
                    .peek()
                    .error("protocols contain method signatures with pass bodies"));
            }
            let mut f = self.function_in(Some(&name))?;
            if !f.type_params.is_empty() {
                return Err(f
                    .at
                    .error("protocol requirements cannot declare method type parameters"));
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

pub(super) fn signature(f: &Function, aliases: &TypeAliases) -> Result<(Vec<Ty>, Type)> {
    Ok((
        f.inputs
            .iter()
            .map(|(_, t)| {
                t.resolve(aliases)?
                    .ok_or_else(|| t.at.error("unit parameters are not supported yet"))
            })
            .collect::<Result<_>>()?,
        f.output.resolve(aliases)?,
    ))
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
        for (_, ty) in &mut method.inputs {
            generics::substitute(ty, &substitutions)?;
        }
        generics::substitute(&mut method.output, &substitutions)?;
    }
    Ok(concrete)
}

pub(super) fn check(
    protocol: &Protocol,
    actual: &Ty,
    methods: &HashMap<String, (Vec<Ty>, Type)>,
    aliases: &TypeAliases,
    access: &modules::AccessMap,
    bound: &Token,
    call: &Token,
) -> Result<()> {
    let Ty::Class(class) = actual else {
        return Err(call.error(format!(
            "{actual} does not satisfy {}; protocols currently require classes",
            protocol.name
        )));
    };
    let mut aliases = aliases.clone();
    aliases.insert(protocol.name.clone(), Some(actual.clone()));
    for requirement in &protocol.methods {
        let name = &requirement.name;
        let method = methods
            .get(&crate::record::method(&class.name, name))
            .ok_or_else(|| {
                call.error(format!(
                    "{actual} does not satisfy {}: missing method `{name}`",
                    protocol.name
                ))
            })?;
        modules::check_member(access, &class.name, name, bound)?;
        let expected = signature(requirement, &aliases)?;
        if *method != expected {
            return Err(call.error(format!("{actual} does not satisfy {}: signature of `{name}` does not match (including receiver borrowing and return type)", protocol.name)));
        }
    }
    Ok(())
}
