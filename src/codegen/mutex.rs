//! `std.sync.Mutex` lowering: the uncontended paths of the three-state futex
//! protocol (Drepper, "Futexes Are Tricky", mutex 2) are inline atomics on the
//! state word; only a contended lock or an unlock with waiters calls the
//! runtime. State 0 is unlocked, 1 locked, 2 locked with possible waiters.
use super::*;
use crate::mutex::MutexOp;
use cranelift_codegen::ir::{AtomicRmwOp, MemFlagsData};

impl Lowerer<'_, '_> {
    pub(super) fn lower_mutex(&mut self, operation: &MutexOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let (value, _) = self.pop_typed(inputs[0].clone())?;
        let result = match operation {
            MutexOp::Exclusive(_) => value,
            MutexOp::IntoInner(class) => {
                let offset = class.get().state_bytes() as i64
                    + Self::record_offset(
                        class.get().fields[..crate::mutex::VALUE]
                            .iter()
                            .map(|(_, t)| t),
                    );
                self.record_field(value, offset, &output).0
            }
            MutexOp::Lock(_) => {
                let address = self.raw_word(value);
                let unlocked = self.bcx.ins().iconst(types::I32, 0);
                let locked = self.bcx.ins().iconst(types::I32, 1);
                let previous =
                    self.bcx
                        .ins()
                        .atomic_cas(MemFlagsData::trusted(), address, unlocked, locked);
                let contended = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, previous, 0);
                let slow = self.runtime.mutex_lock_slow;
                self.call_if(contended, slow, address);
                self.bcx.ins().iconst(types::I64, 0)
            }
            MutexOp::Unlock(_) => {
                let address = self.raw_word(value);
                let unlocked = self.bcx.ins().iconst(types::I32, 0);
                let previous = self.bcx.ins().atomic_rmw(
                    types::I32,
                    MemFlagsData::trusted(),
                    AtomicRmwOp::Xchg,
                    address,
                    unlocked,
                );
                let waiters = self.bcx.ins().icmp_imm_s(IntCC::Equal, previous, 2);
                let wake = self.runtime.mutex_wake;
                self.call_if(waiters, wake, address);
                self.bcx.ins().iconst(types::I64, 0)
            }
        };
        self.stack.push((result, output));
        Ok(())
    }

    /// Call `function(address)` only when `condition` holds, on a cold path.
    fn call_if(
        &mut self,
        condition: cranelift_codegen::ir::Value,
        function: FuncId,
        address: cranelift_codegen::ir::Value,
    ) {
        let call = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.set_cold_block(call);
        self.bcx.ins().brif(condition, call, &[], done, &[]);
        self.bcx.switch_to_block(call);
        self.bcx.seal_block(call);
        let callee = self.module.declare_func_in_func(function, self.bcx.func);
        self.bcx.ins().call(callee, &[address]);
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
    }
}
