//! Direct std.alloc algebra alone can lend either a set or a dictionary's keys.
//! Specialize the verified wrapper body, not a public overload or a conversion.
use super::*;

impl Lower<'_> {
    // These are the two producers of a local key view. A method named `keys`
    // may instead return an ordinary value; decide from its actual type below.
    pub(super) fn possible_algebra_view(&self, arg: &Expr) -> bool {
        match &ungroup(arg).kind {
            Expression::NamedArgument(_, value) => self.possible_algebra_view(value),
            Expression::Name(name) => self
                .names
                .get(name)
                .is_some_and(|l| matches!(l.ty, Ty::KeyView(_))),
            Expression::Method(_, name, args, _) => name == "keys" && args.is_empty(),
            _ => false,
        }
    }

    pub(super) fn allocator_algebra_call(
        &mut self,
        name: &str,
        types: Option<&[TypeRef]>,
        args: &[Expr],
        at: &Token,
        expected: Option<Expectation>,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let prepared = self.prepare_named_call(name, args, at, true)?;
        let (args, swapped) = match &prepared {
            Some((callee, values)) => (values.as_slice(), callee != name),
            None => (args, false),
        };
        let [receiver, first, second] = args else {
            return Err(at.error(format!("`{name}` expects 3 arguments, got {}", args.len())));
        };
        let template = self.generics.templates[name].clone();
        let explicit = types
            .map(|types| {
                value_generics::resolve_arguments(&template.type_params, types, self.aliases, at)
            })
            .transpose()?;
        let set = explicit
            .as_ref()
            .and_then(|types| types[0].as_type())
            .map(|key| Ty::Set(Arc::new(key.clone())));
        let receiver_ty = template.inputs[0].1.resolve(self.aliases)?.unwrap();
        let start = ops.len();
        let mut loans = self.call_arguments(
            std::slice::from_ref(receiver),
            &[(String::new(), receiver_ty)],
            ops,
        )?;
        let mut sources = Vec::with_capacity(2);
        let mut inferred = HashMap::default();
        let arguments = if swapped {
            [second, first]
        } else {
            [first, second]
        };
        for arg in [first, second] {
            let (ty, loan) = self.algebra_operand(arg, set.clone(), ops)?;
            let key = allocator_storage::algebra_key(&ty).ok_or_else(|| {
                arg.at
                    .error(format!("expected a shared set or KeyView, got {ty}"))
            })?;
            if explicit.is_none() {
                // Both set patterns name the same K. Unwrap just this operand,
                // never infer through a KeyView in an ordinary generic call.
                let pattern = &template.inputs[1].1.args[0].args[0];
                infer(
                    pattern,
                    key,
                    &template,
                    &mut inferred,
                    self.aliases,
                    &arg.at,
                )?;
            }
            sources.push(ty);
            loans.push(loan);
        }
        if swapped {
            // Binding validated the labels. Evaluate in written order, then
            // restore declaration order (in particular for difference).
            ops.push(Op::Swap);
            sources.swap(0, 1);
            loans.swap(1, 2);
        }
        let types = if let Some(explicit) = explicit {
            explicit
        } else {
            if let Some(expected) = expected {
                let hint = result_evidence(
                    &template,
                    expected.ty,
                    expected.open_error,
                    self.aliases,
                    at,
                );
                combine_result_evidence(name, &hint, &mut inferred, at)?;
            }
            vec![
                inferred
                    .remove(&template.type_params[0].0)
                    .expect("both sources determine the key"),
            ]
        };
        let variant = self.generics.algebra_variant(&template, &sources);
        let symbol = self.specialize(&variant, types, at)?;
        let sig = self.sigs[&symbol].clone();
        for ((actual, (_, expected)), arg) in
            sources.into_iter().zip(&sig.inputs[1..]).zip(arguments)
        {
            self.same(Some(actual), Some(expected.clone()), &arg.at)?;
        }
        self.record_direct_call(&symbol, at, &loans, ops);
        self.passes(start, &sig.inputs, sig.outputs.first(), ops);
        self.push_call(Op::Call(symbol), &sig.inputs, &loans, at, ops)?;
        Self::end_reads(loans, ops);
        Ok(sig.outputs.first().cloned())
    }

    fn algebra_operand(&mut self, arg: &Expr, set: Type, ops: &mut Vec<Op>) -> Result<(Ty, usize)> {
        let (ty, loan) = if self.possible_algebra_view(arg) {
            let ty = self.stored_value(arg, set.clone(), ops)?;
            if matches!(ty, Ty::KeyView(_)) {
                // Take the origin now: lowering the next projection creates
                // another loan. Both must remain live through the native call.
                return Ok((ty, self.reference_origin(arg, ops)?));
            }
            self.lend_argument_value(arg, ty, ops)?
        } else {
            self.call_borrow(arg, false, set.clone(), ops)?
        };
        let ty = match set {
            Some(set) => self.lend_content(ty, &Ty::Ref(Arc::new(set), false), ops),
            None => ty,
        };
        Ok((ty, loan))
    }
}

impl Engine {
    fn algebra_variant(&mut self, template: &Function, sources: &[Ty]) -> String {
        let mask = usize::from(matches!(sources[0], Ty::KeyView(_)))
            | (usize::from(matches!(sources[1], Ty::KeyView(_))) << 1);
        if mask == 0 {
            return template.name.clone();
        }
        // Unspellable cache name; retain the authenticated template identity
        // and original body. No callable or job lookup selects this variant.
        let variant = format!("{}$keys{mask}", template.name);
        self.templates.entry(variant.clone()).or_insert_with(|| {
            let mut adjusted = template.clone();
            for ((_, pattern), ty) in adjusted.inputs[1..].iter_mut().zip(sources) {
                if matches!(ty, Ty::KeyView(_)) {
                    *pattern = pattern.args[0].clone();
                    pattern.name = Some("KeyView".into());
                }
            }
            Arc::new(adjusted)
        });
        variant
    }
}
