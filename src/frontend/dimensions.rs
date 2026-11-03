//! Dimension declarations and kind checking, kept separate from value types.
use super::*;
use crate::dimension::Dimension;

pub(super) fn expression(name: &str) -> bool {
    matches!(name, "*" | "/" | "$Pow" | "$Root")
}

pub(super) fn syntax(name: &str) -> bool {
    expression(name) || name.starts_with("$exponent:")
}

pub(super) fn bound(bound: &TypeRef) -> bool {
    bound.name.as_deref() == Some("Dimension") && bound.args.is_empty()
}

pub(super) fn is_dimension(ty: &Ty) -> bool {
    matches!(ty, Ty::Dimension(_))
}

pub(super) fn validate_input(ty: &TypeRef, parameters: &[String]) -> Result<()> {
    fn mentions(t: &TypeRef, parameters: &[String]) -> bool {
        t.name
            .as_ref()
            .is_some_and(|name| parameters.contains(name))
            || t.args.iter().any(|arg| mentions(arg, parameters))
    }
    if ty.name.as_deref().is_some_and(expression) && mentions(ty, parameters) {
        return Err(ty.at.error("dimension arguments in parameter types must be concrete or a bare dimension parameter; expressions using parameters belong in return types or bodies"));
    }
    for arg in &ty.args {
        validate_input(arg, parameters)?;
    }
    Ok(())
}

impl Parser {
    pub(super) fn dimension_exponent(&mut self) -> Result<TypeRef> {
        let at = self.peek().clone();
        let negative = self.eat("-");
        let token = self.take();
        let Kind::Number(number) = &token.kind else {
            return Err(token.error("dimension powers and roots require an integer literal"));
        };
        if number.starts_with('_') || number.ends_with('_') || number.contains("__") {
            return Err(token.error("invalid integer separator"));
        }
        let spelling = format!("{}{number}", if negative { "-" } else { "" });
        let exponent = spelling.replace('_', "").parse::<i64>().map_err(|_| {
            token.error("dimension powers and roots require an integer literal within i64 range")
        })?;
        Ok(TypeRef {
            concrete: None,
            metadata: None,
            at,
            name: Some(format!("$exponent:{exponent}")),
            args: vec![],
        })
    }

    pub(super) fn dimension_decl(&mut self, module: &str) -> Result<TypeAlias> {
        let at = self.take();
        let name = self.name()?;
        let target = if self.eat("=") {
            self.ty()?
        } else {
            self.expect("(")?;
            let symbol = self.take();
            let Kind::Text(symbol) = symbol.kind else {
                return Err(
                    symbol.error("a base dimension requires a coherent-unit string literal")
                );
            };
            self.expect(")")?;
            let qualified = if module.is_empty() {
                name.clone()
            } else {
                format!("{module}.{name}")
            };
            generics::type_ref(
                &Ty::Dimension(Arc::new(Dimension::base(qualified, symbol.to_string()))),
                &at,
            )
        };
        self.kind(Kind::Newline, "the end of the dimension declaration")?;
        Ok(TypeAlias {
            at,
            name,
            target,
            dimension: true,
        })
    }
}

impl TypeAliases {
    pub(super) fn intern_dimension(&self, dimension: Dimension) -> Ty {
        Ty::Dimension(self.dimensions.borrow_mut().intern(dimension))
    }
}

impl TypeRef {
    pub(super) fn resolve_dimension(&self, aliases: &TypeAliases) -> Result<Ty> {
        let ty = self.resolve_argument(aliases)?;
        match ty {
            Some(Ty::Dimension(d)) => Ok(aliases.intern_dimension((*d).clone())),
            _ => Err(self
                .at
                .error(format!("`{}` is not a dimension", self.spelling()))),
        }
    }

    pub(super) fn dimension_expression(&self, aliases: &TypeAliases) -> Result<Type> {
        let name = self.name.as_deref().unwrap();
        let arity = if name == "One" { 0 } else { 2 };
        if self.args.len() != arity {
            let name = name.trim_start_matches('$');
            return Err(self
                .at
                .error(format!("`{name}` requires {arity} dimension arguments")));
        }
        if name == "One" {
            return Ok(Some(aliases.intern_dimension(Dimension::default())));
        }
        let Ty::Dimension(left) = self.args[0].resolve_dimension(aliases)? else {
            unreachable!()
        };
        let dimension = if matches!(name, "*" | "/") {
            let Ty::Dimension(right) = self.args[1].resolve_dimension(aliases)? else {
                unreachable!()
            };
            left.product(&right, name == "/")
        } else {
            let exponent = self.args[1]
                .name
                .as_deref()
                .and_then(|n| n.strip_prefix("$exponent:"))
                .and_then(|n| n.parse::<i64>().ok())
                .ok_or_else(|| {
                    self.args[1].at.error(
                        "dimension powers and roots require an integer literal within i64 range",
                    )
                })?;
            if name == "$Pow" {
                left.power(exponent)
            } else {
                left.root(exponent)
            }
        }
        .map_err(|error| {
            aliases.dimension_error(
                &self.at,
                format!("`{}`: {error}", self.spelling()),
                [&Ty::Dimension(left.clone())],
            )
        })?;
        Ok(Some(aliases.intern_dimension(dimension)))
    }
}
