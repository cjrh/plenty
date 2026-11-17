//! Private typed-slot operations for the explicit allocator experiment.
//! The std owner checks extent/alignment and tracks initialized slots. These
//! operations move ownership; they are never public raw-pointer facilities.
use super::*;

pub(super) fn named(name: &str) -> bool {
    matches!(
        name,
        "__allocator_slot_size"
            | "__allocator_slot_put"
            | "__allocator_slot_take"
            | "__allocator_system"
            | "__allocator_share"
            | "__allocator_release"
            | "__allocator_list"
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
