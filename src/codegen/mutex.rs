//! `std.sync.Mutex` lowering: the uncontended paths of the three-state futex
//! protocol (Drepper, "Futexes Are Tricky", mutex 2) are inline atomics on the
//! state word; only a contended lock, an unlock with lock or condition
//! waiters, and a `lock_when` wait call the runtime. State bits 0-1 are 0
//! unlocked, 1 locked, 2 locked with possible waiters; bit 2 (4) records
//! condition waiters. See plenty-runtime/src/mutex.rs.
use super::*;
use crate::mutex::MutexOp;
use cranelift_codegen::ir::{AtomicRmwOp, MemFlagsData};

impl Lowerer<'_, '_> {
    pub(super) fn lower_mutex(&mut self, operation: &MutexOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let mut values = Vec::with_capacity(inputs.len());
        for input in inputs.into_iter().rev() {
            values.push(self.pop_typed(input)?.0);
        }
        values.reverse();
        let value = values[0];
        let result = match operation {
            MutexOp::Exclusive(_) => value,
            MutexOp::IntoInner(class) => {
                let offset = class.get().field_offset(crate::mutex::VALUE) as i64;
                self.class_field(value, offset, &output)
            }
            MutexOp::Lock(_) => {
                let address = self.raw_word(value);
                let previous = self.acquire(address);
                let contended = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, previous, 0);
                let slow = self.runtime.mutex_lock_slow;
                self.call_if(contended, slow, &[address]);
                self.bcx.ins().iconst(types::I64, 0)
            }
            MutexOp::LockNow(_) => {
                // A failed attempt leaves the state as found and records no
                // waiter. Only condition waiters (4) leave an unlocked mutex
                // nonzero; the runtime retries that case without waiting.
                let address = self.raw_word(value);
                let previous = self.acquire(address);
                let acquired = self.bcx.ins().icmp_imm_s(IntCC::Equal, previous, 0);
                let marked = self.bcx.ins().icmp_imm_s(IntCC::Equal, previous, 4);
                let retry = self.bcx.create_block();
                let done = self.bcx.create_block();
                self.bcx.append_block_param(done, types::I8);
                self.bcx.set_cold_block(retry);
                self.bcx
                    .ins()
                    .brif(marked, retry, &[], done, &[acquired.into()]);
                self.bcx.switch_to_block(retry);
                self.bcx.seal_block(retry);
                let callee = self
                    .module
                    .declare_func_in_func(self.runtime.mutex_try_lock, self.bcx.func);
                let call = self.bcx.ins().call(callee, &[address]);
                let retried = self.bcx.inst_results(call)[0];
                self.bcx.ins().jump(done, &[retried.into()]);
                self.bcx.switch_to_block(done);
                self.bcx.seal_block(done);
                self.bcx.block_params(done)[0]
            }
            MutexOp::Unlock(_) => {
                let address = self.raw_word(value);
                let sequence = self.raw_word(values[1]);
                let unlocked = self.bcx.ins().iconst(types::I32, 0);
                let previous = self.bcx.ins().atomic_rmw(
                    types::I32,
                    MemFlagsData::trusted(),
                    AtomicRmwOp::Xchg,
                    address,
                    unlocked,
                );
                let waiters = self
                    .bcx
                    .ins()
                    .icmp_imm_s(IntCC::UnsignedGreaterThan, previous, 1);
                let release = self.runtime.mutex_release;
                self.call_if(waiters, release, &[address, sequence, previous]);
                self.bcx.ins().iconst(types::I64, 0)
            }
            MutexOp::Wait(..) => {
                let address = self.raw_word(value);
                let sequence = self.raw_word(values[1]);
                let callee = self
                    .module
                    .declare_func_in_func(self.runtime.mutex_wait, self.bcx.func);
                let call = self.bcx.ins().call(callee, &[address, sequence, values[2]]);
                self.bcx.inst_results(call)[0]
            }
        };
        self.stack.push((result, output));
        Ok(())
    }

    /// The inline compare-exchange from 0 to 1; returns the state it found.
    fn acquire(&mut self, address: cranelift_codegen::ir::Value) -> cranelift_codegen::ir::Value {
        let unlocked = self.bcx.ins().iconst(types::I32, 0);
        let locked = self.bcx.ins().iconst(types::I32, 1);
        self.bcx
            .ins()
            .atomic_cas(MemFlagsData::trusted(), address, unlocked, locked)
    }

    /// Call `function(arguments)` only when `condition` holds, on a cold path.
    fn call_if(
        &mut self,
        condition: cranelift_codegen::ir::Value,
        function: FuncId,
        arguments: &[cranelift_codegen::ir::Value],
    ) {
        let call = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.set_cold_block(call);
        self.bcx.ins().brif(condition, call, &[], done, &[]);
        self.bcx.switch_to_block(call);
        self.bcx.seal_block(call);
        let callee = self.module.declare_func_in_func(function, self.bcx.func);
        self.bcx.ins().call(callee, arguments);
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
    }
}
