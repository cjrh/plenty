//! A bounded native optimization of checked column filter/count loops. The
//! original scalar loop remains the tail and the fallback for every other shape.
use super::*;
use crate::clist::ClistOp;
use crate::collection::CollectionOp;
use crate::op::{IntegerOp, Pattern};
use crate::value::Value as Literal;
use cranelift_codegen::ir::MemFlagsData;

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

fn step(ops: &[&Op], index: u8) -> bool {
    matches!(ops, [Op::LoadLocal(a), Op::PushInt(Literal::I64(1)),
        Op::Integer(IntegerOp::WrappingAdd), Op::StoreLocal(b)] if *a == index && *b == index)
}

struct Scan {
    address: u8,
    index: u8,
    length: u8,
    item: u8,
    count: u8,
    comparison: IntCC,
    constant: i64,
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
        let (call, tail) = tail.split_at_checked(tail.len().checked_sub(9)?)?;
        let (callee, environment, argument) = match call {
            [
                Op::BorrowLocal(env, false),
                Op::BorrowLocal(arg, false),
                Op::ClosureCall(closure),
            ] if closure.captures.is_empty() && !closure.mutable && !closure.once => {
                (closure.name.as_str(), Some(*env), *arg)
            }
            [Op::BorrowLocal(arg, false), Op::Call(name)] => (name.as_str(), None, *arg),
            _ => return None,
        };
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
        if !matches!(ty, Ty::I32 | Ty::U32)
            || position != index
            || argument != *item
            || result != count
            || !step(rest, *index)
        {
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
            || !step(&rejected_body[..4], *index)
        {
            return None;
        }
        let slots = [*address, *index, *length, *item, *count];
        if environment.is_some_and(|env| slots.contains(&env)) {
            return None;
        }
        for (i, slot) in slots.iter().enumerate() {
            if slots[..i].contains(slot) {
                return None;
            }
        }
        // No alias may change the source bounds, index or count behind our back.
        for slot in [*address, *index, *length, *count] {
            if self.locals[slot as usize].1 != Ty::I64 || self.local_address(slot).is_some() {
                return None;
            }
        }
        let (comparison, constant) = self
            .user_fns
            .get(callee)?
            .inline
            .as_ref()?
            .column_comparison(ty)?;
        Some(Scan {
            address: *address,
            index: *index,
            length: *length,
            item: *item,
            count: *count,
            comparison,
            constant,
        })
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
        let address = self.read_local(scan.address);
        let length = self.read_local(scan.length);
        let end = self.bcx.ins().band_imm_s(length, -4);
        let threshold = self.bcx.ins().iconst(types::I32, scan.constant);
        let threshold = self.bcx.ins().splat(types::I32X4, threshold);
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let sum = self.bcx.ins().splat(types::I64X2, zero);
        let header = self.bcx.create_block();
        let iteration = self.bcx.create_block();
        let exit = self.bcx.create_block();
        self.bcx.append_block_param(header, types::I64);
        self.bcx.append_block_param(header, types::I64X2);
        self.bcx.ins().jump(header, &[index.into(), sum.into()]);
        self.bcx.switch_to_block(header);
        let index = self.bcx.block_params(header)[0];
        let sum = self.bcx.block_params(header)[1];
        let more = self.bcx.ins().icmp(IntCC::SignedLessThan, index, end);
        self.bcx.ins().brif(more, iteration, &[], exit, &[]);
        self.bcx.switch_to_block(iteration);
        self.bcx.seal_block(iteration);
        let offset = self.bcx.ins().ishl_imm_s(index, 2);
        let pointer = self.bcx.ins().iadd(address, offset);
        // A column need not be 16-byte aligned. The guard admits only four
        // live rows, so this never reads padding, another column or a tail.
        let values =
            self.bcx
                .ins()
                .load(types::I32X4, MemFlagsData::new().with_notrap(), pointer, 0);
        let mask = self.bcx.ins().icmp(scan.comparison, values, threshold);
        // Widen -1/0 masks before summing: 32-bit lane counters would wrap on
        // a valid large column. Each i64 lane counts at most half its rows.
        // Comparison lanes are all-zero or all-one bits, so duplicating each
        // 32-bit lane sign-extends it without a second vector comparison.
        let widen = |this: &mut Self, lanes: [u8; 16]| {
            let endian =
                MemFlagsData::new().with_endianness(cranelift_codegen::ir::Endianness::Little);
            let bytes = this.bcx.ins().bitcast(types::I8X16, endian, mask);
            let immediate = this.bcx.func.dfg.immediates.push(lanes.to_vec().into());
            let shuffled = this.bcx.ins().shuffle(bytes, bytes, immediate);
            this.bcx.ins().bitcast(types::I64X2, endian, shuffled)
        };
        let low = widen(self, [0, 1, 2, 3, 0, 1, 2, 3, 4, 5, 6, 7, 4, 5, 6, 7]);
        let high = widen(
            self,
            [8, 9, 10, 11, 8, 9, 10, 11, 12, 13, 14, 15, 12, 13, 14, 15],
        );
        let pair = self.bcx.ins().iadd(low, high);
        let sum = self.bcx.ins().isub(sum, pair);
        let next = self.bcx.ins().iadd_imm_s(index, 4);
        self.bcx.ins().jump(header, &[next.into(), sum.into()]);
        self.bcx.seal_block(header);
        self.bcx.switch_to_block(exit);
        self.bcx.seal_block(exit);
        let sum = self.bcx.block_params(header)[1];
        let a = self.bcx.ins().extractlane(sum, 0);
        let b = self.bcx.ins().extractlane(sum, 1);
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
        let offset = self.bcx.ins().ishl_imm_s(index, 2);
        let pointer = self.bcx.ins().iadd(address, offset);
        let last = self
            .bcx
            .ins()
            .load(types::I32, MemFlagsData::new().with_notrap(), pointer, -4);
        self.write_local(scan.item, last);
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
    }
}
