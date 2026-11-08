//! Explicit error mapping (issue #556). `Result.map_error` is the ordinary
//! Plenty helper in `map_error.plenty`, specialized like user code; only
//! receiver dispatch and mapper diagnostics live here. A payload-bearing enum
//! variant named without arguments is a constructor function value, so
//! `result.map_error(AppError.Io)` needs no closure.
use super::*;

const MAP_ERROR: &str = include_str!("map_error.plenty");
const MAP_ERROR_HELPER: &str = "__plenty_map_error";

fn hidden(at: &Token, name: &str) -> Expr {
    Expr {
        at: at.clone(),
        kind: Expression::Name(name.into()),
    }
}

impl Lower<'_> {
    /// `base.map_error(mapper)`: evaluate the receiver, then the mapper, once
    /// and in that order, and pass both owned values to the helper. The helper
    /// invokes the mapper only on `Err`; on `Ok` the unused mapper is dropped.
    pub(super) fn result_map_error(
        &mut self,
        base: &Expr,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let start = self.locals.len();
        let actual = self.expr(base, ops)?;
        if let Some(actual) = &actual {
            let receiver = classes::unbox(actual.clone());
            if classes::inherent_receiver(&receiver) {
                self.unbox_to(actual.clone(), &receiver, ops);
                return self.temporary_class_method(
                    base,
                    "map_error",
                    None,
                    args,
                    (receiver, vec![]),
                    ops,
                );
            }
        }
        let error = match &actual {
            Some(Ty::Enum(t)) if t.is_result() => t.get().variants[1].fields[0].clone(),
            Some(Ty::Ref(inner, _)) if matches!(&**inner, Ty::Enum(t) if t.is_result()) => {
                return Err(base.at.error(
                    "map_error consumes its Result; pass an owned Result, not a reference",
                ));
            }
            Some(ty) => {
                return Err(base
                    .at
                    .error(format!("map_error requires a Result, got {ty}")));
            }
            None => return Err(base.at.error("map_error requires a Result, got ()")),
        };
        let [mapper] = args else {
            return Err(at.error(format!(
                "map_error takes one mapper, got {} arguments",
                args.len()
            )));
        };
        let result = actual.unwrap();
        let result_name = self.hide(result, at, ops)?;
        let mapper_ty = self.value(mapper, ops)?;
        let expected =
            format!("map_error mapper must take the error `{error}` and return the new error type");
        match &mapper_ty {
            Ty::Ref(..) => {
                return Err(mapper.at.error(
                    "map_error takes an owned mapper; pass the function or closure itself, not a reference",
                ));
            }
            ty => match callables::signature(ty) {
                Some(signature)
                    if signature.inputs == [error.clone()] && signature.output.is_some() => {}
                Some(signature) => {
                    let inputs = signature
                        .inputs
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ");
                    let output = signature
                        .output
                        .as_ref()
                        .map_or_else(|| "()".to_string(), ToString::to_string);
                    let extra = if signature.inputs.len() > 1 {
                        format!(
                            "; supply the other payloads in a closure such as `def(cause: {error}) -> T: ...`"
                        )
                    } else {
                        String::new()
                    };
                    return Err(mapper.at.error(format!(
                        "{expected}; got a callable taking ({inputs}) and returning {output}{extra}"
                    )));
                }
                None => return Err(mapper.at.error(format!("{expected}; got {ty}"))),
            },
        }
        let mapper_name = self.hide(mapper_ty, at, ops)?;
        self.plenty_helper(MAP_ERROR_HELPER, MAP_ERROR)?;
        let output = self.generic_call(
            MAP_ERROR_HELPER,
            None,
            &[hidden(at, &result_name), hidden(at, &mapper_name)],
            at,
            ops,
        );
        self.names.remove(&result_name);
        self.names.remove(&mapper_name);
        let output = output?;
        self.cleanup(start, ops);
        Ok(output)
    }

    /// Hold an evaluated operand in a fresh slot under a name no source can spell.
    pub(super) fn hide(&mut self, ty: Ty, at: &Token, ops: &mut Vec<Op>) -> Result<String> {
        let slot = self.slot(ty.clone(), at)?;
        ops.push(Op::StoreLocal(slot));
        let name = format!("__plenty_operand{slot}");
        self.bind(
            name.clone(),
            Local {
                slot,
                ty,
                mutable: false,
            },
        );
        Ok(name)
    }

    pub(super) fn class_constructor_value(
        &mut self,
        ty: Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if ty.layout_depth() >= 64 {
            return Err(at.error("type nesting exceeds the implementation limit of 64"));
        }
        let Ty::Class(class) = ty else { unreachable!() };
        modules::check_member(self.access, &class.name, "__new__", at)?;
        self.sync_data()?;
        self.function_value(&crate::record::method(&class.name, "new"), at, ops)
    }

    /// A constructor's expected result identifies its concrete data instance.
    /// Inference never guesses missing enum or class type arguments.
    pub(super) fn constructor_expected(
        &mut self,
        e: &Expr,
        expected: &Ty,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Ty>> {
        let Ty::Callable(signature) = expected else {
            return Ok(None);
        };
        let Some(output) = &signature.output else {
            return Ok(None);
        };
        match &ungroup(e).kind {
            Expression::Name(name) if !self.names.contains_key(name) => {
                if enums::prelude_variant(name) && name != "Nothing" {
                    if !enums::prelude_owner(name, output) {
                        return Err(e.at.error(format!("constructor `{name}` does not produce the expected Callable result {output}")));
                    }
                    return self.variant(output.clone(), name, None, &e.at, ops);
                }
                if self.aliases.data.is_class(name) {
                    let class = match output {
                        Ty::Enum(t) if t.is_result() => &t.get().variants[0].fields[0],
                        other => other,
                    };
                    if self
                        .aliases
                        .data
                        .arguments(class)
                        .is_some_and(|(owner, _)| owner == *name)
                    {
                        return self
                            .class_constructor_value(class.clone(), &e.at, ops)
                            .map(Some);
                    }
                }
            }
            Expression::Member(base, variant, _) => {
                if let Expression::Name(owner) = &ungroup(base).kind
                    && !self.names.contains_key(owner)
                    && self.aliases.data.is_enum(owner)
                    && self
                        .aliases
                        .data
                        .arguments(output)
                        .is_some_and(|(name, _)| name == *owner)
                {
                    return self.variant(output.clone(), variant, None, &e.at, ops);
                }
            }
            _ => {}
        }
        Ok(None)
    }

    /// The function value for variant `tag` of `t`: a generated function
    /// taking the payloads in order and returning the constructed variant.
    /// Calling it is equivalent to `Enum.Variant(payloads...)`.
    pub(super) fn variant_function(
        &mut self,
        t: crate::nominal::Nominal<crate::sum::EnumType>,
        tag: usize,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let variant = t.get().variants[tag].name.clone();
        let fields = t.get().variants[tag].fields.clone();
        if fields.iter().any(Ty::unresolved_generator) {
            return Err(at.error(format!(
                "variant `{}.{variant}` cannot be used as a function value because a payload is a generator; write a closure",
                t.name
            )));
        }
        let symbol = format!("__plenty_variant_{}.{variant}", t.name);
        if !self.sigs.contains_key(&symbol) {
            let sig = Arc::new(crate::op::FnSig {
                inputs: fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| (format!("payload{index}"), field.clone()))
                    .collect(),
                outputs: vec![Ty::Enum(t.clone())],
            });
            let mut body: Vec<Op> = fields
                .iter()
                .enumerate()
                .map(|(index, field)| {
                    if field.affine() {
                        Op::MoveLocal(index as u8)
                    } else {
                        Op::LoadLocal(index as u8)
                    }
                })
                .collect();
            body.push(Op::Enum(crate::sum::EnumOp::New(t.clone(), tag)));
            body.push(Op::Return);
            self.sigs.insert(symbol.clone(), sig.clone());
            self.generics.compiled.push(Op::DefineFn(
                symbol.clone(),
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
        self.function_value(&symbol, at, ops).map(Some)
    }
}
