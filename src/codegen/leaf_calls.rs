//! Bounded scalar leaf substitution during native lowering. The typed IR has
//! already passed ownership checks; keeping the callee's source context here
//! preserves trapping arithmetic locations across modules and std boundaries.
use super::*;

const MAX_OPERATIONS: usize = 24;

pub(super) struct Leaf {
    body: Arc<[Op]>,
    sig: Arc<FnSig>,
    source: Option<Arc<str>>,
    standard_library: bool,
}

pub(super) fn scalar(ty: &Ty) -> bool {
    ty.is_numeric() || *ty == Ty::Bool
}

fn marker(op: &Op) -> bool {
    matches!(
        op,
        Op::Site(_) | Op::CallerSite(_) | Op::Access(..) | Op::Loan(_) | Op::UseLoan(_)
    )
}

impl Leaf {
    pub(super) fn capture_reads(&self, captures: usize) -> bool {
        self.body.iter().enumerate().all(|(index, op)| {
            !matches!(op, Op::LoadLocal(i) if (*i as usize) < captures)
                || matches!(self.body[index + 1..].iter().find(|op| !marker(op)), Some(Op::ReadRef(ty)) if scalar(ty))
        })
    }
}

pub(super) fn prepare(bodies: &[(String, Arc<[Op]>)], functions: &mut HashMap<String, UserFn>) {
    for (name, body) in bodies {
        let function = functions.get_mut(name).unwrap();
        if function.generator.is_some()
            || !function.locals.is_empty()
            || !function
                .sig
                .inputs
                .iter()
                .all(|(_, ty)| scalar(ty) || matches!(ty, Ty::Ref(t, false) if scalar(t)))
            || !matches!(function.sig.outputs.as_slice(), [ty] if scalar(ty))
        {
            continue;
        }
        let mut count = 0;
        let eligible = body.iter().all(|op| {
            if marker(op) {
                return true;
            }
            count += 1;
            count <= MAX_OPERATIONS
                && match op {
                    Op::LoadLocal(slot) => (*slot as usize) < function.sig.inputs.len(),
                    Op::ReadRef(ty) => scalar(ty),
                    Op::PushInt(_)
                    | Op::PushFloat { .. }
                    | Op::PushBool(_)
                    | Op::Add
                    | Op::Sub
                    | Op::Mul
                    | Op::Div
                    | Op::FloorDiv
                    | Op::Modulo
                    | Op::FloatNeg
                    | Op::Integer(_)
                    | Op::Eq
                    | Op::Ne
                    | Op::Lt
                    | Op::Le
                    | Op::Gt
                    | Op::Ge
                    | Op::Not
                    | Op::And
                    | Op::Or => true,
                    _ => false,
                }
        });
        if eligible {
            function.inline = Some(Arc::new(Leaf {
                body: body.clone(),
                sig: function.sig.clone(),
                source: function.source.clone(),
                standard_library: function.standard_library,
            }));
        }
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_leaf_call(
        &mut self,
        decl: &Leaf,
        args: &[cranelift_codegen::ir::Value],
        captures: &[cranelift_codegen::ir::Value],
    ) -> Result<()> {
        let body = &decl.body;
        let position = std::mem::replace(
            &mut self.position,
            sites::Position::in_file(decl.source.clone(), decl.standard_library),
        );
        let source_location = self.bcx.srcloc();
        let lowered: Result<()> = (|| {
            let mut index = 0;
            while index < body.len() {
                let op = &body[index];
                if let Op::LoadLocal(slot) = op {
                    if let Some(value) = captures.get(*slot as usize) {
                        index += 1;
                        while marker(&body[index]) {
                            self.lower(&body[index])?;
                            index += 1;
                        }
                        let Op::ReadRef(ty) = &body[index] else {
                            unreachable!("validated capture read");
                        };
                        self.stack.push((*value, ty.clone()));
                    } else {
                        self.stack.push((
                            args[*slot as usize],
                            decl.sig.inputs[*slot as usize].1.clone(),
                        ));
                    }
                } else {
                    self.lower(op)?;
                }
                index += 1;
            }
            Ok(())
        })();
        self.position = position;
        self.bcx.set_srcloc(source_location);
        lowered?;
        Ok(())
    }
}
