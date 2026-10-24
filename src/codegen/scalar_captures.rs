//! Mirrors immutable scalar captures in SSA variables for inlined callbacks.
//! Every assignment defines the mirror, preserving loop and branch joins.
//! Exclusive borrows disable it: an alias could replace the whole closure.
use super::*;
use cranelift_codegen::ir::{MemFlagsData, Value as NativeValue};

#[derive(Default)]
pub(super) struct Captures {
    slots: HashMap<u8, Vec<Variable>>,
    values: HashMap<NativeValue, Vec<NativeValue>>,
    borrows: HashMap<NativeValue, u8>,
}

fn exclusive(ops: &[Op], slot: u8) -> bool {
    ops.iter().any(|op| match op {
        Op::BorrowLocal(i, true) => *i == slot,
        Op::Loop { condition, body } => exclusive(condition, slot) || exclusive(body, slot),
        Op::If {
            then,
            elifs,
            otherwise,
        } => {
            exclusive(then, slot)
                || exclusive(otherwise, slot)
                || elifs
                    .iter()
                    .any(|a| exclusive(&a.condition, slot) || exclusive(&a.body, slot))
        }
        Op::Match(arms) => arms.iter().any(|a| exclusive(&a.body, slot)),
        Op::Try { cleanup, .. } => exclusive(cleanup, slot),
        _ => false,
    })
}

impl Captures {
    pub(super) fn enabled(&self) -> bool {
        !self.slots.is_empty()
    }

    pub(super) fn new(
        ops: &[Op],
        locals: &[(Variable, Ty)],
        parameters: usize,
        functions: &HashMap<String, UserFn>,
        bcx: &mut FunctionBuilder<'_>,
    ) -> Self {
        let mut captures = Self::default();
        for (index, (_, ty)) in locals.iter().enumerate().skip(parameters) {
            let Ty::Closure(closure) = ty else {
                continue;
            };
            if closure.mutable
                || closure.once
                || closure.captures.is_empty()
                || !closure.captures.iter().all(|(_, t)| leaf_calls::scalar(t))
                || exclusive(ops, index as u8)
                || !functions
                    .get(&closure.name)
                    .and_then(|f| f.inline.as_ref())
                    .is_some_and(|f| f.capture_reads(closure.captures.len()))
            {
                continue;
            }
            captures.slots.insert(
                index as u8,
                closure
                    .captures
                    .iter()
                    .map(|(_, ty)| bcx.declare_var(clif_type(ty.clone())))
                    .collect(),
            );
        }
        captures
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn remember_capture_values(&mut self, value: NativeValue, fields: Vec<NativeValue>) {
        self.scalar_captures.values.insert(value, fields);
    }

    pub(super) fn copy_capture_values(&mut self, source: NativeValue, destination: NativeValue) {
        if let Some(fields) = self.scalar_captures.values.get(&source).cloned() {
            self.scalar_captures.values.insert(destination, fields);
        }
    }

    pub(super) fn save_capture_local(&mut self, slot: u8, value: NativeValue) {
        let Some(variables) = self.scalar_captures.slots.get(&slot).cloned() else {
            return;
        };
        let Ty::Closure(closure) = &self.locals[slot as usize].1 else {
            unreachable!()
        };
        let known = self.scalar_captures.values.get(&value).cloned();
        let empty = known::known_zero(&self.bcx.func.dfg, value);
        for (i, variable) in variables.into_iter().enumerate() {
            let ty = &closure.captures[i].1;
            let captured = if let Some(values) = &known {
                values[i]
            } else if empty {
                let zero = self.bcx.ins().iconst(types::I64, 0);
                self.unpack(zero, ty)
            } else {
                let packed = self.bcx.ins().load(
                    types::I128,
                    MemFlagsData::trusted(),
                    value,
                    closure.offset(i) as i32,
                );
                self.unpack(packed, ty)
            };
            self.bcx.def_var(variable, captured);
        }
    }

    pub(super) fn read_capture_local(&mut self, slot: u8, value: NativeValue) {
        if let Some(variables) = self.scalar_captures.slots.get(&slot) {
            let values = variables
                .iter()
                .map(|variable| self.bcx.use_var(*variable))
                .collect();
            self.scalar_captures.values.insert(value, values);
        }
    }

    pub(super) fn borrow_capture_local(&mut self, slot: u8, reference: NativeValue) {
        if self.scalar_captures.slots.contains_key(&slot) {
            self.scalar_captures.borrows.insert(reference, slot);
        }
    }

    pub(super) fn captured_arguments(
        &mut self,
        reference: NativeValue,
    ) -> Option<Vec<NativeValue>> {
        let slot = self.scalar_captures.borrows.get(&reference)?;
        Some(
            self.scalar_captures.slots[slot]
                .iter()
                .map(|variable| self.bcx.use_var(*variable))
                .collect(),
        )
    }
}
