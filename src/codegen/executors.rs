//! Monomorphic worker callbacks behind the runtime's two-pointer C ABI.
use super::*;
use crate::executor::ExecutorOp;
use cranelift_codegen::ir::MemFlags;

pub(super) fn emit_adapters(
    ops: &[Op],
    functions: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    module: &mut ObjectModule,
) -> Result<()> {
    let mut jobs = std::collections::BTreeMap::new();
    crate::threading::walk(ops, &mut |op| {
        if let Op::Executor(operation) = op {
            if let Some(job) = operation.job() {
                jobs.insert(job.adapter.clone(), job.clone());
            }
        }
    });
    for (name, job) in jobs {
        let mut signature = module.make_signature();
        signature
            .params
            .extend([AbiParam::new(PTR_TY), AbiParam::new(PTR_TY)]);
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
        let input = bcx.block_params(entry)[0];
        let output = bcx.block_params(entry)[1];
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
        let packed = lower
            .bcx
            .ins()
            .load(types::I128, MemFlags::trusted(), input, 0);
        let value = lower.unpack(packed, &job.input);
        if let Some(closure) = &job.closure {
            if closure.once {
                lower.stack.push((value, job.input.clone()));
            } else {
                let reference = lower.bcx.ins().uextend(types::I128, input);
                lower.stack.push((
                    reference,
                    Ty::Ref(Rc::new(job.input.clone()), closure.mutable),
                ));
            }
            lower.lower_closure_call(closure)?;
            if !closure.once {
                lower.release(value, &job.input);
            }
        } else {
            lower.stack.push((value, job.input.clone()));
            lower.lower_call(&job.worker)?;
        }
        let value = if job.output == Ty::Unit {
            lower.bcx.ins().iconst(types::I64, 0)
        } else {
            lower.stack.pop().ok_or("missing executor result")?.0
        };
        lower.store_slot(output, value, &job.output);
        lower.bcx.ins().return_(&[]);
        bcx.finalize();
        module
            .define_function(id, &mut ctx)
            .map_err(|e| format!("executor adapter: {e:?}"))?;
    }
    Ok(())
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_executor(&mut self, operation: &ExecutorOp) -> Result<()> {
        let (inputs, output) = operation.signature();
        let mut values = Vec::new();
        for input in inputs.iter().rev() {
            let (value, _) = self.pop_typed(input.clone())?;
            values.push(self.pack(value, input));
        }
        values.reverse();
        let args = self.inline_storage((inputs.len() + 3) * 16);
        for (i, value) in values.iter().enumerate() {
            self.bcx
                .ins()
                .store(MemFlags::trusted(), *value, args, (i * 16) as i32);
        }
        if let Some(job) = operation.job() {
            let id = self.runtime.thread_adapters.borrow()[&job.adapter];
            let callback = self.module.declare_func_in_func(id, self.bcx.func);
            let callback = self.bcx.ins().func_addr(PTR_TY, callback);
            let callback = self.bcx.ins().uextend(types::I128, callback);
            self.bcx.ins().store(
                MemFlags::trusted(),
                callback,
                args,
                (inputs.len() * 16) as i32,
            );
        }
        if let ExecutorOp::Map(job, input, _) | ExecutorOp::Reduce(job, input) = operation {
            let id = metadata::declare(self.module, self.runtime, input)?;
            let gv = self.module.declare_data_in_func(id, self.bcx.func);
            let descriptor = self.bcx.ins().global_value(PTR_TY, gv);
            let descriptor = self.bcx.ins().uextend(types::I128, descriptor);
            self.bcx
                .ins()
                .store(MemFlags::trusted(), descriptor, args, 48);
            let job_type = if matches!(operation, ExecutorOp::Reduce(..)) {
                &job.input
            } else {
                &job.output
            };
            let id = metadata::declare(self.module, self.runtime, job_type)?;
            let gv = self.module.declare_data_in_func(id, self.bcx.func);
            let descriptor = self.bcx.ins().global_value(PTR_TY, gv);
            let descriptor = self.bcx.ins().uextend(types::I128, descriptor);
            self.bcx
                .ins()
                .store(MemFlags::trusted(), descriptor, args, 64);
        }
        let out = self.inline_storage(output.slot_bytes());
        let id = metadata::declare(self.module, self.runtime, &output)?;
        let gv = self.module.declare_data_in_func(id, self.bcx.func);
        let descriptor = self.bcx.ins().global_value(PTR_TY, gv);
        let code = self.bcx.ins().iconst(types::I64, operation.opcode());
        let function = self
            .module
            .declare_func_in_func(self.runtime.executor, self.bcx.func);
        self.bcx
            .ins()
            .call(function, &[code, args, descriptor, out]);
        let value = self
            .bcx
            .ins()
            .load(types::I128, MemFlags::trusted(), out, 0);
        let value = self.unpack(value, &output);
        if !matches!(operation, ExecutorOp::New) {
            self.release(values[0], &inputs[0]);
        }
        self.stack.push((value, output));
        Ok(())
    }
}
