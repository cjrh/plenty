//! Transfer an already prepared fused plan into one concrete suspended frame.
use super::*;
use std::collections::BTreeSet;

/// Construction can contain arbitrary source/callback expressions. Only slots
/// definitely initialized on every continuing branch become frame parameters;
/// per-item scratch slots stay empty until the first resume.
pub(super) fn initialized_after(ops: &[Op], initialized: &mut BTreeSet<u8>) {
    for op in ops {
        match op {
            Op::StoreLocal(slot) => {
                initialized.insert(*slot);
            }
            Op::MoveLocal(slot) | Op::DropLocal(slot) => {
                initialized.remove(slot);
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                let before = initialized.clone();
                initialized_after(then, initialized);
                for arm in elifs.iter() {
                    let mut alternative = before.clone();
                    initialized_after(&arm.condition, &mut alternative);
                    initialized_after(&arm.body, &mut alternative);
                    initialized.retain(|slot| alternative.contains(slot));
                }
                let mut alternative = before;
                initialized_after(otherwise, &mut alternative);
                initialized.retain(|slot| alternative.contains(slot));
            }
            Op::Match(arms) => {
                let before = initialized.clone();
                for (i, arm) in arms.iter().enumerate() {
                    let mut alternative = before.clone();
                    initialized_after(&arm.body, &mut alternative);
                    if i == 0 {
                        *initialized = alternative;
                    } else {
                        initialized.retain(|slot| alternative.contains(slot));
                    }
                }
            }
            _ => {}
        }
    }
}

impl Lower<'_> {
    pub(in crate::frontend) fn pipeline_generator(
        &mut self,
        source: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let saved = self.names.clone();
        let plan = self.pipeline_source(source, ops)?;
        self.names = saved;
        if !plan.owned {
            return Err(source.at.error("generator conversion requires owned sources and owned callback captures; borrowed state cannot be suspended"));
        }
        if plan.item.ty.restricted_storage() {
            return Err(source.at.error(
                "generator elements cannot be references, generators, closures, or scoped tasks",
            ));
        }
        let mut loop_body = plan.iteration.body.clone();
        loop_body.extend(self.pipeline_value_ops(&plan));
        loop_body.push(Op::Yield(plan.item.ty.clone()));
        loop_body.extend(plan.iteration.step.iter().cloned());
        let mut body = vec![Op::Loop {
            condition: plan.iteration.condition.clone().into(),
            body: loop_body.into(),
        }];
        body.extend(plan.iteration.cleanup.iter().cloned());
        for &slot in plan.resources.iter().rev() {
            if self.locals[slot as usize - self.parameters].managed() {
                body.push(Op::DropLocal(slot));
            }
        }
        let mut read = BTreeSet::new();
        let mut loans = BTreeSet::new();
        inspect(&body, &mut read, &mut loans);
        let captures: Vec<_> = plan
            .resources
            .iter()
            .copied()
            .filter(|slot| {
                plan.initialized.contains(slot)
                    && (read.contains(slot)
                        || self.locals[*slot as usize - self.parameters].managed())
            })
            .collect();
        let order: Vec<_> = captures
            .iter()
            .copied()
            .chain(
                plan.resources
                    .iter()
                    .copied()
                    .filter(|slot| !captures.contains(slot)),
            )
            .collect();
        let slots: HashMap<_, _> = order
            .iter()
            .enumerate()
            .map(|(new, old)| (*old, new as u8))
            .collect();
        let types: Vec<_> = order
            .iter()
            .map(|slot| self.locals[*slot as usize - self.parameters].clone())
            .collect();
        if types[..captures.len()].iter().any(Ty::contains_reference) {
            return Err(source
                .at
                .error("generator conversion cannot suspend reference-containing state"));
        }
        let name = format!(
            "__plenty_pipeline_{}_{}_{}_{}",
            self.function_name, source.at.line, source.at.column, self.pipeline_generators
        );
        self.pipeline_generators += 1;
        if let Some(sig) = self.sigs.get(&name) {
            let sig = sig.clone();
            for &slot in &captures {
                ops.push(Op::MoveLocal(slot));
            }
            self.record_direct_call(&name, &source.at, &[], ops);
            self.push_call(Op::Call(name), &sig.inputs, &[], ops);
            return Ok(sig.outputs[0].clone());
        }
        let ty = crate::generator::ty(plan.item.ty.clone(), Some(name.clone()));
        let Ty::Generator(frame) = &ty else {
            unreachable!()
        };
        frame
            .set_slots(types.clone())
            .map_err(|message| source.at.error(message))?;
        let sig = Arc::new(FnSig {
            inputs: types[..captures.len()]
                .iter()
                .enumerate()
                .map(|(i, ty)| (format!("<pipeline state {i}>"), ty.clone()))
                .collect(),
            outputs: vec![ty.clone()],
        });
        let body = relocate(&body, &slots, &loans, &source.at)?;
        let facts = source
            .at
            .source
            .as_ref()
            .map(|path| self.generics.source_facts(path));
        self.generics
            .inherit_requirement(&self.function_name, &name);
        self.sigs.insert(name.clone(), sig.clone());
        self.generics.compiled.push(Op::DefineFn(
            name.clone(),
            Box::new(CompiledFn {
                location: source
                    .at
                    .source
                    .as_ref()
                    .map(|path| format!("{path}:{}:{}", source.at.line, source.at.column).into()),
                origins: Arc::new(crate::ownership::Origins {
                    standard_library: facts.as_ref().is_some_and(|(_, standard)| *standard),
                    source: source.at.source.clone(),
                    trap_file: facts.map(|(relative, _)| relative),
                    names: order
                        .iter()
                        .map(|slot| self.slot_names.get(*slot as usize).cloned().flatten())
                        .collect(),
                    required: self.generics.required.get(&name).cloned(),
                }),
                generator: Some(plan.item.ty),
                sig: sig.clone(),
                doc: "".into(),
                body: body.into(),
                locals: types[captures.len()..].to_vec().into(),
            }),
        ));
        for slot in captures {
            ops.push(Op::MoveLocal(slot));
        }
        self.record_direct_call(&name, &source.at, &[], ops);
        self.push_call(Op::Call(name), &sig.inputs, &[], ops);
        Ok(ty)
    }
}

fn inspect(ops: &[Op], read: &mut BTreeSet<u8>, loans: &mut BTreeSet<usize>) {
    for op in ops {
        match op {
            Op::LoadLocal(slot)
            | Op::MoveLocal(slot)
            | Op::BorrowLocal(slot, _)
            | Op::Next(slot) => {
                read.insert(*slot);
            }
            Op::Loan(loan) => {
                loans.insert(loan.id);
            }
            Op::Loop { condition, body } => {
                inspect(condition, read, loans);
                inspect(body, read, loans);
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    inspect(&arm.body, read, loans);
                }
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                inspect(then, read, loans);
                for arm in elifs.iter() {
                    inspect(&arm.condition, read, loans);
                    inspect(&arm.body, read, loans);
                }
                inspect(otherwise, read, loans);
            }
            _ => {}
        }
    }
}

fn relocate(
    ops: &[Op],
    slots: &HashMap<u8, u8>,
    loans: &BTreeSet<usize>,
    at: &Token,
) -> Result<Vec<Op>> {
    let slot = |old: &u8| {
        slots.get(old).copied().ok_or_else(|| {
            at.error("generator conversion cannot retain a reference to its creating scope")
        })
    };
    let mut result = Vec::with_capacity(ops.len());
    for op in ops {
        result.push(match op {
            Op::LoadLocal(i) => Op::LoadLocal(slot(i)?),
            Op::MoveLocal(i) => Op::MoveLocal(slot(i)?),
            Op::StoreLocal(i) => Op::StoreLocal(slot(i)?),
            Op::DropLocal(i) => Op::DropLocal(slot(i)?),
            Op::BorrowLocal(i, mutable) => Op::BorrowLocal(slot(i)?, *mutable),
            Op::Next(i) => Op::Next(slot(i)?),
            Op::Access(i, mutable, loan) => Op::Access(slot(i)?, *mutable, *loan),
            Op::Loan(loan) => {
                let mut loan = loan.clone();
                loan.root = slot(&loan.root)?;
                loan.through = loan.through.map(|i| slot(&i)).transpose()?;
                loan.binding = loan.binding.map(|i| slot(&i)).transpose()?;
                Op::Loan(loan)
            }
            Op::UseLoan(id) if !loans.contains(id) => continue,
            Op::Loop { condition, body } => Op::Loop {
                condition: relocate(condition, slots, loans, at)?.into(),
                body: relocate(body, slots, loans, at)?.into(),
            },
            Op::If {
                then,
                elifs,
                otherwise,
            } => Op::If {
                then: relocate(then, slots, loans, at)?.into(),
                elifs: elifs
                    .iter()
                    .map(|arm| {
                        Ok(ElifArm {
                            condition: relocate(&arm.condition, slots, loans, at)?.into(),
                            body: relocate(&arm.body, slots, loans, at)?.into(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
                    .into(),
                otherwise: relocate(otherwise, slots, loans, at)?.into(),
            },
            Op::Passes(values) => {
                Op::Passes(values.iter().map(slot).collect::<Result<Vec<_>>>()?.into())
            }
            Op::Returning(site, values) => Op::Returning(
                *site,
                values.iter().map(slot).collect::<Result<Vec<_>>>()?.into(),
            ),
            Op::Building(Building::Open(site, values)) => Op::Building(Building::Open(
                *site,
                values.iter().map(slot).collect::<Result<Vec<_>>>()?.into(),
            )),
            Op::Building(Building::Hold(site, values)) => Op::Building(Building::Hold(
                *site,
                values.iter().map(slot).collect::<Result<Vec<_>>>()?.into(),
            )),
            Op::Tokens(crate::op::TokenMark::Holds(values)) => {
                Op::Tokens(crate::op::TokenMark::Holds(
                    values.iter().map(slot).collect::<Result<Vec<_>>>()?.into(),
                ))
            }
            Op::Tokens(crate::op::TokenMark::Guard(i)) => {
                Op::Tokens(crate::op::TokenMark::Guard(slot(i)?))
            }
            Op::Try {
                source,
                target,
                cleanup,
            } => Op::Try {
                source: source.clone(),
                target: target.clone(),
                cleanup: relocate(cleanup, slots, loans, at)?.into(),
            },
            Op::Match(arms) => Op::Match(
                arms.iter()
                    .map(|arm| {
                        Ok(MatchArm {
                            pattern: arm.pattern,
                            body: relocate(&arm.body, slots, loans, at)?.into(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
                    .into(),
            ),
            _ => op.clone(),
        });
    }
    Ok(result)
}
