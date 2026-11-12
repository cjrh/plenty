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
    Sqrt,
    /// `f32x4.reciprocal_sqrt_estimate()`: the target's estimate instruction.
    ReciprocalSqrtEstimate,
    /// `f64x2.to_f32x4()` and `f32x4.to_f64x2()`: the two low lanes
    /// converted, into a vector of this shape.
    Convert(Shape),
    ToTuple,
    Compare(IntCC, FloatCC),
    Select,
    WrappingSum,
    /// `load_*(values, offset)`: a guarded packed read of whole list rows.
    Load,
    /// `vector.store(values, offset)`: the matching guarded packed write.
    Store,
    /// `get_*(values, index)`: a packed read of vector `index` of a view,
    /// which traps in the scalar body when the index is out of range.
    Get,
    /// `vector.set(values, index)`: the matching packed write.
    Set,
    /// `len_*(values)`: the view's whole vectors.
    Len,
    /// `tail_*(values)`: the index of the first element after them.
    Tail,
    /// `splat_*(value)`: one scalar in every lane.
    Splat,
}

/// A portable SIMD operation as guarded numerical state sees it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Numerical {
    /// Lane arithmetic and `splat`: a value of its operands alone.
    Pure,
    Get,
    Set,
    Len,
    Tail,
}

pub(super) struct Operation {
    shape: Shape,
    action: Action,
    output: Ty,
}

impl Operation {
    /// A view access keeps its scalar body: the body reports an invalid
    /// index, so a call site lowers the access inline and calls the body
    /// only from the cold block of a failed check.
    pub(super) fn indexed(&self) -> bool {
        matches!(self.action, Action::Get | Action::Set)
    }

    /// What guarded numerical state may do with a floating operation: see
    /// [`super::fixed_state`]. Other operations abandon its candidate.
    pub(super) fn numerical(&self) -> Option<(Numerical, i64)> {
        if !self.shape.lane().is_float() {
            return None;
        }
        let kind = match self.action {
            Action::Add
            | Action::Sub
            | Action::Mul
            | Action::Div
            | Action::Sqrt
            | Action::ReciprocalSqrtEstimate
            | Action::Convert(_)
            | Action::Splat => Numerical::Pure,
            Action::Get => Numerical::Get,
            Action::Set => Numerical::Set,
            Action::Len => Numerical::Len,
            Action::Tail => Numerical::Tail,
            _ => return None,
        };
        Some((kind, self.shape.lanes() as i64))
    }
}

/// Exact numeric vector locals without mutable borrows and split numeric tuple
/// locals without any borrows are promoted. A
/// materialized shared borrow is a snapshot: the ownership checker prevents
/// mutation of the source for its entire lifetime. Mutable borrows and moved
/// slots retain the ordinary canonical storage path throughout the function.
///
/// An unborrowed `Option[Vector]` local is promoted to its tag word and
/// payload vector, so a list load's result reaches `match` and `unwrap`
/// without passing through tagged storage.
#[derive(Default)]
pub(super) struct State {
    locals: HashMap<u8, (Variable, Shape)>,
    /// Promoted `Option[Vector]` locals: the tag word and the payload, which
    /// is zero while the tag is `Nothing`.
    options: HashMap<u8, (Variable, Variable, Shape)>,
    values: HashMap<NativeValue, NativeValue>,
    /// A native `Option[Vector]` value's canonical payload, itself in `values`.
    payloads: HashMap<NativeValue, NativeValue>,
    /// Option values built by reading a promoted local: no other value
    /// addresses their storage.
    fresh: crate::hash::HashSet<NativeValue>,
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

/// The vector type and shape inside `Option[Vector]`.
fn option_shape(ty: &Ty) -> Option<(Ty, Shape)> {
    let Ty::Enum(t) = ty else { return None };
    if !t.is_option() {
        return None;
    }
    let definition = t.get();
    let [payload] = definition.variants.get(1)?.fields.as_slice() else {
        return None;
    };
    Some((payload.clone(), shape_of(payload)?))
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
        // Destructuring introduces a temporary StoreLocal/MoveLocal/Split.
        // Preserve a native tuple through that temporary as well as the stack.
        let mut tuples = Vec::new();
        crate::threading::walk(body, &mut |op| {
            if let Op::Split(t, 0) = op {
                let ty = Ty::Enum(t.clone());
                for shape in [Shape::I32, Shape::U32, Shape::F32, Shape::F64] {
                    if ty == crate::sum::tuple(vec![shape.lane(); shape.lanes()])
                        && !tuples.iter().any(|(tuple, _)| tuple == &ty)
                    {
                        tuples.push((ty.clone(), shape));
                    }
                }
            }
        });
        if !locals.iter().skip(inputs).any(|(_, ty)| {
            shape_of(ty).is_some()
                || option_shape(ty).is_some()
                || tuples.iter().any(|(tuple, _)| tuple == ty)
        }) {
            return Self::default();
        }
        let mut excluded = vec![false; locals.len()];
        fn visit(body: &[Op], excluded: &mut [bool], locals: &[(Variable, Ty)]) {
            for op in body {
                match op {
                    Op::BorrowLocal(index, true) => excluded[*index as usize] = true,
                    Op::MoveLocal(index) if shape_of(&locals[*index as usize].1).is_some() => {
                        excluded[*index as usize] = true;
                    }
                    // Addressable tuples retain ordinary mutable/copy semantics.
                    Op::BorrowLocal(index, _) if shape_of(&locals[*index as usize].1).is_none() => {
                        excluded[*index as usize] = true;
                    }
                    Op::If {
                        then,
                        elifs,
                        otherwise,
                    } => {
                        visit(then, excluded, locals);
                        for arm in elifs.iter() {
                            visit(&arm.condition, excluded, locals);
                            visit(&arm.body, excluded, locals);
                        }
                        visit(otherwise, excluded, locals);
                    }
                    Op::Loop { condition, body } => {
                        visit(condition, excluded, locals);
                        visit(body, excluded, locals);
                    }
                    Op::Match(arms) => {
                        for arm in arms.iter() {
                            visit(&arm.body, excluded, locals);
                        }
                    }
                    Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => {
                        visit(cleanup, excluded, locals)
                    }
                    Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => {
                        visit(body, excluded, locals)
                    }

                    _ => {}
                }
            }
        }
        visit(body, &mut excluded, locals);
        let options = locals
            .iter()
            .enumerate()
            .skip(inputs)
            .filter_map(|(index, (_, ty))| {
                let (_, shape) = option_shape(ty).filter(|_| !excluded[index])?;
                let tag = builder.declare_var(types::I64);
                let payload = builder.declare_var(shape.vector());
                Some((index as u8, (tag, payload, shape)))
            })
            .collect();
        let locals = locals
            .iter()
            .enumerate()
            .skip(inputs)
            .filter_map(|(index, (_, ty))| {
                if excluded[index] {
                    return None;
                }
                let shape = shape_of(ty).or_else(|| {
                    tuples
                        .iter()
                        .find_map(|(tuple, shape)| (tuple == ty).then_some(*shape))
                })?;
                Some((index as u8, (builder.declare_var(shape.vector()), shape)))
            })
            .collect();
        Self {
            locals,
            options,
            ..Self::default()
        }
    }

    pub(super) fn promoted(&self, index: u8) -> bool {
        self.locals.contains_key(&index) || self.options.contains_key(&index)
    }
    pub(super) fn fresh(&self, value: NativeValue) -> bool {
        self.values.contains_key(&value) || self.fresh.contains(&value)
    }
    /// The payload of a native `Option[Vector]` value: what `unwrap` and a
    /// `Some` projection yield once the tag is known.
    pub(super) fn option_payload(&self, value: NativeValue) -> Option<NativeValue> {
        self.payloads.get(&value).copied()
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
    classify(name, &decl.sig, decl.standard_library, optimize)
}

/// [`operation`] from a declaration's parts, for passes that run before
/// functions are declared.
pub(super) fn classify(
    name: &str,
    sig: &FnSig,
    standard_library: bool,
    optimize: bool,
) -> Option<Operation> {
    if !cfg!(target_arch = "x86_64") || !optimize || !standard_library {
        return None;
    }
    let (name, method) = if let Some(vector) = name.strip_prefix("std.simd.load_") {
        (vector, "load")
    } else if let Some(vector) = name.strip_prefix("std.simd.get_") {
        (vector, "get")
    } else if let Some(vector) = name.strip_prefix("std.simd.len_") {
        (vector, "len")
    } else if let Some(vector) = name.strip_prefix("std.simd.tail_") {
        (vector, "tail")
    } else if let Some(vector) = name.strip_prefix("std.simd.splat_") {
        (vector, "splat")
    } else {
        name.strip_prefix("__plenty_class_std.simd.")?
            .split_once('.')?
    };
    let shape = match name {
        "i32x4" => Shape::I32,
        "u32x4" => Shape::U32,
        "f32x4" => Shape::F32,
        "f64x2" => Shape::F64,
        _ => return None,
    };
    if matches!(method, "load" | "store") {
        return transfer(name, shape, method == "store", sig);
    }
    if matches!(method, "get" | "set") {
        return view_access(name, shape, method == "set", sig);
    }
    if matches!(method, "len" | "tail" | "splat") {
        let lane = shape.lane();
        let inputs = sig.inputs.iter().map(|(_, ty)| ty).collect::<Vec<_>>();
        let action = match (method, inputs.as_slice(), sig.outputs.as_slice()) {
            ("splat", [input], [output])
                if **input == lane && logical_type(output, name, &lane, shape.lanes()) =>
            {
                Action::Splat
            }
            (_, [Ty::View(element, false)], [Ty::I64]) if **element == lane => {
                if method == "len" {
                    Action::Len
                } else {
                    Action::Tail
                }
            }
            _ => return None,
        };
        return Some(Operation {
            shape,
            action,
            output: sig.outputs[0].clone(),
        });
    }
    let float = shape.lane().is_float();
    let unsigned = matches!(shape, Shape::U32);
    let action = match method {
        "wrapping_add" if !float => Action::Add,
        "wrapping_sub" if !float => Action::Sub,
        "wrapping_mul" if !float => Action::Mul,
        "wrapping_sum" if !float => Action::WrappingSum,
        "add" if float => Action::Add,
        "sub" if float => Action::Sub,
        "mul" if float => Action::Mul,
        "div" if float => Action::Div,
        "sqrt" if float => Action::Sqrt,
        "reciprocal_sqrt_estimate" if matches!(shape, Shape::F32) => Action::ReciprocalSqrtEstimate,
        "to_f32x4" if matches!(shape, Shape::F64) => Action::Convert(Shape::F32),
        "to_f64x2" if matches!(shape, Shape::F32) => Action::Convert(Shape::F64),
        "to_tuple" => Action::ToTuple,
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
    let input = &sig.inputs;
    let [output] = sig.outputs.as_slice() else {
        return None;
    };
    if matches!(action, Action::WrappingSum) {
        return (input.len() == 1
            && borrowed(&input[0].1, name, &shape.lane(), shape.lanes())
            && *output == shape.lane())
        .then(|| Operation {
            shape,
            action,
            output: output.clone(),
        });
    }
    if matches!(action, Action::ToTuple) {
        return (input.len() == 1
            && borrowed(&input[0].1, name, &shape.lane(), shape.lanes())
            && *output == crate::sum::tuple(vec![shape.lane(); shape.lanes()]))
        .then(|| Operation {
            shape,
            action,
            output: output.clone(),
        });
    }
    if let Action::Convert(to) = action {
        return (input.len() == 1
            && borrowed(&input[0].1, name, &shape.lane(), shape.lanes())
            && shape_of(output).is_some_and(|result| result.vector() == to.vector()))
        .then(|| Operation {
            shape,
            action,
            output: output.clone(),
        });
    }
    if matches!(action, Action::Sqrt | Action::ReciprocalSqrtEstimate) {
        return (input.len() == 1
            && borrowed(&input[0].1, name, &shape.lane(), shape.lanes())
            && logical_type(output, name, &shape.lane(), shape.lanes()))
        .then(|| Operation {
            shape,
            action,
            output: output.clone(),
        });
    }
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

/// A list transfer's declared shape: packed rows of the lane type, an `i64`
/// offset, and `Option[Vector]` from a load or `bool` from a store.
fn transfer(name: &str, shape: Shape, store: bool, sig: &FnSig) -> Option<Operation> {
    let lane = shape.lane();
    let vector = |ty: &Ty| logical_type(ty, name, &lane, shape.lanes());
    let rows = |ty: &Ty| {
        matches!(ty, Ty::Ref(list, mutable) if *mutable == store
            && matches!(**list, Ty::List(_))
            && list.element().as_ref() == Some(&lane)
            && metadata::list_row_bytes(&lane) == clif_type(lane.clone()).bytes() as usize)
    };
    let inputs = sig.inputs.iter().map(|(_, ty)| ty).collect::<Vec<_>>();
    let [output] = sig.outputs.as_slice() else {
        return None;
    };
    let declared = match (store, inputs.as_slice()) {
        (true, [receiver, list, Ty::I64]) => {
            borrowed(receiver, name, &lane, shape.lanes()) && rows(list) && *output == Ty::Bool
        }
        (false, [list, Ty::I64]) => {
            rows(list) && option_shape(output).is_some_and(|(payload, _)| vector(&payload))
        }
        _ => false,
    };
    declared.then(|| Operation {
        shape,
        action: if store { Action::Store } else { Action::Load },
        output: output.clone(),
    })
}

/// A view access's declared shape: a view of the lane type whose rows are
/// packed at the lane width, an `i64` vector index, and the vector from a
/// read or nothing from a write.
fn view_access(name: &str, shape: Shape, set: bool, sig: &FnSig) -> Option<Operation> {
    let lane = shape.lane();
    let rows = |ty: &Ty| {
        matches!(ty, Ty::View(element, mutable) if *mutable == set
            && **element == lane
            && metadata::list_row_bytes(&lane) == clif_type(lane.clone()).bytes() as usize)
    };
    let inputs = sig.inputs.iter().map(|(_, ty)| ty).collect::<Vec<_>>();
    let declared = match (set, inputs.as_slice(), sig.outputs.as_slice()) {
        (true, [receiver, view, Ty::I64], []) => {
            borrowed(receiver, name, &lane, shape.lanes()) && rows(view)
        }
        (false, [view, Ty::I64], [output]) => {
            rows(view) && logical_type(output, name, &lane, shape.lanes())
        }
        _ => false,
    };
    declared.then(|| Operation {
        shape,
        action: if set { Action::Set } else { Action::Get },
        output: sig.outputs.first().cloned().unwrap_or(Ty::Unit),
    })
}

impl Lowerer<'_, '_> {
    /// Tests a view access's vector index against the view's whole vectors,
    /// `count >> log2(lanes)`: one unsigned comparison, which also rejects a
    /// negative index. Returns the block that continues with the access,
    /// leaving the builder in the cold block for an index out of range with
    /// `args` ready for the scalar body, which reports it. Returns `None`,
    /// leaving the builder in place, when the enclosing versioned loop's
    /// test covers the index.
    pub(super) fn guard_simd_view(
        &mut self,
        op: &Operation,
        args: &mut [NativeValue],
    ) -> Option<cranelift_codegen::ir::Block> {
        let set = matches!(op.action, Action::Set);
        let [view, index] = args[usize::from(set)..] else {
            unreachable!("view access signature checked by `operation`")
        };
        let shift = op.shape.lanes().ilog2();
        if self.versioned_vector(index, view, shift) {
            return None;
        }
        let source = set.then(|| self.simd_lanes(args[0], op.shape, false));
        let (_, count) = self.split_view(view);
        let vectors = self.bcx.ins().ushr_imm_u(count, i64::from(shift));
        let inside = self.bcx.ins().icmp(IntCC::UnsignedLessThan, index, vectors);
        let [hit, miss] = std::array::from_fn(|_| self.bcx.create_block());
        self.bcx.set_cold_block(miss);
        self.bcx.ins().brif(inside, hit, &[], miss, &[]);
        self.bcx.seal_block(hit);
        self.bcx.switch_to_block(miss);
        self.bcx.seal_block(miss);
        if let Some(vector) = source {
            // The scalar body takes the vector by reference to its lanes'
            // storage. Building that here, not from the caller's lazy
            // snapshot, keeps the vector in a register on the hot path.
            let storage = self.inline_storage(op.shape.lanes() * 16);
            for lane in 0..op.shape.lanes() {
                let value = self.bcx.ins().extractlane(vector, lane as u8);
                let slot = self.bcx.ins().iadd_imm_s(storage, (lane * 16) as i64);
                self.store_slot(slot, value, &op.shape.lane());
            }
            let value = self.bcx.ins().uextend(types::I128, storage);
            let slot = self.inline_storage(16);
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), value, slot, 0);
            args[0] = self.bcx.ins().uextend(types::I128, slot);
        }
        Some(hit)
    }

    /// Whether the versioned loop copy being lowered may skip the check of
    /// the vector index `index` against the whole vectors of `view`,
    /// recording the test that copy then needs. Only the range counter plus
    /// a small constant qualifies; see [`super::induction::Version`].
    fn versioned_vector(&mut self, index: NativeValue, view: NativeValue, shift: u32) -> bool {
        use super::induction::{Assumed, LengthOf, VERSION_OFFSET, offset};
        let dfg = &self.bcx.func.dfg;
        let bounds = &self.counters.bounds;
        let Some(version) = &bounds.version else {
            return false;
        };
        let view = bounds.canonical(dfg, view);
        if version.entries.get(&view) != Some(&true) {
            return false;
        }
        let (base, k) = offset(dfg, index);
        let below_stop = bounds
            .below
            .get(&base)
            .is_some_and(|&bound| Some(bounds.canonical(dfg, bound)) == version.stop);
        if !(0..=VERSION_OFFSET).contains(&k) || !below_stop || !self.nonnegative_operand(base) {
            return false;
        }
        let assumed = Assumed::Counter(LengthOf::Vectors(view, shift));
        let version = self.counters.bounds.version.as_mut().unwrap();
        match version.assumed.iter_mut().find(|(a, _)| *a == assumed) {
            Some((_, most)) => *most = (*most).max(k),
            None => version.assumed.push((assumed, k)),
        }
        true
    }

    /// The packed read or write of vector `index`, whose index is in range.
    pub(super) fn lower_simd_view(&mut self, op: &Operation, args: &[NativeValue]) {
        let set = matches!(op.action, Action::Set);
        let source = set.then(|| self.simd_lanes(args[0], op.shape, false));
        let [view, index] = args[usize::from(set)..] else {
            unreachable!("view access signature checked by `operation`")
        };
        let (rows, _) = self.split_view(view);
        // x86-64 scales an index by at most 8, so a 16-byte vector's address
        // is `rows + (index + index) * 8`: one `lea` and the access's own
        // addressing mode, not a copy and a shift.
        debug_assert_eq!(op.shape.vector().bytes(), 16);
        let pairs = self.bcx.ins().iadd(index, index);
        let address = self.row_address(rows, pairs, 8);
        let flags = MemFlagsData::trusted();
        match source {
            // A write returns `()`, which has no value.
            Some(vector) => {
                self.bcx.ins().store(flags, vector, address, 0);
            }
            None => {
                let vector = self.bcx.ins().load(op.shape.vector(), flags, address, 0);
                let value = self.materialize_simd(vector, op.shape, &op.output);
                self.stack.push((value, op.output.clone()));
            }
        }
    }

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

    /// Constructors use the same private native value and lazy canonical snapshot
    /// as arithmetic results. Escapes retain the snapshot through ordinary uses.
    pub(super) fn construct_simd(&mut self, ty: &Ty) -> Result<bool> {
        if !self.module.optimizes() || !cfg!(target_arch = "x86_64") {
            return Ok(false);
        }
        let Some(shape) = shape_of(ty) else {
            return Ok(false);
        };
        let mut lanes = Vec::with_capacity(shape.lanes());
        for _ in 0..shape.lanes() {
            lanes.push(self.pop_typed(shape.lane())?.0);
        }
        lanes.reverse();
        let mut vector = self.bcx.ins().splat(shape.vector(), lanes[0]);
        for (index, &lane) in lanes.iter().enumerate().skip(1) {
            vector = self.bcx.ins().insertlane(vector, lane, index as u8);
        }
        let value = self.materialize_simd(vector, shape, ty);
        self.stack.push((value, ty.clone()));
        Ok(true)
    }

    /// Only values produced by native lowering are eligible. Ordinary tuples,
    /// addressable copies and values crossing an ABI boundary keep their layout.
    pub(super) fn split_simd(&mut self, value: NativeValue, fields: &[Ty]) -> bool {
        let Some(&vector) = self.portable_simd.values.get(&value) else {
            return false;
        };
        let vector_ty = self.bcx.func.dfg.value_type(vector);
        if fields.len() != vector_ty.lane_count() as usize
            || fields
                .iter()
                .any(|ty| clif_type(ty.clone()) != vector_ty.lane_type())
        {
            return false;
        }
        for (index, ty) in fields.iter().enumerate() {
            let lane = self.bcx.ins().extractlane(vector, index as u8);
            self.stack.push((lane, ty.clone()));
        }
        true
    }

    /// Empty ownership slots have no observable value. Do not interpret their
    /// null canonical pointer as lane storage when clearing a promoted tuple.
    pub(super) fn clear_simd_local(&mut self, index: u8) -> bool {
        if let Some(&(tag, payload, shape)) = self.portable_simd.options.get(&index) {
            let nothing = self.bcx.ins().iconst(types::I64, 0);
            let zero = self.zero_simd(shape);
            self.bcx.def_var(tag, nothing);
            self.bcx.def_var(payload, zero);
            return true;
        }
        let Some(&(variable, shape)) = self.portable_simd.locals.get(&index) else {
            return false;
        };
        let zero = self.zero_simd(shape);
        self.bcx.def_var(variable, zero);
        true
    }

    fn zero_simd(&mut self, shape: Shape) -> NativeValue {
        let zero = match shape {
            Shape::F32 => self.bcx.ins().f32const(0.0),
            Shape::F64 => self.bcx.ins().f64const(0.0),
            Shape::I32 | Shape::U32 => self.bcx.ins().iconst(types::I32, 0),
        };
        self.bcx.ins().splat(shape.vector(), zero)
    }

    /// The canonical `Option[Vector]` for tag word `tag` (1 is `Some`) and
    /// payload `vector`, as a lazy snapshot. `Nothing` has a zero payload
    /// word, as its constructor gives it.
    fn option_simd(
        &mut self,
        tag: NativeValue,
        vector: NativeValue,
        shape: Shape,
        option: &Ty,
    ) -> NativeValue {
        let (payload_ty, _) = option_shape(option).expect("Option[Vector]");
        let payload = self.materialize_simd(vector, shape, &payload_ty);
        let before = self
            .bcx
            .func
            .layout
            .last_inst(self.bcx.current_block().unwrap());
        let storage = self.raw_word(payload);
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let word = self.bcx.ins().select(tag, storage, zero);
        let value = self.bcx.ins().iconcat(word, tag);
        self.record_simd_snapshot(before);
        self.portable_simd.payloads.insert(value, payload);
        value
    }

    /// The tag word and payload vector of a canonical `Option[Vector]`.
    fn option_simd_parts(
        &mut self,
        value: NativeValue,
        shape: Shape,
    ) -> (NativeValue, NativeValue) {
        let tag = self.sum_tag(value);
        let storage = self.raw_word(value);
        let some = self.bcx.create_block();
        let nothing = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, shape.vector());
        self.bcx.ins().brif(tag, some, &[], nothing, &[]);
        self.bcx.switch_to_block(some);
        self.bcx.seal_block(some);
        let vector = self.gather_simd_lanes(storage, shape, false);
        self.bcx.ins().jump(done, &[vector.into()]);
        self.bcx.switch_to_block(nothing);
        self.bcx.seal_block(nothing);
        let zero = self.zero_simd(shape);
        self.bcx.ins().jump(done, &[zero.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        (tag, self.bcx.block_params(done)[0])
    }

    pub(super) fn read_simd_local(&mut self, index: u8) -> Option<NativeValue> {
        if let Some(&(tag, payload, shape)) = self.portable_simd.options.get(&index) {
            let tag = self.bcx.use_var(tag);
            let vector = self.bcx.use_var(payload);
            let ty = self.locals[index as usize].1.clone();
            let value = self.option_simd(tag, vector, shape, &ty);
            self.portable_simd.fresh.insert(value);
            return Some(value);
        }
        let (variable, shape) = *self.portable_simd.locals.get(&index)?;
        let vector = self.bcx.use_var(variable);
        let ty = self.locals[index as usize].1.clone();
        Some(self.materialize_simd(vector, shape, &ty))
    }

    pub(super) fn write_simd_local(&mut self, index: u8, value: NativeValue) -> bool {
        if let Some(&(tag, payload, shape)) = self.portable_simd.options.get(&index) {
            let native = self
                .portable_simd
                .payloads
                .get(&value)
                .map(|payload| self.portable_simd.values[payload]);
            let (word, vector) = match native {
                // Built by `option_simd` as `iconcat(word, tag)`.
                Some(vector) => (self.high_word(value), vector),
                None => self.option_simd_parts(value, shape),
            };
            self.bcx.def_var(tag, word);
            self.bcx.def_var(payload, vector);
            return true;
        }
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

    /// Reads or writes all lanes as one packed vector: a scalar list stores
    /// its rows at the lane width, in lane order. The span must satisfy
    /// `0 <= offset <= length - lanes`, the scalar bodies' guard; comparing
    /// against the limit, which is negative for a short list, never adds to
    /// `offset`. A span that does not fit leaves the list untouched and yields
    /// `Nothing` or `False`; neither path traps.
    fn lower_simd_transfer(&mut self, op: &Operation, args: &[NativeValue]) {
        let store = matches!(op.action, Action::Store);
        let source = store.then(|| self.simd_lanes(args[0], op.shape, false));
        let [list, offset] = args[usize::from(store)..] else {
            unreachable!("transfer signature checked by `operation`")
        };
        let owner = self.reference_word(list);
        let flags = MemFlagsData::trusted();
        let rows = self
            .bcx
            .ins()
            .load(PTR_TY, flags, owner, crate::list_layout::ROWS);
        let row_bytes = i64::from(op.shape.vector().lane_type().bytes());
        let length = self.dense_list_len(owner, row_bytes);
        let limit = self
            .bcx
            .ins()
            .iadd_imm_s(length, -(op.shape.lanes() as i64));
        let [bounded, hit, miss, done] = std::array::from_fn(|_| self.bcx.create_block());
        self.bcx.set_cold_block(miss);
        // The result's tag word or truth value; a load also merges its vector.
        let outcome = if store { types::I8 } else { types::I64 };
        self.bcx.append_block_param(done, outcome);
        if !store {
            self.bcx.append_block_param(done, op.shape.vector());
        }
        let negative = self.bcx.ins().icmp_imm_s(IntCC::SignedLessThan, offset, 0);
        self.bcx.ins().brif(negative, miss, &[], bounded, &[]);
        self.bcx.switch_to_block(bounded);
        self.bcx.seal_block(bounded);
        let late = self.bcx.ins().icmp(IntCC::SignedGreaterThan, offset, limit);
        self.bcx.ins().brif(late, miss, &[], hit, &[]);
        for (block, fits) in [(hit, true), (miss, false)] {
            self.bcx.switch_to_block(block);
            self.bcx.seal_block(block);
            let flag = self.bcx.ins().iconst(outcome, i64::from(fits));
            let vector = match source {
                Some(vector) if fits => {
                    let address = self.row_address(rows, offset, row_bytes);
                    self.bcx.ins().store(flags, vector, address, 0);
                    None
                }
                Some(_) => None,
                None if fits => {
                    let address = self.row_address(rows, offset, row_bytes);
                    Some(self.bcx.ins().load(op.shape.vector(), flags, address, 0))
                }
                None => Some(self.zero_simd(op.shape)),
            };
            let mut results = vec![BlockArg::Value(flag)];
            results.extend(vector.map(BlockArg::Value));
            self.bcx.ins().jump(done, &results);
        }
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        let params = self.bcx.block_params(done).to_vec();
        let value = match params[..] {
            [tag, vector] => self.option_simd(tag, vector, op.shape, &op.output),
            _ => params[0],
        };
        self.stack.push((value, op.output.clone()));
    }

    pub(super) fn lower_portable_simd(&mut self, op: &Operation, args: &[NativeValue]) {
        if matches!(op.action, Action::Load | Action::Store) {
            return self.lower_simd_transfer(op, args);
        }
        if op.indexed() {
            return self.lower_simd_view(op, args);
        }
        if matches!(op.action, Action::Len | Action::Tail) {
            let (_, count) = self.split_view(args[0]);
            let shift = i64::from(op.shape.lanes().ilog2());
            let mut result = self.bcx.ins().ushr_imm_u(count, shift);
            if matches!(op.action, Action::Tail) {
                result = self.bcx.ins().ishl_imm_u(result, shift);
            }
            // At most the view's count, which is not negative.
            self.counters.nonnegative.insert(result);
            self.stack.push((result, Ty::I64));
            return;
        }
        if matches!(op.action, Action::Splat) {
            let vector = self.bcx.ins().splat(op.shape.vector(), args[0]);
            let value = self.materialize_simd(vector, op.shape, &op.output);
            self.stack.push((value, op.output.clone()));
            return;
        }
        let left = self.simd_lanes(args[0], op.shape, false);
        if matches!(op.action, Action::WrappingSum) {
            // Keep the existing modular lane-order fold, without materializing
            // an enum and calling its scalar body. In particular, a subsequent
            // vector reduction need not survive this reduction's ABI call.
            let mut sum = self.bcx.ins().extractlane(left, 0);
            for index in 1..op.shape.lanes() {
                let lane = self.bcx.ins().extractlane(left, index as u8);
                sum = self.bcx.ins().iadd(sum, lane);
            }
            self.stack.push((sum, op.output.clone()));
            return;
        }
        if matches!(op.action, Action::ToTuple) {
            let value = self.materialize_simd(left, op.shape, &op.output);
            self.stack.push((value, op.output.clone()));
            return;
        }
        if matches!(op.action, Action::Sqrt | Action::ReciprocalSqrtEstimate) {
            let result = match op.action {
                Action::Sqrt => self.bcx.ins().sqrt(left),
                // `classify` admits this on x86-64 only.
                _ => self.bcx.ins().x86_rsqrtps(left),
            };
            let value = self.materialize_simd(result, op.shape, &op.output);
            self.stack.push((value, op.output.clone()));
            return;
        }
        if let Action::Convert(to) = op.action {
            // `cvtpd2ps` rounds as the scalar cast does and zeroes lanes 2
            // and 3; `cvtps2pd` widens lanes 0 and 1 exactly.
            let result = match to {
                Shape::F32 => self.bcx.ins().fvdemote(left),
                _ => self.bcx.ins().fvpromote_low(left),
            };
            let value = self.materialize_simd(result, to, &op.output);
            self.stack.push((value, op.output.clone()));
            return;
        }
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
            Action::WrappingSum
            | Action::Sqrt
            | Action::ReciprocalSqrtEstimate
            | Action::Convert(_)
            | Action::ToTuple
            | Action::Load
            | Action::Store
            | Action::Get
            | Action::Set
            | Action::Len
            | Action::Tail
            | Action::Splat => {
                unreachable!("lowered above")
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
