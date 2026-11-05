//! Compile-time arguments are distinct from runtime types. Values participate
//! in specialization identity, but can never become stored element types.
use super::*;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Argument {
    Type(Ty),
    Value(Constant),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Constant {
    pub(super) ty: Ty,
    /// All eight integer domains fit i128; bool uses zero and one.
    pub(super) value: i128,
}
impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ty == Ty::Bool {
            f.write_str(if self.value == 0 { "False" } else { "True" })
        } else {
            self.value.fmt(f)
        }
    }
}
impl fmt::Display for Argument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Type(t) => t.fmt(f),
            Self::Value(v) => v.fmt(f),
        }
    }
}
impl Argument {
    pub(super) fn as_type(&self) -> Option<&Ty> {
        match self {
            Self::Type(t) => Some(t),
            Self::Value(_) => None,
        }
    }
    pub(super) fn identity_name(&self) -> String {
        match self {
            Self::Type(t) => t.identity_name(),
            Self::Value(v) => v.to_string(),
        }
    }
    pub(super) fn reference(&self, at: &Token) -> TypeRef {
        match self {
            Self::Type(t) => generics::type_ref(t, at),
            Self::Value(v) => TypeRef::value(v.expression(at)),
        }
    }
}
impl Constant {
    pub(super) fn expression(&self, at: &Token) -> Expr {
        let kind = if self.ty == Ty::Bool {
            Expression::Bool(self.value != 0)
        } else if self.value < 0 {
            Expression::Unary(
                "-".into(),
                Box::new(Expr {
                    at: at.clone(),
                    kind: Expression::Number(format!("{}{}", -self.value, self.ty)),
                }),
            )
        } else {
            Expression::Number(format!("{}{}", self.value, self.ty))
        };
        Expr {
            at: at.clone(),
            kind,
        }
    }
}
impl TypeRef {
    pub(super) fn value(e: Expr) -> Self {
        Self {
            at: e.at.clone(),
            name: None,
            args: vec![],
            concrete: None,
            metadata: Some(Box::new(TypeMetadata {
                value: Some(Box::new(e)),
                ..Default::default()
            })),
        }
    }
    pub(super) fn value_expression(&self) -> Option<&Expr> {
        self.metadata.as_ref()?.value.as_deref()
    }
    pub(super) fn value_parameter(&self) -> bool {
        self.metadata.as_ref().is_some_and(|m| m.value_parameter)
    }
}

pub(super) fn validate_parameters(params: &[(String, Option<TypeRef>)]) -> Result<()> {
    for (index, (_, bound)) in params.iter().enumerate() {
        let Some(bound) = bound.as_ref().filter(|b| b.value_parameter()) else {
            continue;
        };
        let concrete = bound
            .name
            .as_deref()
            .and_then(named_type)
            .is_some_and(|t| t.is_int() || t == Ty::Bool);
        let dependent = params[..index].iter().any(|(n, b)| {
            Some(n) == bound.name.as_ref()
                && b.as_ref()
                    .is_some_and(|b| !b.value_parameter() && b.name.as_deref() == Some("IntType"))
        });
        if !bound.args.is_empty() || !(concrete || dependent) {
            return Err(bound.at.error("a const parameter requires an integer type, bool, or a preceding type parameter constrained by IntType"));
        }
    }
    Ok(())
}

pub(super) fn resolve_arguments(
    params: &[(String, Option<TypeRef>)],
    args: &[TypeRef],
    aliases: &TypeAliases,
    at: &Token,
) -> Result<Vec<Argument>> {
    if params.len() != args.len() {
        return Err(at.error(format!(
            "generic declaration requires {} arguments, got {}",
            params.len(),
            args.len()
        )));
    }
    let mut replacements = generics::Substitution::default();
    let mut actual = Vec::with_capacity(args.len());
    for ((name, bound), arg) in params.iter().zip(args) {
        let value = if let Some(bound) = bound.as_ref().filter(|b| b.value_parameter()) {
            let mut bound = bound.clone();
            generics::substitute(&mut bound, &replacements)?;
            let ty = bound.resolve(aliases)?.ok_or_else(|| {
                bound
                    .at
                    .error("a const parameter requires an integer or bool type")
            })?;
            if !ty.is_int() && ty != Ty::Bool {
                return Err(bound
                    .at
                    .error("a const parameter requires an integer or bool type"));
            }
            let expression = if let Some(e) = arg.value_expression() {
                e.clone()
            } else if arg.args.is_empty()
                && let Some(name) = &arg.name
            {
                if aliases.data.shapes_only() && aliases.contains_key(name) {
                    Constant {
                        ty: ty.clone(),
                        value: 0,
                    }
                    .expression(&arg.at)
                } else {
                    Expr {
                        at: arg.at.clone(),
                        kind: Expression::Call(format!("$const.{name}"), vec![]),
                    }
                }
            } else {
                return Err(arg.at.error(
                    "value arguments must be literals, module constants, or value parameters",
                ));
            };
            let constant = consts::generic_value(&expression, &ty, aliases)?;
            Argument::Value(constant)
        } else {
            Argument::Type(if bound.as_ref().is_some_and(dimensions::bound) {
                arg.resolve_dimension(aliases)?
            } else {
                let ty = arg.resolve_argument(aliases)?.unwrap_or(Ty::Unit);
                if let Ty::Dimension(d) = &ty {
                    return Err(aliases.dimension_error(
                        &arg.at,
                        format!("dimension `{d}` has no values; dimension arguments require a parameter constrained by Dimension"),
                        [&ty],
                    ));
                }
                ty
            })
        };
        replacements.insert(name.clone(), value.reference(&arg.at));
        actual.push(value);
    }
    Ok(actual)
}

pub(super) fn parameter_type(ty: &Ty, at: &Token) -> TypeRef {
    let mut t = generics::type_ref(ty, at);
    t.metadata
        .get_or_insert_with(Default::default)
        .value_parameter = true;
    t
}

pub(super) fn spelling(e: &Expr) -> String {
    match &e.kind {
        Expression::Number(n) => n.clone(),
        Expression::Bool(b) => if *b { "True" } else { "False" }.into(),
        Expression::Unary(sign, e) => format!("{sign}{}", spelling(e)),
        Expression::Call(name, _) => name.strip_prefix("$const.").unwrap_or(name).into(),
        _ => "value".into(),
    }
}
