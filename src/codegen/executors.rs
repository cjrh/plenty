//! Monomorphic worker callbacks with input, output, and shared-environment slots.
use super::*;
use crate::executor::{ExecutorOp, Job};
use cranelift_codegen::ir::MemFlagsData;

/// Declare one adapter per executor job, in name order.
pub(super) fn declare_adapters(
    ops: &[Op],
    workers: usize,
    module: &mut ObjectModule,
    adapters: &mut HashMap<String, FuncId>,
) -> Result<Vec<(Arc<Job>, FuncId)>> {
    // An adapter's name identifies its job, so any one use of it will do.
    let jobs: std::collections::BTreeMap<_, _> =
        crate::threading::find(ops, workers, |op| match op {
            Op::Executor(operation) => operation
                .job()
                .map(|job| (job.adapter.clone(), job.clone())),
            _ => None,
        })
        .into_iter()
        .collect();
    let mut declared = Vec::new();
    for (name, job) in jobs {
        let id =
            module.declare_function(&name, Linkage::Local, &signature(module.make_signature()))?;
        adapters.insert(name, id);
        declared.push((job, id));
    }
    Ok(declared)
}

/// Input, output, and shared-environment slots.
fn signature(mut signature: Signature) -> Signature {
    signature.params.extend([
        AbiParam::new(PTR_TY),
        AbiParam::new(PTR_TY),
        AbiParam::new(PTR_TY),
    ]);
    signature
}

pub(super) fn build_adapter(
    job: &Job,
    id: FuncId,
    functions: &HashMap<String, UserFn>,
    str_data: &StrData,
    eof_empty_str: DataId,
    runtime: &Runtime,
    out: &mut Worker,
) -> Result<()> {
    {
        let mut module = out.symbols();
        let signature = signature(module.make_signature());
        let mut func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
        let mut bcx = FunctionBuilder::new(&mut func, &mut out.builder);
        let entry = bcx.create_block();
        bcx.append_block_params_for_function_params(entry);
        bcx.switch_to_block(entry);
        bcx.seal_block(entry);
        let input = bcx.block_params(entry)[0];
        let output = bcx.block_params(entry)[1];
        let shared = bcx.block_params(entry)[2];
        let mut lower = Lowerer {
            bcx: &mut bcx,
            module: &mut module,
            runtime,
            user_fns: functions,
            str_data,
            eof_empty_str,
            locals: &[],
            stack: vec![],
            terminated: false,
            loop_targets: vec![],
            checked_targets: vec![],
            generator: None,
            local_frame: None,
            return_storage: None,
            return_types: &[],
            collection_scratch: None,
            fresh_load: None,
            scalar_captures: Default::default(),
            portable_simd: Default::default(),
            slot_copies: Vec::new(),
            empty_slot: None,
            exits: None,
            counters: Default::default(),
            position: Default::default(),
            trap_blocks: HashMap::default(),
            moves: Default::default(),
        };
        let packed = lower
            .bcx
            .ins()
            .load(types::I128, MemFlagsData::trusted(), input, 0);
        let value = lower.unpack(packed, &job.input);
        if let Some(closure) = &job.shared {
            let reference = lower.bcx.ins().uextend(types::I128, shared);
            lower.stack.push((
                reference,
                Ty::Ref(Arc::new(Ty::Closure(closure.clone())), false),
            ));
            lower.stack.push((value, job.input.clone()));
            lower.lower_closure_call(closure)?;
        } else if let Some(closure) = &job.closure {
            if closure.once {
                lower.stack.push((value, job.input.clone()));
            } else {
                let reference = lower.bcx.ins().uextend(types::I128, input);
                lower.stack.push((
                    reference,
                    Ty::Ref(Arc::new(job.input.clone()), closure.mutable),
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
        lower.seal_trap_blocks();
        bcx.finalize(crate::codegen::FRONTEND);
        out.define(id, func, module);
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
        let args = self.inline_storage((inputs.len() + 5) * 16);
        for (i, value) in values.iter().enumerate() {
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), *value, args, (i * 16) as i32);
        }
        if let Some(job) = operation.job() {
            let id = self.runtime.thread_adapters[&job.adapter];
            let callback = self.module.declare_func_in_func(id, self.bcx.func);
            let callback = self.bcx.ins().func_addr(PTR_TY, callback);
            let callback = self.bcx.ins().uextend(types::I128, callback);
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), callback, args, 32);
        }
        if let ExecutorOp::Map(job, _, _) = operation {
            let shared = if job.shared.is_some() {
                self.bcx.ins().iadd_imm_s(args, 80)
            } else {
                self.bcx.ins().iconst(PTR_TY, 0)
            };
            if job.shared.is_some() {
                self.bcx
                    .ins()
                    .store(MemFlagsData::trusted(), values[1], args, 80);
                self.bcx
                    .ins()
                    .store(MemFlagsData::trusted(), values[2], args, 16);
            }
            let shared = self.bcx.ins().uextend(types::I128, shared);
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), shared, args, 96);
        }
        if let ExecutorOp::Map(job, input, _) | ExecutorOp::Reduce(job, input) = operation {
            let id = self.module.metadata(input);
            let gv = self.module.declare_data_in_func(id, self.bcx.func);
            let descriptor = self.bcx.ins().symbol_value(PTR_TY, gv);
            let descriptor = self.bcx.ins().uextend(types::I128, descriptor);
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), descriptor, args, 48);
            let job_type = if matches!(operation, ExecutorOp::Reduce(..)) {
                &job.input
            } else {
                &job.output
            };
            let id = self.module.metadata(job_type);
            let gv = self.module.declare_data_in_func(id, self.bcx.func);
            let descriptor = self.bcx.ins().symbol_value(PTR_TY, gv);
            let descriptor = self.bcx.ins().uextend(types::I128, descriptor);
            self.bcx
                .ins()
                .store(MemFlagsData::trusted(), descriptor, args, 64);
        }
        let out = self.inline_storage(output.slot_bytes());
        let id = self.module.metadata(&output);
        let gv = self.module.declare_data_in_func(id, self.bcx.func);
        let descriptor = self.bcx.ins().symbol_value(PTR_TY, gv);
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
            .load(types::I128, MemFlagsData::trusted(), out, 0);
        let value = self.unpack(value, &output);
        if !matches!(operation, ExecutorOp::New) {
            self.release(values[0], &inputs[0]);
        }
        if let ExecutorOp::Map(job, _, _) = operation
            && job.shared.is_some()
        {
            self.release(values[1], &inputs[1]);
        }
        self.stack.push((value, output));
        Ok(())
    }
}
