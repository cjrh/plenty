//! Lower resumable bodies directly to native basic blocks. Every yield is a
//! checked empty-stack boundary; locals live in the owned frame, never stale SSA.
use super::*;
use cranelift_codegen::ir::{MemFlags, Value as NativeValue};

pub(super) struct GeneratorContext {
    pub frame: NativeValue,
    pub out: NativeValue,
    pub continuations: Vec<Block>,
}

pub(super) fn resume_signature(module: &ObjectModule) -> Signature {
    let mut sig = module.make_signature();
    sig.call_conv = CallConv::SystemV;
    sig.params.extend([AbiParam::new(PTR_TY); 2]);
    sig.returns.push(AbiParam::new(types::I8));
    sig
}

pub(super) fn emit_generator(
    name: &str,
    fns: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    module: &mut ObjectModule,
) -> Result<()> {
    let decl = &fns[name];
    let resume_id = decl.resume.unwrap();
    let slot_types: Vec<_> = decl
        .sig
        .inputs
        .iter()
        .map(|(_, t)| t.clone())
        .chain(decl.locals.iter().cloned())
        .collect();
    let mask_id = module.declare_data(
        &format!("__plenty_frame_{name}"),
        Linkage::Local,
        false,
        false,
    )?;
    let mut mask = DataDescription::new();
    mask.define(vec![0; slot_types.len().max(1) * 8].into_boxed_slice());
    mask.set_align(8);
    for (i, ty) in slot_types.iter().enumerate() {
        let id = metadata::declare(module, runtime, ty)?;
        let reference = module.declare_data_in_data(id, &mut mask);
        mask.write_data_addr((i * 8) as u32, reference, 0);
    }
    module.define_data(mask_id, &mask)?;

    // Constructor: transfer arguments into zero-initialized frame slots. No
    // source body instruction runs until the first next/iteration.
    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(
        UserFuncName::user(0, decl.id.as_u32()),
        user_fn_signature(module, &decl.sig),
    );
    let mut fn_ctx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fn_ctx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let resume = module.declare_func_in_func(resume_id, b.func);
        let callback = b.ins().func_addr(PTR_TY, resume);
        let mask = module.declare_data_in_func(mask_id, b.func);
        let mask = b.ins().global_value(PTR_TY, mask);
        let count = b.ins().iconst(types::I64, slot_types.len() as i64);
        let new = module.declare_func_in_func(runtime.generator_new, b.func);
        let call = b.ins().call(new, &[callback, count, mask]);
        let frame = b.inst_results(call)[0];
        for (i, (_, ty)) in decl.sig.inputs.iter().enumerate() {
            let value = b.block_params(entry)[i];
            let value = enums::pack_value(&mut b, value, ty);
            b.ins()
                .store(MemFlags::trusted(), value, frame, 64 + i as i32 * 16);
        }
        b.ins().return_(&[frame]);
        b.finalize();
    }
    module.define_function(decl.id, &mut ctx)?;

    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(
        UserFuncName::user(0, resume_id.as_u32()),
        resume_signature(module),
    );
    let mut fn_ctx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fn_ctx);
        let dispatch = b.create_block();
        b.append_block_params_for_function_params(dispatch);
        b.switch_to_block(dispatch);
        let frame = b.block_params(dispatch)[0];
        let out = b.block_params(dispatch)[1];
        let choose_state = b.create_block();
        b.ins().jump(choose_state, &[]);
        let start = b.create_block();
        b.switch_to_block(start);
        let locals = slot_types
            .into_iter()
            .map(|ty| (b.declare_var(clif_type(ty.clone())), ty))
            .collect::<Vec<_>>();
        let mut lower = Lowerer {
            bcx: &mut b,
            module,
            runtime,
            user_fns: fns,
            str_data,
            eof_empty_str,
            locals: &locals,
            stack: Vec::new(),
            terminated: false,
            loop_targets: Vec::new(),
            local_frame: None,
            collection_scratch: None,
            generator: Some(GeneratorContext {
                frame,
                out,
                continuations: vec![start],
            }),
        };
        for op in decl.body.iter() {
            if lower.terminated {
                break;
            }
            lower.lower(op)?;
        }
        if !lower.terminated {
            lower.complete_generator();
        }
        let continuations = lower.generator.take().unwrap().continuations;
        b.switch_to_block(choose_state);
        let state = b.ins().load(types::I64, MemFlags::trusted(), frame, 24);
        for (i, block) in continuations.into_iter().enumerate() {
            let yes = b.ins().icmp_imm(IntCC::Equal, state, i as i64);
            let next = b.create_block();
            b.ins().brif(yes, block, &[], next, &[]);
            b.switch_to_block(next);
        }
        b.ins().trap(TrapCode::unwrap_user(1));
        b.seal_all_blocks();
        b.finalize();
    }
    module
        .define_function(resume_id, &mut ctx)
        .map_err(|error| -> Box<dyn Error> {
            format!("in generator `{name}`: {error:?}").into()
        })?;
    Ok(())
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_next(&mut self, slot: u8) -> Result<()> {
        let Ty::Generator(element) = self.locals[slot as usize].1.clone() else {
            return Err("next requires generator".into());
        };
        let output = crate::sum::option((*element).clone());
        let frame = self.read_local(slot);
        let value = self.collection_call(24, &[frame], Some(&output))?;
        self.stack.push((value, output));
        Ok(())
    }
    pub(super) fn lower_yield(&mut self, ty: &Ty) -> Result<()> {
        let (value, _) = self.pop_typed(ty.clone())?;
        if !self.stack.is_empty() {
            return Err("nonempty stack at yield".into());
        }
        let value = self.pack(value, ty);
        let g = self.generator.as_mut().ok_or("yield outside generator")?;
        let next = self.bcx.create_block();
        let state = self
            .bcx
            .ins()
            .iconst(types::I64, g.continuations.len() as i64);
        g.continuations.push(next);
        self.bcx
            .ins()
            .store(MemFlags::trusted(), state, g.frame, 24);
        self.bcx.ins().store(MemFlags::trusted(), value, g.out, 0);
        let ready = self.bcx.ins().iconst(types::I8, 1);
        self.bcx.ins().return_(&[ready]);
        self.bcx.switch_to_block(next);
        Ok(())
    }
    pub(super) fn complete_generator(&mut self) {
        let frame = self.generator.as_ref().unwrap().frame;
        let finish = self
            .module
            .declare_func_in_func(self.runtime.generator_finish, self.bcx.func);
        self.bcx.ins().call(finish, &[frame]);
        let done = self.bcx.ins().iconst(types::I8, 0);
        self.bcx.ins().return_(&[done]);
        self.terminated = true;
    }
}
