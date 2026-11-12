//! Fixed std ceilings and open modes are declaration metadata, not new types.
//! Check statically known inputs before call wrappers replace them with local
//! names; std bodies keep their runtime guards for dynamic inputs and erased
//! callable values.
use super::*;

impl Parser {
    /// The std-only marker follows a parameter or field's existing type.
    /// Keeping it outside type arguments avoids introducing value generics.
    pub(super) fn limited_type(&mut self) -> Result<TypeRef> {
        let mut ty = self.ty()?;
        if self.peek().is("__open_mode") {
            let at = self.take();
            if !self.standard {
                return Err(at.error(
                    "`__open_mode` is reserved for standard-library parameter and field declarations",
                ));
            }
            let text = |ty: &TypeRef| {
                matches!(ty.name.as_deref(), Some("str" | "strview" | "substr"))
                    && ty.args.is_empty()
            };
            let borrowed = ty.name.as_deref() == Some("&") && ty.args.len() == 1;
            if !(text(&ty) || borrowed && text(&ty.args[0])) {
                return Err(at.error("`__open_mode` requires str, &str or &strview"));
            }
            ty.metadata = Some(Box::new(TypeMetadata {
                check: Some(StaticCheck::OpenMode),
                ..Default::default()
            }));
            return Ok(ty);
        }
        if !self.peek().is("__max") {
            return Ok(ty);
        }
        let at = self.take();
        if !self.standard {
            return Err(at.error(
                "`__max` is reserved for standard-library parameter and field declarations",
            ));
        }
        if !matches!(ty.name.as_deref(), Some("Positive" | "NonNegative"))
            || ty.args.len() != 1
            || ty.args[0].name.as_deref() != Some("i64")
        {
            return Err(at.error("`__max` requires Positive[i64] or NonNegative[i64]"));
        }
        self.expect("(")?;
        let bound = self.take();
        let Kind::Number(text) = &bound.kind else {
            return Err(bound.error("`__max` takes a non-negative decimal i64 literal"));
        };
        let ceiling = text
            .replace('_', "")
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .ok_or_else(|| bound.error("`__max` takes a non-negative decimal i64 literal"))?;
        if ceiling == 0 && ty.name.as_deref() == Some("Positive") {
            return Err(bound.error("a Positive[i64] ceiling must be at least 1"));
        }
        self.expect(")")?;
        ty.metadata = Some(Box::new(TypeMetadata {
            check: Some(StaticCheck::Ceiling(ceiling)),
            ..Default::default()
        }));
        Ok(ty)
    }
}

impl Lower<'_> {
    pub(super) fn check_ceiling(
        &self,
        value: &Expr,
        operation: &str,
        parameter: &str,
        ceiling: i64,
    ) -> Result<()> {
        if let Some(integer) = consts::known_integer(value, &self.generics.constants, self.aliases)
            && integer > i128::from(ceiling)
        {
            return Err(value.at.error(format!("`{operation}`: constant argument `{parameter}` is {integer}, above its fixed ceiling of {ceiling}")));
        }
        Ok(())
    }

    pub(super) fn check_static(
        &self,
        value: &Expr,
        operation: &str,
        parameter: &str,
        check: StaticCheck,
    ) -> Result<()> {
        match check {
            StaticCheck::Ceiling(ceiling) => {
                self.check_ceiling(value, operation, parameter, ceiling)
            }
            StaticCheck::OpenMode => self.check_open_mode(value),
        }
    }

    pub(super) fn check_call_ceilings(&self, name: &str, args: &[Expr]) -> Result<()> {
        let Some(inputs) = self.generics.ceilings.get(name) else {
            return Ok(());
        };
        for (index, arg) in args.iter().enumerate() {
            let (input, value) = match &arg.kind {
                Expression::NamedArgument(label, value) => {
                    (inputs.iter().find(|(n, _)| n == label), &**value)
                }
                _ => (inputs.get(index), arg),
            };
            if let Some((parameter, Some(check))) = input {
                self.check_static(value, name, parameter, *check)?;
            }
        }
        Ok(())
    }

    pub(super) fn check_field_ceiling(&self, target: &Expr, value: &Expr) -> Result<()> {
        let Expression::Member(base, field, _) = &ungroup(target).kind else {
            return Ok(());
        };
        let Some(mut owner) = self.place_type(base) else {
            return Ok(());
        };
        loop {
            owner = match owner {
                Ty::Ref(inner, _) | Ty::Box(inner) | Ty::Shared(inner) => (*inner).clone(),
                _ => break,
            };
        }
        let Some((declaration, _)) = self.aliases.data.origin_class(&owner) else {
            return Ok(());
        };
        if let Some((_, ty)) = declaration.fields.iter().find(|(name, _)| name == field)
            && let Some(check) = ty.static_check()
        {
            self.check_static(value, &declaration.name, field, check)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_defaults_are_rejected_without_a_call() {
        let mut resolved = modules::single(
            "def f(value: Positive[i64] = 65) -> ():\n    pass\ndef main() -> ():\n    pass\n",
        )
        .unwrap();
        let f = resolved
            .functions
            .iter_mut()
            .find(|f| f.name == "f")
            .unwrap();
        // Inject the metadata an embedded std declaration's parser supplies.
        f.inputs[0].1.metadata = Some(Box::new(TypeMetadata {
            check: Some(StaticCheck::Ceiling(64)),
            ..Default::default()
        }));
        let error = match lower(
            resolved,
            &mut Heap::default(),
            crate::CheckOptions::default(),
        ) {
            Ok(_) => panic!("invalid default accepted"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("constant argument `value` is 65"), "{error}");
    }
}
