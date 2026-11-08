//! Sum conveniences specialize ordinary Plenty helpers, preserving callback
//! ownership and cleanup through the same checks as user-written matches.
use super::patterns::{Pat, PatKind};
use super::*;

const HELPERS: &str = include_str!("sum_methods.plenty");

pub(super) fn method(name: &str) -> bool {
    matches!(
        name,
        "unwrap_or_else" | "ok_or_else" | "map_or" | "map_or_else" | "expect"
    )
}

impl Lower<'_> {
    pub(super) fn sum_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let start = self.locals.len();
        // Preserve Option's existing contextual inference (notably Nothing).
        // Result's missing error type still needs a receiver annotation.
        let hint = if matches!(name, "unwrap_or_else" | "expect") {
            expected.clone().map(crate::sum::option)
        } else {
            None
        };
        let actual = self.expr_hinted(base, hint, ops)?;
        if let Some(actual) = &actual {
            let receiver = classes::unbox(actual.clone());
            if classes::inherent_receiver(&receiver) {
                self.unbox_to(actual.clone(), &receiver, ops);
                return self.temporary_class_method(
                    base,
                    name,
                    None,
                    args,
                    (receiver, vec![]),
                    ops,
                );
            }
        }
        let Some(Ty::Enum(sum)) = actual else {
            return Err(base
                .at
                .error(format!("{name} requires an owned Option or Result")));
        };
        if !sum.propagatable() || (name == "ok_or_else" && !sum.is_option()) {
            return Err(base.at.error(format!(
                "{name} requires {}",
                if name == "ok_or_else" {
                    "an Option"
                } else {
                    "an Option or Result"
                }
            )));
        }
        let success_tag = usize::from(sum.is_option());
        let payload = sum.get().variants[success_tag].fields[0].clone();
        if name == "expect" {
            return self.sum_expect(base, args, sum, start, ops);
        }
        let count = if name.starts_with("map_or") { 2 } else { 1 };
        if args.len() != count {
            return Err(base.at.error(format!(
                "{name} takes {count} argument(s), got {}",
                args.len()
            )));
        }
        if payload.carries_loan() {
            return self.borrowed_sum_method(base, name, args, sum, expected, start, ops);
        }
        let source_name = self.hide(Ty::Enum(sum.clone()), &base.at, ops)?;
        let mut names = vec![source_name];
        let mut output = if name.starts_with("unwrap_or") {
            Some(payload.clone())
        } else {
            expected
        };
        if name == "ok_or_else" {
            output = match output {
                Some(Ty::Enum(t)) if t.is_result() => Some(t.get().variants[1].fields[0].clone()),
                _ => None,
            };
        }
        for (index, arg) in args.iter().enumerate() {
            let eager = index == 0 && name == "map_or";
            let ty = if eager {
                self.expr_expected(arg, output.clone(), ops)?
                    .unwrap_or_else(|| {
                        ops.push(Op::PushUnit);
                        Ty::Unit
                    })
            } else {
                let inputs = if index == 1 {
                    vec![payload.clone()]
                } else if sum.is_result() {
                    vec![sum.get().variants[1].fields[0].clone()]
                } else {
                    vec![]
                };
                let hint = output.clone().map(|output| {
                    Ty::Callable(Arc::new(crate::op::CallableSig {
                        inputs: inputs.clone(),
                        output: (output != Ty::Unit).then_some(output),
                    }))
                });
                // Context identifies constructor functions such as Ok; it must
                // not coerce a captured closure into a code-pointer Callable.
                let ty = self
                    .expr_hinted(arg, hint, ops)?
                    .ok_or_else(|| arg.at.error(format!("{name} requires a callback")))?;
                let Some(signature) = callables::signature(&ty) else {
                    return Err(arg.at.error(format!("{name} requires an owned callable")));
                };
                let result = signature.output.clone().unwrap_or(Ty::Unit);
                if signature.inputs != inputs {
                    return Err(arg.at.error(format!(
                        "{name} callback must take ({})",
                        inputs
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
                if (index == 1 || name == "unwrap_or_else")
                    && let Some(output) = &output
                {
                    self.same(Some(result.clone()), Some(output.clone()), &arg.at)?;
                }
                output = Some(result);
                ty
            };
            let ty = if eager {
                output = Some(ty.clone());
                // Generic function parameters cannot be bare unit. An inline
                // Option carries every default, including (), without allocating.
                let Ty::Enum(wrapper) = crate::sum::option(ty) else {
                    unreachable!()
                };
                ops.push(Op::Enum(crate::sum::EnumOp::New(wrapper.clone(), 1)));
                Ty::Enum(wrapper)
            } else {
                ty
            };
            names.push(self.hide(ty, &arg.at, ops)?);
        }
        let helper = format!(
            "__plenty_{}_{}",
            if sum.is_option() { "option" } else { "result" },
            name
        );
        if !self.generics.templates.contains_key(&helper) {
            for mut function in modules::generated_interface(HELPERS)?.functions {
                function.name = format!("__plenty_{}", function.name);
                self.generics.validate_template(&function, self.aliases)?;
                self.generics.retention.keep(&function.body);
                self.generics
                    .templates
                    .insert(function.name.clone(), Arc::new(function));
            }
        }
        let arguments = names
            .iter()
            .map(|name| Expr {
                at: base.at.clone(),
                kind: Expression::Name(name.clone()),
            })
            .collect::<Vec<_>>();
        let result = self.generic_call(&helper, None, &arguments, &base.at, ops);
        for name in names {
            self.names.remove(&name);
        }
        let result = result?;
        self.cleanup(start, ops);
        Ok(result)
    }

    /// A borrowed payload cannot move into a generic helper, which would have
    /// to return a borrow of its own parameter. The method lowers as the
    /// `match` it abbreviates instead, so the payload binding, the callbacks'
    /// results and an eager `map_or` default join like the arms of any
    /// value-producing `match`.
    #[allow(clippy::too_many_arguments)]
    fn borrowed_sum_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        sum: crate::nominal::Nominal<crate::sum::EnumType>,
        expected: Type,
        start: usize,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let at = &base.at;
        let parent = self.reference_origin(base, ops)?;
        let source = self.hide(Ty::Enum(sum.clone()), at, ops)?;
        self.reference_locals
            .insert(self.names[&source].slot, parent);
        let mut hidden = vec![source.clone()];
        let name_expr = |name: &str| Expr {
            at: at.clone(),
            kind: Expression::Name(name.into()),
        };
        let call = |callee: &Expr, inputs: Vec<Expr>| Expr {
            at: callee.at.clone(),
            kind: Expression::Invoke(Box::new(callee.clone()), inputs),
        };
        let payload = sum.get().variants[usize::from(sum.is_option())].fields[0].clone();
        let error = sum
            .get()
            .variants
            .get(1)
            .and_then(|v| v.fields.first().cloned());
        // Arguments evaluate eagerly, left to right, as for an owned payload.
        let mut operands = Vec::with_capacity(args.len());
        for (index, arg) in args.iter().enumerate() {
            let hint = if name == "map_or" && index == 0 {
                expected.clone()
            } else {
                let inputs = match (index, error.clone()) {
                    (1, _) => vec![payload.clone()],
                    (_, Some(error)) if sum.is_result() => vec![error],
                    _ => vec![],
                };
                let output = if name == "unwrap_or_else" {
                    Some(payload.clone())
                } else {
                    expected.clone()
                };
                output.map(|output| {
                    Ty::Callable(Arc::new(crate::op::CallableSig {
                        inputs,
                        output: (output != Ty::Unit).then_some(output),
                    }))
                })
            };
            let ty = self
                .expr_hinted(arg, hint, ops)?
                .ok_or_else(|| arg.at.error(format!("{name} requires a value")))?;
            let origin = (ty.carries_loan() && !references::loan_free(arg))
                .then(|| self.reference_origin(arg, ops))
                .transpose()?;
            let operand = self.hide(ty, &arg.at, ops)?;
            if let Some(origin) = origin {
                self.reference_locals
                    .insert(self.names[&operand].slot, origin);
            }
            hidden.push(operand.clone());
            operands.push(name_expr(&operand));
        }
        let payload_name = format!("__plenty_payload{}", self.locals.len());
        let error_name = format!("__plenty_error{}", self.locals.len());
        let mapped = match name {
            "unwrap_or_else" => name_expr(&payload_name),
            _ => call(&operands[1], vec![name_expr(&payload_name)]),
        };
        let fallback = match name {
            "map_or" => operands[0].clone(),
            _ if sum.is_result() => call(&operands[0], vec![name_expr(&error_name)]),
            _ => call(&operands[0], vec![]),
        };
        let binding = |name: &str| Pat {
            at: at.clone(),
            kind: PatKind::Binding(name.into()),
        };
        let variant = |variant: &str, fields: Option<Vec<Pat>>, body: Expr| enums::Case {
            at: at.clone(),
            pattern: Pat {
                at: at.clone(),
                kind: PatKind::Variant(None, variant.into(), fields),
            },
            body: vec![Stmt {
                at: body.at.clone(),
                kind: Statement::Expr(body),
            }],
        };
        let empty = if sum.is_option() {
            variant("Nothing", None, fallback)
        } else if name == "map_or" {
            let wildcard = Pat {
                at: at.clone(),
                kind: PatKind::Wildcard,
            };
            variant("Err", Some(vec![wildcard]), fallback)
        } else {
            variant("Err", Some(vec![binding(&error_name)]), fallback)
        };
        let full = variant(
            if sum.is_option() { "Some" } else { "Ok" },
            Some(vec![binding(&payload_name)]),
            mapped,
        );
        self.plain_reference_arms = Some(match name {
            "map_or" => crate::ownership::Adapter::MapOr,
            "map_or_else" => crate::ownership::Adapter::MapOrElse,
            _ => crate::ownership::Adapter::UnwrapOrElse,
        });
        let result = self.match_value(at, &name_expr(&source), &[full, empty], expected, ops);
        for name in hidden {
            self.names.remove(&name);
        }
        let result = result?;
        self.cleanup(start, ops);
        Ok(result)
    }

    fn sum_expect(
        &mut self,
        base: &Expr,
        args: &[Expr],
        sum: crate::nominal::Nominal<crate::sum::EnumType>,
        start: usize,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let payload = sum.get().variants[usize::from(sum.is_option())].fields[0].clone();
        let [message] = args else {
            return Err(base.at.error("expect takes one message argument"));
        };
        let parent = if payload.carries_loan() {
            Some(self.reference_origin(base, ops)?)
        } else {
            None
        };
        let source = self.slot(Ty::Enum(sum.clone()), &base.at)?;
        ops.push(Op::StoreLocal(source));
        let actual = self.expr_expected(message, Some(Ty::Str), ops)?;
        self.same(actual, Some(Ty::Str), &message.at)?;
        let message = self.slot(Ty::Str, &message.at)?;
        ops.push(Op::StoreLocal(message));
        let tag = usize::from(sum.is_option());
        let yes = vec![
            Op::MoveLocal(source),
            Op::Enum(if payload.affine() {
                crate::sum::EnumOp::Take(sum.clone(), tag, 0)
            } else {
                crate::sum::EnumOp::Field(sum.clone(), tag, 0)
            }),
        ];
        let mut no = vec![Op::MoveLocal(message)];
        self.mark(&base.at, &mut no);
        no.extend([Op::Collection(CollectionOp::Trap), Op::Unreachable]);
        ops.extend([
            Op::LoadLocal(source),
            Op::Enum(crate::sum::EnumOp::Tag(sum)),
            Op::PushInt(Value::I64(tag as i64)),
            Op::Eq,
            branch(yes, no),
        ]);
        if let Some(parent) = parent {
            let root = self.loans[parent].root;
            self.new_loan(root, payload.carries_mutable_loan(), Some(parent), ops);
        }
        self.cleanup(start, ops);
        if payload == Ty::Unit {
            ops.push(Op::Drop);
            Ok(None)
        } else {
            Ok(Some(payload))
        }
    }
}
