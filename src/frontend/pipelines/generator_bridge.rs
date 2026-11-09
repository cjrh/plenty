//! Transfer an already prepared fused plan into one concrete suspended frame.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

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

const BORROWED: &str = "generator conversion requires owned sources and owned callback captures, or a borrowed source; borrowed state cannot be suspended";

impl Lower<'_> {
    pub(in crate::frontend) fn pipeline_generator(
        &mut self,
        source: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let saved = self.names.clone();
        let plan = self.pipeline_source(source, ops)?;
        self.names = saved;
        if plan.item.ty.restricted_storage() {
            return Err(source.at.error(
                "generator elements cannot be references, generators, closures, or scoped tasks",
            ));
        }
        let mut loop_body = plan.iteration.body.clone();
        loop_body.extend(self.pipeline_value_ops(&plan));
        loop_body.push(Op::Yield(plan.item.ty.clone()));
        loop_body.extend(plan.iteration.step.iter().cloned());
        // Plan state that starts empty is frame state, not a capture.
        let mut body: Vec<Op> = plan.empty.iter().copied().map(Op::EmptyLocal).collect();
        body.push(Op::Loop {
            condition: plan.iteration.condition.clone().into(),
            body: loop_body.into(),
        });
        body.extend(plan.iteration.cleanup.iter().cloned());
        for &slot in plan.resources.iter().rev() {
            if self.locals[slot as usize - self.parameters].managed() {
                body.push(Op::DropLocal(slot));
            }
        }
        let mut read = BTreeSet::new();
        let mut loans = BTreeSet::new();
        let mut roots = BTreeMap::new();
        let mut accessed = BTreeSet::new();
        inspect(
            &body,
            &mut read,
            &mut loans,
            &mut (&mut roots, &mut accessed),
        );
        let mut captures: Vec<_> = plan
            .resources
            .iter()
            .copied()
            .filter(|slot| {
                plan.initialized.contains(slot)
                    && (read.contains(slot)
                        || self.locals[*slot as usize - self.parameters].managed())
            })
            .collect();
        // A shared reference or view the plan reads from its creating scope
        // is copied into the frame; the result's loan keeps its referent
        // borrowed (issue #899).
        let shared: Vec<u8> = read
            .iter()
            .copied()
            .filter(|slot| {
                !plan.resources.contains(slot)
                    && accessed.contains(slot)
                    && matches!(
                        self.slot_type(*slot),
                        Some((Ty::Ref(_, false) | Ty::View(_, false), _))
                    )
            })
            .collect();
        captures.extend(&shared);
        // The plan reads a borrowed source through its own state; loans
        // rooted at the source, a reference or a local owner, stay in the
        // frame rooted at an empty anchor (issue #899).
        let anchors: BTreeMap<u8, bool> = roots
            .iter()
            .filter(|(slot, _)| {
                !plan.resources.contains(slot)
                    && !accessed.contains(slot)
                    && !captures.contains(slot)
            })
            .map(|(slot, mutable)| (*slot, *mutable))
            .collect();
        captures.extend(anchors.keys());
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
            .map(|slot| match anchors.contains_key(slot) {
                true => Ty::Unit,
                false => self.slot_type(*slot).expect("plan slot").0.clone(),
            })
            .collect();
        // A borrowed source reaches the frame as a reference, view or
        // borrowing generator, whose loans the caller keeps while the frame
        // lives (issue #899). A borrowing callback environment has no such
        // loan.
        let lends = |ty: &Ty| matches!(ty, Ty::Ref(..) | Ty::View(..)) || ty.lends_frame();
        if types[..captures.len()]
            .iter()
            .any(|ty| ty.contains_reference() && !lends(ty))
        {
            return Err(source.at.error(
                "generator conversion cannot suspend a callback that borrows; capture owned values with `def [name]`",
            ));
        }
        let lends = !anchors.is_empty() || types[..captures.len()].iter().any(lends);
        // Without a borrowed capture, unowned state such as a column plan's
        // raw rows has no loan the frame could carry.
        if !plan.owned && !lends {
            return Err(source.at.error(BORROWED));
        }
        let mut held = plan.loans.clone();
        for &slot in &captures {
            match self.reference_locals.get(&slot) {
                Some(&loan) => held.push(loan),
                // A reference parameter or a borrowed local owner has no
                // loan of its own here; one rooted at it ties the frame to
                // it, unless the plan already holds one.
                None if (shared.contains(&slot) || anchors.contains_key(&slot))
                    && !plan.loans.iter().any(|&l| self.loans[l].root == slot) =>
                {
                    let mutable = anchors.get(&slot).copied().unwrap_or(false);
                    let loan = self.new_loan(slot, mutable, None, ops);
                    held.push(loan);
                }
                None => {}
            }
        }
        self.outermost_loans(&mut held);
        let name = format!(
            "__plenty_pipeline_{}_{}_{}_{}",
            self.function_name, source.at.line, source.at.column, self.pipeline.generators
        );
        self.pipeline.generators += 1;
        if let Some(sig) = self.sigs.get(&name) {
            let sig = sig.clone();
            ops.extend(capture_ops(&captures, &shared, &anchors));
            self.record_direct_call(&name, &source.at, &[], ops);
            self.push_call(Op::Call(name), &sig.inputs, &[], &source.at, ops)?;
            if lends {
                self.lending_frame_result(&held, ops);
            }
            return Ok(sig.outputs[0].clone());
        }
        let ty = crate::generator::producer(plan.item.ty.clone(), Some(name.clone()), lends);
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
        let frame = Frame {
            slots: &slots,
            anchors: &anchors,
            loans: &loans,
        };
        let body = relocate(&body, &frame, &source.at)?;
        let facts = source
            .at
            .source()
            .as_ref()
            .map(|path| self.generics.source_facts(path));
        self.generics
            .inherit_requirement(self.sigs, &self.function_name, &name);
        self.sigs.insert(name.clone(), sig.clone());
        let parameter_values = self.parameter_values();
        self.generics.compiled.push(Op::DefineFn(
            name.clone(),
            Box::new(CompiledFn {
                location: source.at.file.map(|_| source.at.position()),
                origins: Arc::new(crate::ownership::Origins {
                    parameter_values: order
                        .iter()
                        .map(|slot| {
                            parameter_values
                                .get(*slot as usize)
                                .cloned()
                                .flatten()
                                .filter(|_| !anchors.contains_key(slot))
                        })
                        .collect(),
                    standard_library: facts.as_ref().is_some_and(|(_, standard)| *standard),
                    source: source.at.source(),
                    trap_file: facts.map(|(relative, _)| relative),
                    names: order
                        .iter()
                        .map(|slot| self.slot_names.get(*slot as usize).cloned().flatten())
                        .collect(),
                    required: self.generics.requirement_of(self.sigs, &name).cloned(),
                    display: None,
                }),
                generator: Some(plan.item.ty),
                sig: sig.clone(),
                doc: "".into(),
                body: body.into(),
                locals: types[captures.len()..].to_vec().into(),
            }),
        ));
        ops.extend(capture_ops(&captures, &shared, &anchors));
        self.record_direct_call(&name, &source.at, &[], ops);
        self.push_call(Op::Call(name), &sig.inputs, &[], &source.at, ops)?;
        if lends {
            self.lending_frame_result(&held, ops);
        }
        Ok(ty)
    }
}

/// The arguments passing `captures` to the frame: a reference is copied, an
/// anchor is empty and other state moves.
fn capture_ops<'a>(
    captures: &'a [u8],
    shared: &'a [u8],
    anchors: &'a BTreeMap<u8, bool>,
) -> impl Iterator<Item = Op> + 'a {
    captures.iter().map(|slot| {
        if anchors.contains_key(slot) {
            Op::PushUnit
        } else if shared.contains(slot) {
            Op::LoadLocal(*slot)
        } else {
            Op::MoveLocal(*slot)
        }
    })
}

/// Collects the slots `ops` read and the loans they make. `roots` gets each
/// loan root with whether a loan of it is exclusive, and the slots read
/// other than as a loan root.
impl Lower<'_> {
    /// Keeps only the loans in `held` that no other one in it is an
    /// ancestor of. An ancestor covers its descendants' footprint, and the
    /// frame's result loan derives from these, so it does not conflict with
    /// an exclusive sibling such as a plan's per-item loan (issue #899).
    #[inline(never)]
    fn outermost_loans(&self, held: &mut Vec<usize>) {
        let all = held.clone();
        held.retain(|&loan| {
            let mut ancestor = self.loans[loan].parent;
            while let Some(id) = ancestor {
                if all.contains(&id) && self.loans[loan].dependencies.is_empty() {
                    return false;
                }
                ancestor = self.loans[id].parent;
            }
            true
        });
    }
}

fn inspect(
    ops: &[Op],
    read: &mut BTreeSet<u8>,
    loans: &mut BTreeSet<usize>,
    roots: &mut (&mut BTreeMap<u8, bool>, &mut BTreeSet<u8>),
) {
    for op in ops {
        match op {
            Op::LoadLocal(slot)
            | Op::MoveLocal(slot)
            | Op::BorrowLocal(slot, _)
            | Op::Next(slot) => {
                read.insert(*slot);
                roots.1.insert(*slot);
            }
            Op::Loan(loan) => {
                loans.insert(loan.id);
                // A loan through a reference parameter names it as its root.
                read.insert(loan.root);
                read.extend(loan.through);
                *roots.0.entry(loan.root).or_default() |= loan.mutable;
            }
            Op::Loop { condition, body } => {
                inspect(condition, read, loans, roots);
                inspect(body, read, loans, roots);
            }
            Op::CheckedBlock { body, .. }
            | Op::CheckedFunction { body, .. }
            | Op::CheckedArith { cleanup: body, .. } => {
                inspect(body, read, loans, roots);
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    inspect(&arm.body, read, loans, roots);
                }
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                inspect(then, read, loans, roots);
                for arm in elifs.iter() {
                    inspect(&arm.condition, read, loans, roots);
                    inspect(&arm.body, read, loans, roots);
                }
                inspect(otherwise, read, loans, roots);
            }
            _ => {}
        }
    }
}

/// Where a plan's state lives in its frame.
struct Frame<'a> {
    slots: &'a HashMap<u8, u8>,
    /// Slots that appear only as loan roots, whose frame slot is empty.
    anchors: &'a BTreeMap<u8, bool>,
    loans: &'a BTreeSet<usize>,
}

fn relocate(ops: &[Op], frame: &Frame, at: &Token) -> Result<Vec<Op>> {
    let Frame {
        slots,
        anchors,
        loans,
    } = *frame;
    let root = |old: &u8| slots.get(old).copied().ok_or_else(|| at.error(BORROWED));
    let slot = |old: &u8| {
        if anchors.contains_key(old) {
            return Err(at.error(BORROWED));
        }
        root(old)
    };
    let mut result = Vec::with_capacity(ops.len());
    for op in ops {
        result.push(match op {
            Op::LoadLocal(i) => Op::LoadLocal(slot(i)?),
            Op::MoveLocal(i) => Op::MoveLocal(slot(i)?),
            Op::StoreLocal(i) => Op::StoreLocal(slot(i)?),
            Op::DropLocal(i) => Op::DropLocal(slot(i)?),
            Op::EmptyLocal(i) => Op::EmptyLocal(slot(i)?),
            Op::BorrowLocal(i, mutable) => Op::BorrowLocal(slot(i)?, *mutable),
            Op::Next(i) => Op::Next(slot(i)?),
            // Loans made before the frame stay with the creating scope: the
            // frame's own reference parameters stand for them.
            Op::Access(i, mutable, loan) => {
                Op::Access(slot(i)?, *mutable, loan.filter(|id| loans.contains(id)))
            }
            Op::FieldMove(fact) => Op::FieldMove(fact.with_root(slot(&fact.root())?)),
            Op::Loan(loan) => {
                let mut loan = loan.clone();
                loan.root = root(&loan.root)?;
                // Diagnostic only, so a source outside the frame is dropped.
                loan.through = loan.through.and_then(|i| slots.get(&i).copied());
                loan.binding = loan.binding.map(|i| slot(&i)).transpose()?;
                loan.parent = loan.parent.filter(|id| loans.contains(id));
                loan.dependencies.retain(|id| loans.contains(id));
                loan.holder = loan.holder.filter(|held| loans.contains(&held.holder));
                Op::Loan(loan)
            }
            Op::UseLoan(id) if !loans.contains(id) => continue,
            Op::Loop { condition, body } => Op::Loop {
                condition: relocate(condition, frame, at)?.into(),
                body: relocate(body, frame, at)?.into(),
            },
            Op::If {
                then,
                elifs,
                otherwise,
            } => Op::If {
                then: relocate(then, frame, at)?.into(),
                elifs: elifs
                    .iter()
                    .map(|arm| {
                        Ok(ElifArm {
                            condition: relocate(&arm.condition, frame, at)?.into(),
                            body: relocate(&arm.body, frame, at)?.into(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
                    .into(),
                otherwise: relocate(otherwise, frame, at)?.into(),
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
                cleanup: relocate(cleanup, frame, at)?.into(),
            },
            Op::CheckedBlock { body, result } => Op::CheckedBlock {
                body: relocate(body, frame, at)?.into(),
                result: result.clone(),
            },
            Op::CheckedFunction { body, result } => Op::CheckedFunction {
                body: relocate(body, frame, at)?.into(),
                result: result.clone(),
            },
            Op::CheckedArith {
                operation,
                ty,
                cleanup,
            } => Op::CheckedArith {
                operation: *operation,
                ty: ty.clone(),
                cleanup: relocate(cleanup, frame, at)?.into(),
            },

            Op::Match(arms) => Op::Match(
                arms.iter()
                    .map(|arm| {
                        Ok(MatchArm {
                            pattern: arm.pattern,
                            body: relocate(&arm.body, frame, at)?.into(),
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
