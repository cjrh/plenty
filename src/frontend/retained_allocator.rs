//! Trusted std.alloc entry points; every use participates in actor exclusion.
use super::*;

impl Lower<'_> {
    pub(super) fn retained_allocator(
        &mut self,
        name: &str,
        types: &[TypeRef],
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        use crate::collection_opcodes as code;
        let handle = Ty::ForeignPtr(
            "std.alloc.Handle".into(),
            crate::foreign::Threads::Shareable,
        );
        let storage = Ty::Array(Arc::new(Ty::U64), 3);
        let shared = Ty::Ref(Arc::new(storage.clone()), false);
        let exclusive = Ty::Ref(Arc::new(storage.clone()), true);
        let (opcode, inputs, output, descriptor) = match name {
            "__allocator_system" => (code::ALLOCATOR_SYSTEM, vec![], Some(handle.clone()), None),
            "__allocator_share" => (
                code::ALLOCATOR_SHARE,
                vec![handle.clone()],
                Some(handle.clone()),
                None,
            ),
            "__allocator_release" => (code::ALLOCATOR_RELEASE, vec![handle.clone()], None, None),
            "__allocator_block" => (
                code::ALLOCATOR_BLOCK,
                vec![handle, Ty::I64, Ty::I64],
                Some(crate::sum::result(
                    storage.clone(),
                    crate::sum::alloc_error(),
                )),
                None,
            ),
            "__allocator_block_release" => {
                (code::ALLOCATOR_BLOCK_RELEASE, vec![exclusive], None, None)
            }
            "__allocator_block_len" => {
                (code::ALLOCATOR_BLOCK_LEN, vec![shared], Some(Ty::I64), None)
            }
            "__allocator_block_alignment" => (
                code::ALLOCATOR_BLOCK_ALIGNMENT,
                vec![shared],
                Some(Ty::I64),
                None,
            ),
            "__allocator_block_get" => (
                code::ALLOCATOR_BLOCK_GET,
                vec![shared, Ty::I64],
                Some(Ty::U8),
                None,
            ),
            "__allocator_block_set" => (
                code::ALLOCATOR_BLOCK_SET,
                vec![exclusive, Ty::I64, Ty::U8],
                None,
                None,
            ),
            "__allocator_block_split" => (
                code::ALLOCATOR_BLOCK_SPLIT,
                vec![exclusive, Ty::I64],
                Some(storage),
                None,
            ),
            "__allocator_box" => {
                let [element] = types else {
                    return Err(at.error("allocator box requires one element type"));
                };
                let element = element.resolve(self.aliases)?.unwrap_or(Ty::Unit);
                if !element.heap_storable() || crate::boxed::erasure_barrier(&element) {
                    return Err(at.error(format!("allocator boxes cannot store `{element}`: borrowed or hidden task/channel ownership is unsupported")));
                }
                let boxed = Ty::Box(Arc::new(element.clone()));
                (
                    code::ALLOCATOR_BOX,
                    vec![handle, Ty::Ref(Arc::new(element), true)],
                    Some(crate::sum::result(boxed.clone(), crate::sum::alloc_error())),
                    Some(boxed),
                )
            }
            "__allocator_list" => {
                let [element] = types else {
                    return Err(at.error("allocator list requires one element type"));
                };
                let element = element.resolve(self.aliases)?.unwrap_or(Ty::Unit);
                if !element.heap_storable() || crate::boxed::erasure_barrier(&element) {
                    return Err(at.error(format!("allocator lists cannot store `{element}`: borrowed or hidden task/channel ownership is unsupported")));
                }
                let list = Ty::List(Arc::new(element));
                (
                    code::ALLOCATOR_LIST,
                    vec![handle, Ty::I64],
                    Some(crate::sum::result(list.clone(), crate::sum::alloc_error())),
                    Some(list),
                )
            }
            _ => unreachable!("checked allocator intrinsic"),
        };
        if descriptor.is_none() && !types.is_empty() {
            return Err(at.error("allocator handle operations take no type arguments"));
        }
        if args.len() != inputs.len() {
            return Err(at.error(format!("`{name}` requires {} arguments", inputs.len())));
        }
        for (arg, expected) in args.iter().zip(&inputs) {
            let actual = self.value(arg, ops)?;
            self.same(Some(actual), Some(expected.clone()), &arg.at)?;
        }
        ops.push(Op::Thread(crate::threading::ThreadOp::AllocatorUse));
        ops.push(Op::ForeignCall {
            declaration: crate::foreign::Declaration {
                target: crate::foreign::Target::Intrinsic(Arc::new(
                    crate::foreign::Intrinsic::Allocator { opcode, descriptor },
                )),
                arguments: vec![crate::foreign::Argument::Direct; inputs.len()],
                thread_safe: true,
                host_callback: false,
            },
            sig: Arc::new(FnSig {
                inputs: inputs
                    .into_iter()
                    .enumerate()
                    .map(|(i, t)| (format!("arg{i}"), t))
                    .collect(),
                outputs: output.iter().cloned().collect(),
            }),
        });
        Ok(output)
    }
}
