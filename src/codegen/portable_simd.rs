//! Native lowering for the closed, trusted std.simd surface. Public values keep
//! the ordinary inline-enum ABI; eligible locals stay in private vector SSA.
//! The Plenty bodies remain the scalar implementation on every other target.
use super::*;
use cranelift_codegen::ir::{Inst, MemFlagsData, Value as NativeValue};

#[derive(Clone, Copy)]
enum Shape {
    I32,
    U32,
    F32,
    F64,
}

impl Shape {
    fn lane(self) -> Ty {
        match self {
            Self::I32 => Ty::I32,
            Self::U32 => Ty::U32,
            Self::F32 => Ty::F32,
            Self::F64 => Ty::F64,
        }
    }

    fn vector(self) -> cranelift_codegen::ir::Type {
        match self {
            Self::I32 | Self::U32 => types::I32X4,
            Self::F32 => types::F32X4,
            Self::F64 => types::F64X2,
        }
    }

    fn mask(self) -> &'static str {
        if matches!(self, Self::F64) {
            "mask64x2"
        } else {
            "mask32x4"
        }
    }

    fn lanes(self) -> usize {
        self.vector().lane_count() as usize
    }
}

#[derive(Clone, Copy)]
enum Action {
    Add,
    Sub,
    Mul,
    Div,
    Compare(IntCC, FloatCC),
    Select,
}

pub(super) struct Operation {
    shape: Shape,
    action: Action,
    output: Ty,
}

/// Only exact numeric vector locals without mutable borrows are promoted. A
/// materialized shared borrow is a snapshot: the ownership checker prevents
/// mutation of the source for its entire lifetime. Mutable borrows and moved
/// slots retain the ordinary canonical storage path throughout the function.
#[derive(Default)]
pub(super) struct State {
    locals: HashMap<u8, (Variable, Shape)>,
    values: HashMap<NativeValue, NativeValue>,
    snapshots: Vec<Vec<Inst>>,
}

fn shape_of(ty: &Ty) -> Option<Shape> {
    let Ty::Enum(t) = ty else { return None };
    let name = t.name.strip_prefix("std.simd.")?;
    let shape = match name {
        "i32x4" => Shape::I32,
        "u32x4" => Shape::U32,
        "f32x4" => Shape::F32,
        "f64x2" => Shape::F64,
        _ => return None,
    };
    logical_type(ty, name, &shape.lane(), shape.lanes()).then_some(shape)
}

impl State {
    pub(super) fn new(
        body: &[Op],
        locals: &[(Variable, Ty)],
        inputs: usize,
        builder: &mut FunctionBuilder<'_>,
        enabled: bool,
    ) -> Self {
        if !enabled || !cfg!(target_arch = "x86_64") {
            return Self::default();
        }
        if !locals
            .iter()
            .skip(inputs)
            .any(|(_, ty)| shape_of(ty).is_some())
        {
            return Self::default();
        }
        let mut excluded = vec![false; locals.len()];
        fn visit(body: &[Op], excluded: &mut [bool]) {
            for op in body {
                match op {
                    Op::BorrowLocal(index, true) | Op::MoveLocal(index) => {
                        excluded[*index as usize] = true
                    }
                    Op::If {
                        then,
                        elifs,
                        otherwise,
                    } => {
                        visit(then, excluded);
                        for arm in elifs.iter() {
                            visit(&arm.condition, excluded);
                            visit(&arm.body, excluded);
                        }
                        visit(otherwise, excluded);
                    }
                    Op::Loop { condition, body } => {
                        visit(condition, excluded);
                        visit(body, excluded);
                    }
                    Op::Match(arms) => {
                        for arm in arms.iter() {
                            visit(&arm.body, excluded);
                        }
                    }
                    Op::Try { cleanup, .. } => visit(cleanup, excluded),
                    _ => {}
                }
            }
        }
        visit(body, &mut excluded);
        let locals = locals
            .iter()
            .enumerate()
            .skip(inputs)
            .filter_map(|(index, (_, ty))| {
                if excluded[index] {
                    return None;
                }
                let shape = shape_of(ty)?;
                Some((index as u8, (builder.declare_var(shape.vector()), shape)))
            })
            .collect();
        Self {
            locals,
            values: HashMap::default(),
            snapshots: Vec::new(),
        }
    }

    pub(super) fn promoted(&self, index: u8) -> bool {
        self.locals.contains_key(&index)
    }
    pub(super) fn fresh(&self, value: NativeValue) -> bool {
        self.values.contains_key(&value)
    }
}

fn logical_type(ty: &Ty, name: &str, lane: &Ty, count: usize) -> bool {
    let Ty::Enum(t) = ty else { return false };
    let t = t.get();
    t.name.strip_prefix("std.simd.") == Some(name)
        && t.variants.len() == 1
        && t.variants[0].name == "Lanes"
        && t.variants[0].fields.len() == count
        && t.variants[0].fields.iter().all(|field| field == lane)
}

fn borrowed(ty: &Ty, name: &str, lane: &Ty, count: usize) -> bool {
    matches!(ty, Ty::Ref(t, false) if logical_type(t, name, lane, count))
}

/// Capability table for the baseline host backend, checked before emitting any
/// vector IR. Other hosts and unoptimized builds execute the scalar module.
pub(super) fn operation(name: &str, decl: &UserFn, optimize: bool) -> Option<Operation> {
    if !cfg!(target_arch = "x86_64") || !optimize || !decl.standard_library {
        return None;
    }
    let (name, method) = name
        .strip_prefix("__plenty_class_std.simd.")?
        .split_once('.')?;
    let shape = match name {
        "i32x4" => Shape::I32,
        "u32x4" => Shape::U32,
        "f32x4" => Shape::F32,
        "f64x2" => Shape::F64,
        _ => return None,
    };
    let float = shape.lane().is_float();
    let unsigned = matches!(shape, Shape::U32);
    let action = match method {
        "wrapping_add" if !float => Action::Add,
        "wrapping_sub" if !float => Action::Sub,
        "wrapping_mul" if !float => Action::Mul,
        "add" if float => Action::Add,
        "sub" if float => Action::Sub,
        "mul" if float => Action::Mul,
        "div" if float => Action::Div,
        "simd_eq" => Action::Compare(IntCC::Equal, FloatCC::Equal),
        "simd_ne" => Action::Compare(IntCC::NotEqual, FloatCC::NotEqual),
        "simd_lt" => Action::Compare(
            if unsigned {
                IntCC::UnsignedLessThan
            } else {
                IntCC::SignedLessThan
            },
            FloatCC::LessThan,
        ),
        "simd_le" => Action::Compare(
            if unsigned {
                IntCC::UnsignedLessThanOrEqual
            } else {
                IntCC::SignedLessThanOrEqual
            },
            FloatCC::LessThanOrEqual,
        ),
        "simd_gt" => Action::Compare(
            if unsigned {
                IntCC::UnsignedGreaterThan
            } else {
                IntCC::SignedGreaterThan
            },
            FloatCC::GreaterThan,
        ),
        "simd_ge" => Action::Compare(
            if unsigned {
                IntCC::UnsignedGreaterThanOrEqual
            } else {
                IntCC::SignedGreaterThanOrEqual
            },
            FloatCC::GreaterThanOrEqual,
        ),
        "selected" => Action::Select,
        _ => return None,
    };
    let input = &decl.sig.inputs;
    let [output] = decl.sig.outputs.as_slice() else {
        return None;
    };
    let selected = matches!(action, Action::Select);
    let right = if selected { 2 } else { 1 };
    if input.len() != right + 1
        || !borrowed(&input[0].1, name, &shape.lane(), shape.lanes())
        || !borrowed(&input[right].1, name, &shape.lane(), shape.lanes())
        || (selected && !borrowed(&input[1].1, shape.mask(), &Ty::Bool, shape.lanes()))
    {
        return None;
    }
    let comparison = matches!(action, Action::Compare(..));
    if !logical_type(
        output,
        if comparison { shape.mask() } else { name },
        &if comparison { Ty::Bool } else { shape.lane() },
        shape.lanes(),
    ) {
        return None;
    }
    Some(Operation {
        shape,
        action,
        output: output.clone(),
    })
}

impl Lowerer<'_, '_> {
    fn record_simd_snapshot(&mut self, before: Option<Inst>) {
        let block = self.bcx.current_block().expect("SIMD snapshot block");
        let layout = &self.bcx.func.layout;
        let mut next = before.map_or(layout.first_inst(block), |inst| layout.next_inst(inst));
        let mut instructions = Vec::new();
        while let Some(inst) = next {
            instructions.push(inst);
            next = layout.next_inst(inst);
        }
        self.portable_simd.snapshots.push(instructions);
    }

    /// Cranelift conservatively retains stores even in an unused stack slot.
    /// Remove only our private materializations whose SSA results never reach
    /// any other instruction. References between snapshots retain dependencies;
    /// calls, returns and block arguments count as observable uses too.
    pub(super) fn finish_portable_simd(&mut self) {
        let groups = &self.portable_simd.snapshots;
        if groups.is_empty() {
            return;
        }
        let mut owners = HashMap::default();
        let mut values = HashMap::default();
        for (group, instructions) in groups.iter().enumerate() {
            for &inst in instructions {
                owners.insert(inst, group);
                for &value in self.bcx.func.dfg.inst_results(inst) {
                    values.insert(value, group);
                }
            }
        }
        let mut dependencies = vec![Vec::new(); groups.len()];
        let mut live = vec![false; groups.len()];
        let mut work = Vec::new();
        for block in self.bcx.func.layout.blocks() {
            for inst in self.bcx.func.layout.block_insts(block) {
                for value in self.bcx.func.dfg.inst_values(inst) {
                    let value = self.bcx.func.dfg.resolve_aliases(value);
                    let Some(&producer) = values.get(&value) else {
                        continue;
                    };
                    match owners.get(&inst) {
                        Some(&consumer) if producer != consumer => {
                            dependencies[consumer].push(producer)
                        }
                        Some(_) => {}
                        None if !live[producer] => {
                            live[producer] = true;
                            work.push(producer);
                        }
                        None => {}
                    }
                }
            }
        }
        while let Some(consumer) = work.pop() {
            for &producer in &dependencies[consumer] {
                if !live[producer] {
                    live[producer] = true;
                    work.push(producer);
                }
            }
        }
        for (group, instructions) in groups.iter().enumerate() {
            if !live[group] {
                for &inst in instructions {
                    self.bcx.func.layout.remove_inst(inst);
                }
            }
        }
    }

    fn materialize_simd(&mut self, vector: NativeValue, shape: Shape, ty: &Ty) -> NativeValue {
        let before = self
            .bcx
            .func
            .layout
            .last_inst(self.bcx.current_block().unwrap());
        let storage = self.inline_storage(ty.inline_bytes());
        for index in 0..shape.lanes() {
            let lane = self.bcx.ins().extractlane(vector, index as u8);
            let slot = self.bcx.ins().iadd_imm_s(storage, (index * 16) as i64);
            self.store_slot(slot, lane, &shape.lane());
        }
        let value = self.bcx.ins().uextend(types::I128, storage);
        self.portable_simd.values.insert(value, vector);
        self.record_simd_snapshot(before);
        value
    }

    pub(super) fn read_simd_local(&mut self, index: u8) -> Option<NativeValue> {
        let (variable, shape) = *self.portable_simd.locals.get(&index)?;
        let vector = self.bcx.use_var(variable);
        let ty = self.locals[index as usize].1.clone();
        Some(self.materialize_simd(vector, shape, &ty))
    }

    pub(super) fn write_simd_local(&mut self, index: u8, value: NativeValue) -> bool {
        let Some(&(variable, shape)) = self.portable_simd.locals.get(&index) else {
            return false;
        };
        let vector = if let Some(&vector) = self.portable_simd.values.get(&value) {
            vector
        } else {
            let storage = self.raw_word(value);
            self.gather_simd_lanes(storage, shape, false)
        };
        self.bcx.def_var(variable, vector);
        true
    }

    pub(super) fn borrow_simd_local(&mut self, index: u8) -> Option<NativeValue> {
        let value = self.read_simd_local(index)?;
        let vector = self.portable_simd.values[&value];
        let before = self
            .bcx
            .func
            .layout
            .last_inst(self.bcx.current_block().unwrap());
        let slot = self.inline_storage(16);
        self.bcx
            .ins()
            .store(MemFlagsData::trusted(), value, slot, 0);
        let reference = self.bcx.ins().uextend(types::I128, slot);
        self.portable_simd.values.insert(reference, vector);
        self.record_simd_snapshot(before);
        Some(reference)
    }

    fn simd_lanes(&mut self, reference: NativeValue, shape: Shape, mask: bool) -> NativeValue {
        if !mask && let Some(&vector) = self.portable_simd.values.get(&reference) {
            return vector;
        }
        // References may project through enclosing Option/Result payloads. The
        // payload word still addresses the same inline multi-field storage.
        let storage = self.reference_word(reference);
        self.gather_simd_lanes(storage, shape, mask)
    }

    fn gather_simd_lanes(&mut self, storage: NativeValue, shape: Shape, mask: bool) -> NativeValue {
        let scalar = if mask { Ty::Bool } else { shape.lane() };
        let vector_ty = if mask {
            if shape.lanes() == 2 {
                types::I64X2
            } else {
                types::I32X4
            }
        } else {
            shape.vector()
        };
        let mut vector = None;
        for index in 0..shape.lanes() {
            let lane = self.bcx.ins().load(
                clif_type(scalar.clone()),
                MemFlagsData::trusted(),
                storage,
                (index * 16) as i32,
            );
            let lane = if mask {
                let lane = self.bcx.ins().uextend(vector_ty.lane_type(), lane);
                self.bcx.ins().ineg(lane)
            } else {
                lane
            };
            vector = Some(match vector {
                None => self.bcx.ins().splat(vector_ty, lane),
                Some(vector) => self.bcx.ins().insertlane(vector, lane, index as u8),
            });
        }
        vector.unwrap()
    }

    pub(super) fn lower_portable_simd(&mut self, op: &Operation, args: &[NativeValue]) {
        let left = self.simd_lanes(args[0], op.shape, false);
        let right = self.simd_lanes(
            args[if matches!(op.action, Action::Select) {
                2
            } else {
                1
            }],
            op.shape,
            false,
        );
        let float = op.shape.lane().is_float();
        let result = match op.action {
            Action::Add if float => self.bcx.ins().fadd(left, right),
            Action::Sub if float => self.bcx.ins().fsub(left, right),
            Action::Mul if float => self.bcx.ins().fmul(left, right),
            Action::Div => self.bcx.ins().fdiv(left, right),
            Action::Add => self.bcx.ins().iadd(left, right),
            Action::Sub => self.bcx.ins().isub(left, right),
            Action::Mul => self.bcx.ins().imul(left, right),
            Action::Compare(_, cc) if float => self.bcx.ins().fcmp(cc, left, right),
            Action::Compare(cc, _) => self.bcx.ins().icmp(cc, left, right),
            Action::Select => {
                let mask = self.simd_lanes(args[1], op.shape, true);
                let mask = self
                    .bcx
                    .ins()
                    .bitcast(op.shape.vector(), MemFlagsData::new(), mask);
                self.bcx.ins().bitselect(mask, left, right)
            }
        };
        let comparison = matches!(op.action, Action::Compare(..));
        if !comparison {
            let value = self.materialize_simd(result, op.shape, &op.output);
            self.stack.push((value, op.output.clone()));
            return;
        }
        let scalar = if comparison {
            Ty::Bool
        } else {
            op.shape.lane()
        };
        let storage = self.inline_storage(op.output.inline_bytes());
        for index in 0..op.shape.lanes() {
            let lane = self.bcx.ins().extractlane(result, index as u8);
            let lane = if comparison {
                self.bcx.ins().icmp_imm_s(IntCC::NotEqual, lane, 0)
            } else {
                lane
            };
            let slot = self.bcx.ins().iadd_imm_s(storage, (index * 16) as i64);
            self.store_slot(slot, lane, &scalar);
        }
        // The sole variant has tag zero, and every lane slot has just been
        // initialized using the ordinary scalar representation.
        let value = self.bcx.ins().uextend(types::I128, storage);
        self.stack.push((value, op.output.clone()));
    }
}
