//! Private allocator operations, including experimental typed slots and
//! retained provider registration. Ownership transfers stay compiler/std-only;
//! these are never public raw-pointer or general-purpose forget facilities.
use super::*;

pub(super) fn named(name: &str) -> bool {
    matches!(
        name,
        "__allocator_slot_size"
            | "__allocator_slot_put"
            | "__allocator_slot_take"
            | "__allocator_register"
            | "__allocator_commit"
            | "__allocator_block_take"
            | "__allocator_system"
            | "__allocator_share"
            | "__allocator_release"
            | "__allocator_list"
            | "__allocator_dict_set"
            | "__allocator_box"
            | "__allocator_block"
            | "__allocator_block_release"
            | "__allocator_block_len"
            | "__allocator_block_alignment"
            | "__allocator_block_get"
            | "__allocator_block_set"
            | "__allocator_block_split"
    )
}

impl Lower<'_> {
    pub(super) fn allocator_storage(
        &mut self,
        name: &str,
        types: &[TypeRef],
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if !at
            .source()
            .as_ref()
            .is_some_and(|source| self.generics.source_facts(source).1)
        {
            return Err(at.error(format!("`{name}` is reserved for the standard library")));
        }
        if matches!(
            name,
            "__allocator_register" | "__allocator_commit" | "__allocator_block_take"
        ) {
            return self.allocator_provider(name, types, args, at, ops);
        }
        if !name.starts_with("__allocator_slot_") {
            return self.retained_allocator(name, types, args, at, ops);
        }
        let [element] = types else {
            return Err(at.error(format!("`{name}` requires one element type")));
        };
        let element = element.resolve(self.aliases)?.unwrap_or(Ty::Unit);
        if !element.heap_storable() || crate::boxed::erasure_barrier(&element) {
            return Err(at.error(format!("allocator slots cannot store `{element}`: borrowed or hidden task/channel ownership is unsupported")));
        }
        let count = match name {
            "__allocator_slot_size" => 0,
            "__allocator_slot_put" => 3,
            _ => 2,
        };
        if args.len() != count {
            return Err(at.error(format!("`{name}` requires {count} arguments")));
        }
        if count == 0 {
            let size = i64::try_from(element.slot_bytes())
                .map_err(|_| at.error("allocator slot layout exceeds i64"))?;
            ops.push(Op::PushInt(Value::I64(size)));
            return Ok(Some(Ty::I64));
        }
        let pointer = self.value(&args[0], ops)?;
        if !matches!(&pointer, Ty::ForeignPtr(name, _) if name.as_ref() == "std.experimental.alloc.Pointer")
        {
            return Err(at.error("allocator slots require the private allocator storage pointer"));
        }
        let offset = self.value(&args[1], ops)?;
        self.same(Some(offset), Some(Ty::U64), &args[1].at)?;
        let mut inputs = vec![("pointer".into(), pointer), ("offset".into(), Ty::U64)];
        let put = count == 3;
        if put {
            let actual = self.value(&args[2], ops)?;
            self.same(Some(actual), Some(element.clone()), &args[2].at)?;
            inputs.push(("value".into(), element.clone()));
        }
        let output = (!put).then_some(element);
        ops.push(Op::ForeignCall {
            declaration: crate::foreign::Declaration {
                target: crate::foreign::Target::Intrinsic(Arc::new(if put {
                    crate::foreign::Intrinsic::SlotPut
                } else {
                    crate::foreign::Intrinsic::SlotTake
                })),
                arguments: vec![crate::foreign::Argument::Direct; count],
                thread_safe: false,
                host_callback: false,
            },
            sig: Arc::new(FnSig {
                inputs,
                outputs: output.iter().cloned().collect(),
            }),
        });
        Ok(output)
    }
}

impl Lower<'_> {
    fn allocator_provider(
        &mut self,
        name: &str,
        types: &[TypeRef],
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        use crate::foreign::{Argument as Mode, Declaration, Intrinsic, Target};
        let mut inputs = Vec::new();
        let count = if name == "__allocator_register" { 2 } else { 1 };
        if args.len() != count {
            return Err(at.error(format!("`{name}` requires {count} arguments")));
        }
        for (i, arg) in args.iter().enumerate() {
            inputs.push((format!("arg{i}"), self.value(arg, ops)?));
        }
        let (intrinsic, output) = if name == "__allocator_block_take" {
            if !types.is_empty()
                || !matches!(&inputs[0].1, Ty::Class(t) if t.name.as_ref() == "std.alloc.ByteBlock")
            {
                return Err(at.error("allocator block transfer requires owned std.alloc.ByteBlock"));
            }
            (
                Intrinsic::AllocatorBlockTake,
                Some(Ty::Array(Arc::new(Ty::U64), 3)),
            )
        } else {
            let [state] = types else {
                return Err(at.error("allocator registration requires one provider type"));
            };
            let state = state.resolve(self.aliases)?.unwrap_or(Ty::Unit);
            if name == "__allocator_commit" {
                self.same(Some(inputs[0].1.clone()), Some(state), at)?;
                (Intrinsic::AllocatorCommit, None)
            } else {
                let handle = Ty::ForeignPtr(
                    "std.alloc.Handle".into(),
                    crate::foreign::Threads::Shareable,
                );
                self.same(Some(inputs[0].1.clone()), Some(handle.clone()), at)?;
                self.same(
                    Some(inputs[1].1.clone()),
                    Some(Ty::Ref(Arc::new(state.clone()), true)),
                    at,
                )?;
                let allocate = self.specialize(
                    "std.alloc._provider_allocate",
                    vec![Argument::Type(state.clone())],
                    at,
                )?;
                let release = self.specialize(
                    "std.alloc._provider_release",
                    vec![Argument::Type(state.clone())],
                    at,
                )?;
                let provider = Arc::new(crate::threading::AllocatorProvider {
                    state,
                    allocate,
                    release,
                });
                ops.push(Op::Thread(crate::threading::ThreadOp::AllocatorProvider(
                    provider.clone(),
                )));
                (
                    Intrinsic::AllocatorRegister(provider),
                    Some(crate::sum::result(handle, Ty::I64)),
                )
            }
        };
        ops.push(Op::Thread(crate::threading::ThreadOp::AllocatorUse));
        ops.push(Op::ForeignCall {
            declaration: Declaration {
                target: Target::Intrinsic(Arc::new(intrinsic)),
                arguments: vec![Mode::Direct; inputs.len()],
                thread_safe: true,
                host_callback: false,
            },
            sig: Arc::new(FnSig {
                inputs,
                outputs: output.iter().cloned().collect(),
            }),
        });
        Ok(output)
    }
}
