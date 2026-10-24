//! Static call binding. Wrappers preserve written evaluation order and give
//! omitted defaults the callee's error and ownership boundary.
use super::*;

impl Lower<'_> {
    pub(super) fn validate_defaults(&mut self, function: &Function) -> Result<()> {
        if !function.defaults.iter().any(Option::is_some) {
            return Ok(());
        }
        let signature = self.sigs[&function.name].clone();
        for (index, default) in function.defaults.iter().enumerate() {
            let Some(default) = default else { continue };
            let ty = &signature.inputs[index].1;
            let ty = if let Ty::Ref(inner, _) = ty {
                inner
            } else {
                ty
            };
            let at = &default.at;
            let mut body = vec![Stmt {
                at: default.at.clone(),
                kind: Statement::Assign {
                    name: format!("$default{index}"),
                    mutable: false,
                    annotation: Some(generics::type_ref(ty, &default.at)),
                    value: default.clone(),
                },
            }];
            body.push(Stmt {
                at: at.clone(),
                kind: Statement::Return(signature.outputs.first().map(|_| Expr {
                    at: at.clone(),
                    kind: Expression::Name("$result".into()),
                })),
            });
            // A separate unreachable check keeps the callee's result contract
            // without spending one local slot per parameter and per default.
            let validator = Function {
                once: false,
                captures: vec![],
                foreign: None,
                export: None,
                name: format!("__plenty_default_check_{}_{index}", function.name),
                type_params: vec![],
                at: function.at.clone(),
                inputs: signature
                    .outputs
                    .first()
                    .map(|ty| vec![("$result".into(), generics::type_ref(ty, at))])
                    .unwrap_or_default(),
                defaults: vec![],
                output: generics::type_ref(signature.outputs.first().unwrap_or(&Ty::Unit), at),
                doc: String::new(),
                doc_at: None,
                body,
            };
            register_signature(&validator, self.aliases, self.sigs)?;
            self.generics
                .inherit_requirement(&function.name, &validator.name);
            self.generics.declare(validator, self.sigs);
        }
        Ok(())
    }

    pub(super) fn prepare_named_call(
        &mut self,
        name: &str,
        args: &[Expr],
        at: &Token,
        method: bool,
    ) -> Result<Option<(String, Vec<Expr>)>> {
        let named = args
            .iter()
            .any(|arg| matches!(arg.kind, Expression::NamedArgument(..)));
        let template = self.generics.templates.get(name).cloned();
        let defaults = self
            .generics
            .defaults
            .get(name)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if !named && defaults.is_empty() {
            return Ok(None);
        }
        let (inputs, output, type_params) = if let Some(template) = &template {
            (
                template.inputs.clone(),
                template.output.clone(),
                template.type_params.clone(),
            )
        } else if let Some(sig) = self.sigs.get(name) {
            // Frontend signatures carry at most one output; only textual IR
            // signatures can have several, and those never reach `self.sigs`.
            debug_assert!(sig.outputs.len() <= 1);
            (
                sig.inputs
                    .iter()
                    .map(|(n, ty)| (n.clone(), generics::type_ref(ty, at)))
                    .collect(),
                generics::type_ref(sig.outputs.first().unwrap_or(&Ty::Unit), at),
                Vec::new(),
            )
        } else {
            return Ok(None);
        };
        if !named && args.len() == inputs.len() {
            return Ok(None);
        }
        if method
            && args.len() < inputs.len()
            && self
                .generics
                .protocol_instances
                .contains(&self.function_name)
        {
            return Err(at.error("method defaults cannot be omitted in a protocol-constrained function; pass all arguments required by the protocol"));
        }
        let mut supplied = vec![false; inputs.len()];
        let mut positions = Vec::with_capacity(args.len());
        let mut values = Vec::with_capacity(args.len());
        let mut saw_named = false;
        for (position, argument) in args.iter().enumerate() {
            let (index, value) = if let Expression::NamedArgument(label, value) = &argument.kind {
                saw_named = true;
                let index = inputs
                    .iter()
                    .position(|(parameter, _)| parameter == label)
                    .filter(|index| !method || *index != 0)
                    .ok_or_else(|| {
                        argument
                            .at
                            .error(format!("unknown named argument `{label}` for `{name}`"))
                    })?;
                (index, &**value)
            } else {
                if saw_named {
                    return Err(argument
                        .at
                        .error("positional arguments must precede named arguments"));
                }
                if position >= inputs.len() {
                    return Err(at.error(format!(
                        "`{name}` expects at most {} arguments, got {}",
                        inputs.len(),
                        args.len()
                    )));
                }
                (position, argument)
            };
            if std::mem::replace(&mut supplied[index], true) {
                return Err(argument.at.error(format!(
                    "duplicate argument: parameter `{}` is supplied more than once",
                    inputs[index].0
                )));
            }
            positions.push(index);
            values.push(value.clone());
        }
        for (index, (parameter, _)) in inputs.iter().enumerate() {
            if !supplied[index] && defaults.get(index).and_then(Option::as_ref).is_none() {
                return Err(at.error(format!("missing argument `{parameter}` for `{name}`")));
            }
        }
        if positions.iter().copied().eq(0..inputs.len()) {
            return Ok(Some((name.to_owned(), values)));
        }
        let key = (name.to_owned(), positions.clone());
        if let Some(wrapper) = self.generics.call_wrappers.get(&key) {
            return Ok(Some((wrapper.clone(), values)));
        }
        let symbol = format!(
            "__plenty_call_wrapper_{}",
            self.generics.call_wrappers.len()
        );
        let mut body = Vec::new();
        let mut forwarded = Vec::with_capacity(inputs.len());
        for (index, (parameter, pattern)) in inputs.iter().enumerate() {
            let value = Expr {
                at: at.clone(),
                kind: Expression::Name(parameter.clone()),
            };
            if supplied[index] {
                forwarded.push(value);
                continue;
            }
            let default = defaults[index].as_ref().unwrap();
            let resolved = pattern
                .resolve(self.aliases)
                .ok()
                .flatten()
                .map(|ty| generics::type_ref(&ty, &default.at));
            let ty = resolved.as_ref().unwrap_or(pattern);
            let borrowed = matches!(ty.name.as_deref(), Some("&" | "&mut"));
            let mutable = ty.name.as_deref() == Some("&mut");
            if borrowed && matches!(output.name.as_deref(), Some("&" | "&mut")) {
                return Err(
                    at.error("a returned reference cannot borrow an omitted default temporary")
                );
            }
            body.push(Stmt {
                at: default.at.clone(),
                kind: Statement::Assign {
                    name: parameter.clone(),
                    mutable,
                    annotation: Some(if borrowed {
                        ty.args[0].clone()
                    } else {
                        ty.clone()
                    }),
                    value: default.clone(),
                },
            });
            forwarded.push(if borrowed {
                Expr {
                    at: default.at.clone(),
                    kind: Expression::Unary(
                        if mutable { "&mut" } else { "&" }.into(),
                        Box::new(value),
                    ),
                }
            } else {
                value
            });
        }
        let call = if type_params.is_empty() {
            Expression::Call(name.to_owned(), forwarded)
        } else {
            Expression::GenericCall(
                name.to_owned(),
                type_params
                    .iter()
                    .map(|(parameter, _)| TypeRef {
                        at: at.clone(),
                        concrete: None,
                        name: Some(parameter.clone()),
                        args: vec![],
                    })
                    .collect(),
                forwarded,
            )
        };
        body.push(Stmt {
            at: at.clone(),
            kind: Statement::Return(Some(Expr {
                at: at.clone(),
                kind: call,
            })),
        });
        let wrapper = Function {
            once: false,
            captures: vec![],
            foreign: None,
            export: None,
            name: symbol.clone(),
            type_params,
            at: at.clone(),
            inputs: positions
                .iter()
                .map(|&index| inputs[index].clone())
                .collect(),
            defaults: vec![],
            output,
            doc: String::new(),
            doc_at: None,
            body,
        };
        if wrapper.type_params.is_empty() {
            register_signature(&wrapper, self.aliases, self.sigs)?;
            if let Some(fields) = self.sigs.returned_fields(name).cloned() {
                let wrapper = self.sigs.intern(&symbol);
                self.sigs.set_returned_fields(wrapper, fields);
            }
            self.generics.declare(wrapper, self.sigs);
        } else {
            self.generics.retention.keep(&wrapper.body);
            self.generics
                .templates
                .insert(symbol.clone(), Arc::new(wrapper));
        }
        self.generics
            .inherit_requirement(&self.function_name, &symbol);
        self.generics.call_wrappers.insert(key, symbol.clone());
        Ok(Some((symbol, values)))
    }
}
