//! C entry adapters and pinned parent-frame storage for scoped tasks.
use super::*;
use crate::threading::{Task, ThreadOp};
use cranelift_codegen::ir::{MemFlags, StackSlotData, StackSlotKind};

pub(super) fn emit_adapters(
    ops: &[Op],
    functions: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    module: &mut ObjectModule,
) -> Result<()> {
    let mut tasks = std::collections::BTreeMap::new();
    crate::threading::walk(ops, &mut |op| {
        if let Op::Thread(ThreadOp::Start(task, _)) = op {
            tasks.insert(task.adapter.clone(), task.clone());
        }
    });
    for (name, task) in tasks {
        let mut signature = module.make_signature();
        signature.params.push(AbiParam::new(PTR_TY));
        signature.returns.push(AbiParam::new(PTR_TY));
        let id = module.declare_function(&name, Linkage::Local, &signature)?;
        runtime.thread_adapters.borrow_mut().insert(name, id);
        let mut ctx = Context::new();
        ctx.func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
        let mut fc = FunctionBuilderContext::new();
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut fc);
        let entry = bcx.create_block();
        bcx.append_block_params_for_function_params(entry);
        bcx.switch_to_block(entry);
        bcx.seal_block(entry);
        let storage = bcx.block_params(entry)[0];
        let mut lower = Lowerer {
            bcx: &mut bcx,
            module,
            runtime,
            user_fns: functions,
            str_data,
            eof_empty_str,
            locals: &[],
            stack: vec![],
            terminated: false,
            loop_targets: vec![],
            generator: None,
            local_frame: None,
            return_storage: None,
            collection_scratch: None,
        };
        for (i, ty) in task.inputs.iter().enumerate() {
            let packed = lower.bcx.ins().load(
                types::I128,
                MemFlags::trusted(),
                storage,
                task.argument_offset(i) as i32,
            );
            let value = lower.unpack(packed, ty);
            lower.stack.push((value, ty.clone()));
        }
        if let Some(closure) = &task.closure {
            if task.owned_job().is_some() && !closure.once {
                lower.stack.pop();
                let slot = lower
                    .bcx
                    .ins()
                    .iadd_imm(storage, task.argument_offset(0) as i64);
                let reference = lower.bcx.ins().uextend(types::I128, slot);
                lower.stack.push((
                    reference,
                    Ty::Ref(Rc::new(Ty::Closure(closure.clone())), closure.mutable),
                ));
            }
            lower.lower_closure_call(closure)?;
            if task.owned_job().is_some() && !closure.once {
                let environment = lower.bcx.ins().load(
                    PTR_TY,
                    MemFlags::trusted(),
                    storage,
                    task.argument_offset(0) as i32,
                );
                lower.release(environment, &Ty::Closure(closure.clone()));
            }
        } else {
            lower.lower_call(&task.worker)?;
        }
        let value = if task.output == Ty::Unit {
            lower.bcx.ins().iconst(types::I64, 0)
        } else {
            lower.stack.pop().ok_or("missing worker result")?.0
        };
        let result_slot = lower.bcx.ins().iadd_imm(storage, 16);
        lower.store_slot(result_slot, value, &task.output);
        let zero = lower.bcx.ins().iconst(PTR_TY, 0);
        lower.bcx.ins().return_(&[zero]);
        bcx.finalize();
        module
            .define_function(id, &mut ctx)
            .map_err(|e| format!("thread adapter: {e:?}"))?;
    }
    Ok(())
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_thread(&mut self, op: &ThreadOp) -> Result<()> {
        match op {
            ThreadOp::Start(task, slot) => {
                let split = self
                    .stack
                    .len()
                    .checked_sub(task.inputs.len())
                    .ok_or("thread argument underflow")?;
                let arguments: Vec<_> = self.stack.drain(split..).collect();
                let memory = self.bcx.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    task.bytes() as u32,
                    4,
                ));
                let storage = self.bcx.ins().stack_addr(PTR_TY, memory, 0);
                for (i, (value, ty)) in arguments.iter().enumerate() {
                    let dest = self
                        .bcx
                        .ins()
                        .iadd_imm(storage, task.argument_offset(i) as i64);
                    self.store_slot(dest, *value, ty);
                }
                let id = self.runtime.thread_adapters.borrow()[&task.adapter];
                let callback = self.module.declare_func_in_func(id, self.bcx.func);
                let callback = self.bcx.ins().func_addr(PTR_TY, callback);
                let start = self
                    .module
                    .declare_func_in_func(self.runtime.thread_start, self.bcx.func);
                let call = self.bcx.ins().call(start, &[storage, callback]);
                let status = self.bcx.inst_results(call)[0];
                let failed = self.bcx.ins().icmp_imm(IntCC::NotEqual, status, 0);
                let zero = self.bcx.ins().iconst(PTR_TY, 0);
                let token = self.bcx.ins().select(failed, zero, storage);
                self.write_local(*slot, token);
                let fail = self.bcx.create_block();
                let done = self.bcx.create_block();
                self.bcx.ins().brif(failed, fail, &[], done, &[]);
                self.bcx.switch_to_block(fail);
                self.bcx.seal_block(fail);
                if task.owned_job().is_none() {
                    for (value, ty) in arguments.iter().rev() {
                        self.release(*value, ty);
                    }
                }
                self.bcx.ins().jump(done, &[]);
                self.bcx.switch_to_block(done);
                self.bcx.seal_block(done);
                let result = if let Some(job) = task.owned_job() {
                    // The source environment is inert after the move into task
                    // storage. On failure it becomes the residual owner; on
                    // success only the worker owns its relocated copy.
                    let payload = self.pack(arguments[0].0, job);
                    let denied = self.bcx.ins().icmp_imm(IntCC::Equal, status, 1); // EPERM
                    let denied = self.bcx.ins().uextend(types::I128, denied);
                    let denied = self.bcx.ins().ishl_imm(denied, 64);
                    let error = self.wrap_sum(payload, 0);
                    let error = self.bcx.ins().bor(error, denied);
                    let error = self.wrap_sum(error, 1);
                    let zero = self.bcx.ins().iconst(types::I64, 0);
                    let zero = self.bcx.ins().uextend(types::I128, zero);
                    self.bcx.ins().select(failed, error, zero)
                } else {
                    let payload = self.bcx.ins().uextend(types::I128, status);
                    let tag = self.bcx.ins().uextend(types::I128, failed);
                    let tag = self.bcx.ins().ishl_imm(tag, 64);
                    self.bcx.ins().bor(payload, tag)
                };
                self.stack.push((result, task.start_result()));
            }
            ThreadOp::Join(task) => {
                let (storage, _) = self.pop_typed(Ty::Task(task.clone()))?;
                let value = self.thread_result(storage, task);
                if task.output != Ty::Unit {
                    let value = self.snapshot_inline(value, &task.output);
                    self.stack.push((value, task.output.clone()));
                }
            }
            ThreadOp::Finish(task, slot) => {
                let storage = self.read_local(*slot);
                let live = self.bcx.ins().icmp_imm(IntCC::NotEqual, storage, 0);
                let join = self.bcx.create_block();
                let done = self.bcx.create_block();
                self.bcx.ins().brif(live, join, &[], done, &[]);
                self.bcx.switch_to_block(join);
                self.bcx.seal_block(join);
                let value = self.thread_result(storage, task);
                self.release(value, &task.output);
                self.bcx.ins().jump(done, &[]);
                self.bcx.switch_to_block(done);
                self.bcx.seal_block(done);
                let zero = self.bcx.ins().iconst(PTR_TY, 0);
                self.write_local(*slot, zero);
            }
        }
        Ok(())
    }

    fn thread_result(
        &mut self,
        storage: cranelift_codegen::ir::Value,
        task: &Task,
    ) -> cranelift_codegen::ir::Value {
        let join = self
            .module
            .declare_func_in_func(self.runtime.thread_join, self.bcx.func);
        self.bcx.ins().call(join, &[storage]);
        let packed = self
            .bcx
            .ins()
            .load(types::I128, MemFlags::trusted(), storage, 16);
        self.unpack(packed, &task.output)
    }
}
