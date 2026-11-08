//! A bounded native optimization of checked column filter/count loops. The
//! original scalar loop remains the tail and the fallback for every other shape.
use super::*;
use crate::clist::ClistOp;
use crate::collection::CollectionOp;
use crate::op::{IntegerOp, Pattern};
use crate::value::Value as Literal;
use cranelift_codegen::ir::MemFlagsData;
use cranelift_codegen::ir::condcodes::CondCode;

fn operations(ops: &[Op]) -> Vec<&Op> {
    ops.iter()
        .filter(|op| {
            !matches!(
                op,
                Op::Site(_)
                    | Op::CallerSite(_)
                    | Op::Access(..)
                    | Op::Loan(_)
                    | Op::UseLoan(_)
                    | Op::EndLoan(_)
            )
        })
        .collect()
}

/// The vector shape of a column type: its lane type, lane count, and how many
/// iterations a lane counter of that width can count before it must be
/// widened into the 64-bit total.
struct Shape {
    vector: cranelift_codegen::ir::Type,
    lanes: i64,
    iterations: i64,
    signed: bool,
}

fn shape(ty: &Ty) -> Option<Shape> {
    let (vector, lanes, iterations) = match ty {
        Ty::I8 | Ty::U8 => (types::I8X16, 16, i64::from(u8::MAX)),
        Ty::I16 | Ty::U16 => (types::I16X8, 8, i64::from(u16::MAX)),
        Ty::I32 | Ty::U32 => (types::I32X4, 4, i64::from(u32::MAX)),
        _ => return None,
    };
    Some(Shape {
        vector,
        lanes,
        iterations,
        signed: matches!(ty, Ty::I8 | Ty::I16 | Ty::I32),
    })
}

/// A literal of the column type, as its value.
fn literal(value: &Literal, ty: &Ty) -> Option<i64> {
    Some(match (value, ty) {
        (Literal::I8(n), Ty::I8) => i64::from(*n),
        (Literal::U8(n), Ty::U8) => i64::from(*n),
        (Literal::I16(n), Ty::I16) => i64::from(*n),
        (Literal::U16(n), Ty::U16) => i64::from(*n),
        (Literal::I32(n), Ty::I32) => i64::from(*n),
        (Literal::U32(n), Ty::U32) => i64::from(*n),
        _ => return None,
    })
}

/// The signed condition that gives `condition`'s result on operands whose
/// sign bits are flipped. SSE2 compares lanes only as signed integers.
fn biased(condition: IntCC) -> IntCC {
    match condition {
        IntCC::UnsignedLessThan => IntCC::SignedLessThan,
        IntCC::UnsignedLessThanOrEqual => IntCC::SignedLessThanOrEqual,
        IntCC::UnsignedGreaterThan => IntCC::SignedGreaterThan,
        IntCC::UnsignedGreaterThanOrEqual => IntCC::SignedGreaterThanOrEqual,
        other => other,
    }
}

fn step(ops: &[&Op], index: u8) -> bool {
    matches!(ops, [Op::LoadLocal(a), Op::PushInt(Literal::I64(1)),
        Op::Integer(IntegerOp::WrappingAdd), Op::StoreLocal(b)] if *a == index && *b == index)
}

#[derive(Clone)]
enum Threshold {
    Constant(i64),
    /// A hidden-function parameter: `&T` pointing into the closure environment.
    Capture(u8),
    /// An immutable scalar local of a predicate spliced into the loop.
    Local(u8),
}

#[derive(Clone)]
struct Comparison {
    condition: IntCC,
    threshold: Threshold,
}

#[derive(Clone)]
pub(super) struct Predicate {
    ty: Ty,
    captures: usize,
    comparisons: Vec<Comparison>,
}

type Operand<'a> = &'a dyn Fn(&[&Op]) -> Option<Threshold>;

fn comparison(ops: &[&Op], ty: &Ty, item: u8, operand: Operand) -> Option<Comparison> {
    let (compare, operands) = ops.split_last()?;
    let signed = shape(ty)?.signed;
    let condition = match compare {
        Op::Eq => IntCC::Equal,
        Op::Ne => IntCC::NotEqual,
        Op::Lt if signed => IntCC::SignedLessThan,
        Op::Le if signed => IntCC::SignedLessThanOrEqual,
        Op::Gt if signed => IntCC::SignedGreaterThan,
        Op::Ge if signed => IntCC::SignedGreaterThanOrEqual,
        Op::Lt => IntCC::UnsignedLessThan,
        Op::Le => IntCC::UnsignedLessThanOrEqual,
        Op::Gt => IntCC::UnsignedGreaterThan,
        Op::Ge => IntCC::UnsignedGreaterThanOrEqual,
        _ => return None,
    };
    let reads_item = |ops: &[&Op]| matches!(ops, [Op::LoadLocal(slot), Op::ReadRef(t)] if *slot == item && t == ty);
    let threshold = |ops: &[&Op]| match ops {
        [Op::PushInt(value)] => literal(value, ty).map(Threshold::Constant),
        _ => operand(ops),
    };
    if operands.len() >= 2 && reads_item(&operands[..2]) {
        Some(Comparison {
            condition,
            threshold: threshold(&operands[2..])?,
        })
    } else if operands.len() >= 2 && reads_item(&operands[operands.len() - 2..]) {
        Some(Comparison {
            condition: condition.swap_args(),
            threshold: threshold(&operands[..operands.len() - 2])?,
        })
    } else {
        None
    }
}

/// The comparisons of a predicate body that reads its item through `item`.
fn comparisons(ops: &[&Op], ty: &Ty, item: u8, operand: Operand) -> Option<Vec<Comparison>> {
    if let Some(Op::Match(arms)) = ops.last().copied() {
        // Source `and` short-circuits through this exact branch. Both accepted
        // comparisons only read immutable scalar values, so evaluating their
        // masks eagerly cannot add calls, traps, effects or observable reads.
        let [yes, no] = arms.as_ref() else {
            return None;
        };
        if yes.body.len() > 24
            || no.body.len() > 8
            || yes.pattern != Pattern::Bool(true)
            || no.pattern != Pattern::Bool(false)
            || !matches!(operations(&no.body).as_slice(), [Op::PushBool(false)])
        {
            return None;
        }
        Some(vec![
            comparison(&ops[..ops.len() - 1], ty, item, operand)?,
            comparison(&operations(&yes.body), ty, item, operand)?,
        ])
    } else {
        Some(vec![comparison(ops, ty, item, operand)?])
    }
}

fn predicate(body: &[Op], function: &UserFn) -> Option<Predicate> {
    if body.len() > 48 || function.generator.is_some() || !function.locals.is_empty() {
        return None;
    }
    let inputs = &function.sig.inputs;
    let captures = inputs.len().checked_sub(1)?;
    let Ty::Ref(ty, false) = &inputs.last()?.1 else {
        return None;
    };
    if captures > 2
        || shape(ty).is_none()
        || inputs
            .iter()
            .any(|(_, input)| input != &inputs.last().unwrap().1)
        || function.sig.outputs.as_slice() != [Ty::Bool]
    {
        return None;
    }
    let item = captures as u8;
    let capture = |ops: &[&Op]| match ops {
        [Op::LoadLocal(slot), Op::ReadRef(t)] if *slot < item && t == ty.as_ref() => {
            Some(Threshold::Capture(*slot))
        }
        _ => None,
    };
    let comparisons = comparisons(&operations(body), ty, item, &capture)?;
    Some(Predicate {
        ty: ty.as_ref().clone(),
        captures,
        comparisons,
    })
}

pub(super) fn prepare(bodies: &[(String, Arc<[Op]>)], functions: &mut HashMap<String, UserFn>) {
    for (name, body) in bodies {
        let function = functions.get_mut(name).unwrap();
        function.column_predicate = predicate(body, function).map(Arc::new);
    }
}

struct Scan {
    address: u8,
    index: u8,
    length: u8,
    item: u8,
    count: u8,
    environment: Option<(u8, Arc<crate::closure::ClosureType>)>,
    predicate: Arc<Predicate>,
}

impl Lowerer<'_, '_> {
    fn clist_count_scan(&self, condition: &[Op], body: &[Op]) -> Option<Scan> {
        // Keep work bounded even for large user-written loop bodies. These
        // limits include the source-position and ownership markers we ignore.
        if condition.len() > 16 || body.len() > 96 {
            return None;
        }
        let condition = operations(condition);
        let [Op::LoadLocal(index), Op::LoadLocal(length), Op::Lt] = condition.as_slice() else {
            return None;
        };
        let body = operations(body);
        let [
            Op::LoadLocal(address),
            Op::LoadLocal(position),
            Op::Collection(CollectionOp::Clist(ClistOp::Load(ty))),
            Op::StoreLocal(item),
            tail @ ..,
        ] = body.as_slice()
        else {
            return None;
        };
        if position != index || shape(ty).is_none() {
            return None;
        }
        let (call, tail) = tail.split_at_checked(tail.len().checked_sub(9)?)?;
        let count = self.count_tail(*index, tail)?;
        let slots = [*address, *index, *length, *item, count];
        for (i, slot) in slots.iter().enumerate() {
            if slots[..i].contains(slot) {
                return None;
            }
        }
        // No alias may change the source bounds, index or count behind our back.
        for slot in [*address, *index, *length, count] {
            if self.locals[slot as usize].1 != Ty::I64 || self.local_address(slot).is_some() {
                return None;
            }
        }
        let scan = |environment, predicate| Scan {
            address: *address,
            index: *index,
            length: *length,
            item: *item,
            count,
            environment,
            predicate,
        };
        // A closure literal spliced into the pipeline binds its `&T` parameter
        // to the item and reads immutable scalar captures as plain locals.
        if let [
            Op::BorrowLocal(arg, false),
            Op::StoreLocal(parameter),
            body @ ..,
        ] = call
        {
            let reference = Ty::Ref(Arc::new(ty.clone()), false);
            if *arg != *item
                || body.len() > 48
                || slots.contains(parameter)
                || self.locals[*parameter as usize].1 != reference
            {
                return None;
            }
            let local = |ops: &[&Op]| match ops {
                [Op::LoadLocal(slot)]
                    if !slots.contains(slot)
                        && slot != parameter
                        && self.locals[*slot as usize].1 == *ty =>
                {
                    Some(Threshold::Local(*slot))
                }
                _ => None,
            };
            let comparisons = comparisons(body, ty, *parameter, &local)?;
            let predicate = Predicate {
                ty: ty.clone(),
                captures: 0,
                comparisons,
            };
            return Some(scan(None, Arc::new(predicate)));
        }
        let (callee, environment, argument) = match call {
            [
                Op::BorrowLocal(env, false),
                Op::BorrowLocal(arg, false),
                Op::ClosureCall(closure),
            ] if !closure.mutable
                && !closure.once
                && closure.captures.len() <= 2
                && closure.captures.iter().all(|(_, t)| t == ty)
                && self.locals[*env as usize].1 == Ty::Closure(closure.clone()) =>
            {
                (closure.name.as_str(), Some((*env, closure.clone())), *arg)
            }
            [Op::BorrowLocal(arg, false), Op::Call(name)] => (name.as_str(), None, *arg),
            _ => return None,
        };
        if argument != *item
            || environment
                .as_ref()
                .is_some_and(|(env, _)| slots.contains(env))
        {
            return None;
        }
        let predicate = self.user_fns.get(callee)?.column_predicate.as_ref()?;
        if predicate.ty != *ty
            || predicate.captures != environment.as_ref().map_or(0, |(_, t)| t.captures.len())
        {
            return None;
        }
        Some(scan(environment, predicate.clone()))
    }

    /// The count of a filter/count loop tail: the filter branch, the count
    /// increment and the index step. Returns the count local.
    fn count_tail(&self, index: u8, tail: &[&Op]) -> Option<u8> {
        let [
            Op::Match(arms),
            Op::LoadLocal(count),
            Op::PushInt(Literal::I64(1)),
            Op::Add,
            Op::StoreLocal(result),
            rest @ ..,
        ] = tail
        else {
            return None;
        };
        if result != count || !step(rest, index) {
            return None;
        }
        let [accepted, rejected] = arms.as_ref() else {
            return None;
        };
        let rejected_body = operations(&rejected.body);
        if accepted.pattern != Pattern::Bool(true)
            || !operations(&accepted.body).is_empty()
            || rejected.pattern != Pattern::Bool(false)
            || rejected_body.len() != 5
            || !matches!(rejected_body[4], Op::Continue)
            || !step(&rejected_body[..4], index)
        {
            return None;
        }
        Some(*count)
    }

    pub(super) fn lower_clist_simd_prefix(&mut self, condition: &[Op], body: &[Op]) {
        if self.gathering() {
            return;
        }
        let Some(scan) = self.clist_count_scan(condition, body) else {
            return;
        };
        let index = self.read_local(scan.index);
        let count = self.read_local(scan.count);
        // These are the generated pipeline's fresh counters. Starting at zero
        // proves the total count fits i64, including each vector accumulator.
        if known::known_int(&self.bcx.func.dfg, index) != Some(0)
            || known::known_int(&self.bcx.func.dfg, count) != Some(0)
        {
            return;
        }
        let ty = &scan.predicate.ty;
        let shape = shape(ty).expect("validated column type");
        let scalar = clif_type(ty.clone());
        let bits = scalar.bits();
        // Unsigned lanes compare as signed ones once each sign bit is flipped.
        let bias = (!shape.signed).then(|| {
            let bit = self.bcx.ins().iconst(scalar, 1i64 << (bits - 1));
            (bit, self.bcx.ins().splat(shape.vector, bit))
        });
        let address = self.read_local(scan.address);
        let length = self.read_local(scan.length);
        let end = self.bcx.ins().band_imm_s(length, -shape.lanes);
        let environment = scan
            .environment
            .as_ref()
            .map(|(slot, ty)| (self.read_local(*slot), ty));
        let comparisons: Vec<_> = scan
            .predicate
            .comparisons
            .iter()
            .map(|comparison| {
                let mut threshold = match comparison.threshold {
                    Threshold::Constant(value) => {
                        let mask = (1i128 << bits) - 1;
                        self.bcx
                            .ins()
                            .iconst(scalar, (i128::from(value) & mask) as i64)
                    }
                    Threshold::Local(slot) => self.read_local(slot),
                    Threshold::Capture(slot) => {
                        let (storage, closure) =
                            environment.as_ref().expect("validated capture environment");
                        // Read the closure's own snapshot, not the source binding.
                        // Only immutable by-value scalar captures reach this path.
                        let packed = self.bcx.ins().load(
                            types::I128,
                            MemFlagsData::trusted(),
                            *storage,
                            closure.offset(slot as usize) as i32,
                        );
                        self.unpack(packed, ty)
                    }
                };
                let mut condition = comparison.condition;
                if let Some((bit, _)) = bias {
                    threshold = self.bcx.ins().bxor(threshold, bit);
                    condition = biased(condition);
                }
                (condition, self.bcx.ins().splat(shape.vector, threshold))
            })
            .collect();
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let sum = self.bcx.ins().splat(types::I64X2, zero);
        let scalar_zero = self.bcx.ins().iconst(scalar, 0);
        let empty = self.bcx.ins().splat(shape.vector, scalar_zero);
        // Each lane counts its matches in a counter of the column's width.
        // A chunk stops before that counter can wrap; its counts are then
        // widened and added to two 64-bit lanes.
        let outer = self.bcx.create_block();
        let chunk = self.bcx.create_block();
        let inner = self.bcx.create_block();
        let iteration = self.bcx.create_block();
        let flush = self.bcx.create_block();
        let exit = self.bcx.create_block();
        self.bcx.append_block_param(outer, types::I64);
        self.bcx.append_block_param(outer, types::I64X2);
        self.bcx.append_block_param(inner, types::I64);
        self.bcx.append_block_param(inner, shape.vector);
        self.bcx.ins().jump(outer, &[index.into(), sum.into()]);
        self.bcx.switch_to_block(outer);
        let start = self.bcx.block_params(outer)[0];
        let total = self.bcx.block_params(outer)[1];
        let more = self.bcx.ins().icmp(IntCC::SignedLessThan, start, end);
        self.bcx.ins().brif(more, chunk, &[], exit, &[]);
        self.bcx.switch_to_block(chunk);
        self.bcx.seal_block(chunk);
        let most = self
            .bcx
            .ins()
            .iadd_imm_s(start, shape.iterations * shape.lanes);
        let stop = self.bcx.ins().smin(most, end);
        self.bcx.ins().jump(inner, &[start.into(), empty.into()]);
        self.bcx.switch_to_block(inner);
        let position = self.bcx.block_params(inner)[0];
        let counts = self.bcx.block_params(inner)[1];
        let go = self.bcx.ins().icmp(IntCC::SignedLessThan, position, stop);
        self.bcx.ins().brif(go, iteration, &[], flush, &[]);
        self.bcx.switch_to_block(iteration);
        self.bcx.seal_block(iteration);
        let width = i64::from(bits / 8);
        let offset = self.bcx.ins().imul_imm_s(position, width);
        let pointer = self.bcx.ins().iadd(address, offset);
        // A column need not be 16-byte aligned. The guard admits only full
        // vectors of live rows, so this never reads padding, another column
        // or a tail.
        let mut values =
            self.bcx
                .ins()
                .load(shape.vector, MemFlagsData::new().with_notrap(), pointer, 0);
        if let Some((_, lanes)) = bias {
            values = self.bcx.ins().bxor(values, lanes);
        }
        let (condition, threshold) = comparisons[0];
        let mut mask = self.bcx.ins().icmp(condition, values, threshold);
        if let Some(&(condition, threshold)) = comparisons.get(1) {
            let other = self.bcx.ins().icmp(condition, values, threshold);
            mask = self.bcx.ins().band(mask, other);
        }
        // A matching lane is all one bits, -1, so subtracting it counts one.
        let counts_next = self.bcx.ins().isub(counts, mask);
        let next = self.bcx.ins().iadd_imm_s(position, shape.lanes);
        self.bcx
            .ins()
            .jump(inner, &[next.into(), counts_next.into()]);
        self.bcx.seal_block(inner);
        self.bcx.switch_to_block(flush);
        self.bcx.seal_block(flush);
        // Counts are unsigned and at most `iterations`, so adding adjacent
        // widened halves cannot overflow the wider lanes.
        let mut wide = counts;
        while self.bcx.func.dfg.value_type(wide) != types::I64X2 {
            let low = self.bcx.ins().uwiden_low(wide);
            let high = self.bcx.ins().uwiden_high(wide);
            wide = self.bcx.ins().iadd(low, high);
        }
        let total_next = self.bcx.ins().iadd(total, wide);
        self.bcx
            .ins()
            .jump(outer, &[position.into(), total_next.into()]);
        self.bcx.seal_block(outer);
        self.bcx.switch_to_block(exit);
        self.bcx.seal_block(exit);
        let index = start;
        let a = self.bcx.ins().extractlane(total, 0);
        let b = self.bcx.ins().extractlane(total, 1);
        let count = self.bcx.ins().iadd(a, b);
        self.write_local(scan.count, count);
        self.write_local(scan.index, index);
        // Preserve the loop's final item value too; do not make observability
        // assumptions about this local merely because pipelines generate it.
        let save = self.bcx.create_block();
        let done = self.bcx.create_block();
        let ran = self
            .bcx
            .ins()
            .icmp_imm_s(IntCC::SignedGreaterThan, index, 0);
        self.bcx.ins().brif(ran, save, &[], done, &[]);
        self.bcx.switch_to_block(save);
        self.bcx.seal_block(save);
        let offset = self.bcx.ins().imul_imm_s(index, width);
        let pointer = self.bcx.ins().iadd(address, offset);
        let last = self.bcx.ins().load(
            scalar,
            MemFlagsData::new().with_notrap(),
            pointer,
            -(width as i32),
        );
        self.write_local(scan.item, last);
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
    }
}
