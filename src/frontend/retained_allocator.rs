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
        let (opcode, mut inputs, output, descriptor) = match name {
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
            "__allocator_copy" => {
                let [element] = types else {
                    return Err(at.error("allocator copy requires one element type"));
                };
                let element = element.resolve(self.aliases)?.unwrap_or(Ty::Unit);
                // The input is borrowed, but T must not keep a loan. This includes
                // view classes and loans carried by Option/Result; other aggregate
                // storage (including array elements at length zero) forbids loans.
                if element.carries_loan() {
                    return Err(at.error(format!(
                        "Allocator.copy result cannot carry a loan: `{element}`"
                    )));
                }
                Self::check_explicit_copy(&element, at)?;
                (
                    code::ALLOCATOR_COPY,
                    vec![handle, Ty::Ref(Arc::new(element.clone()), false)],
                    Some(crate::sum::result(
                        element.clone(),
                        crate::sum::alloc_error(),
                    )),
                    Some(element),
                )
            }
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
            "__allocator_dict_set" => {
                let [collection] = types else {
                    return Err(at.error("allocator requires a dict or set type"));
                };
                // Resolve the complete builtin type so ordinary key admission and
                // heap storage checks apply before crossing the native boundary.
                let Some(collection @ (Ty::Dict(..) | Ty::Set(_))) =
                    collection.resolve(self.aliases)?
                else {
                    return Err(at.error("allocator requires a dict or set type"));
                };
                if crate::boxed::erasure_barrier(&collection) {
                    return Err(at.error(format!("allocator cannot construct `{collection}`: hidden task/channel ownership is unsupported")));
                }
                (
                    code::ALLOCATOR_DICT_SET,
                    vec![handle, Ty::I64],
                    Some(crate::sum::result(
                        collection.clone(),
                        crate::sum::alloc_error(),
                    )),
                    Some(collection),
                )
            }
            "__allocator_text_replace" => {
                let view = Ty::View(Arc::new(Ty::Text), false);
                (
                    code::ALLOCATOR_TEXT_REPLACE,
                    vec![handle, view.clone(), view.clone(), view],
                    Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())),
                    None,
                )
            }
            "__allocator_text_split" | "__allocator_text_split_whitespace" => {
                let view = Ty::View(Arc::new(Ty::Text), false);
                let list = Ty::List(Arc::new(Ty::Str));
                let (opcode, inputs) = if name == "__allocator_text_split" {
                    (code::ALLOCATOR_TEXT_SPLIT, vec![handle, view.clone(), view])
                } else {
                    (code::ALLOCATOR_TEXT_SPLIT_WHITESPACE, vec![handle, view])
                };
                (
                    opcode,
                    inputs,
                    Some(crate::sum::result(list.clone(), crate::sum::alloc_error())),
                    Some(list),
                )
            }
            "__allocator_text_splitlines" => {
                let list = Ty::List(Arc::new(Ty::Str));
                (
                    code::ALLOCATOR_TEXT_SPLITLINES,
                    vec![handle, Ty::View(Arc::new(Ty::Text), false), Ty::Bool],
                    Some(crate::sum::result(list.clone(), crate::sum::alloc_error())),
                    Some(list),
                )
            }
            "__allocator_text_quote" => (
                code::ALLOCATOR_TEXT_QUOTE,
                vec![handle, Ty::View(Arc::new(Ty::Text), false)],
                Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())),
                None,
            ),
            "__allocator_text_case" => (
                code::ALLOCATOR_TEXT_CASE,
                vec![handle, Ty::View(Arc::new(Ty::Text), false), Ty::Bool],
                Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())),
                None,
            ),
            "__allocator_text_strip" | "__allocator_text_repeat" => (
                if name == "__allocator_text_strip" {
                    code::ALLOCATOR_TEXT_STRIP
                } else {
                    code::ALLOCATOR_TEXT_REPEAT
                },
                vec![handle, Ty::View(Arc::new(Ty::Text), false), Ty::I64],
                Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())),
                None,
            ),
            "__allocator_text_finish" => (
                code::ALLOCATOR_TEXT_FINISH,
                vec![handle, crate::bytes::byte_view(), Ty::Bool],
                Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())),
                None,
            ),
            "__allocator_text_join" => (
                code::ALLOCATOR_TEXT_JOIN,
                vec![
                    handle,
                    Ty::View(Arc::new(Ty::Text), false),
                    Ty::View(Arc::new(Ty::Str), false),
                ],
                Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())),
                Some(Ty::Str),
            ),
            "__allocator_text_concat" => {
                let view = Ty::View(Arc::new(Ty::Text), false);
                (
                    code::ALLOCATOR_TEXT_CONCAT,
                    vec![handle, view.clone(), view],
                    Some(crate::sum::result(Ty::Str, crate::sum::alloc_error())),
                    None,
                )
            }
            "__allocator_bytes_slice" | "__allocator_text_slice" => {
                let (opcode, source, result) = if name == "__allocator_text_slice" {
                    (
                        code::ALLOCATOR_TEXT_SLICE,
                        Ty::View(Arc::new(Ty::Text), false),
                        Ty::Str,
                    )
                } else {
                    (
                        code::ALLOCATOR_BYTES_SLICE,
                        crate::bytes::byte_view(),
                        Ty::ByteStorage,
                    )
                };
                let bound = crate::sum::option(Ty::I64);
                (
                    opcode,
                    vec![handle, source, bound.clone(), bound, Ty::I64],
                    Some(crate::sum::result(result, crate::sum::alloc_error())),
                    None,
                )
            }
            "__allocator_clist_slice" | "__allocator_list_slice" => {
                let [element] = types else {
                    return Err(at.error("allocator slice requires one element type"));
                };
                let element = element.resolve(self.aliases)?.unwrap_or(Ty::Unit);
                if element.carries_loan() {
                    return Err(at.error(format!(
                        "Allocator.slice result cannot carry a loan: `{element}`"
                    )));
                }
                let (opcode, list) = if name == "__allocator_clist_slice" {
                    // Access to every field was checked at the public caller.
                    clists::check_row(&element, at)?;
                    (code::ALLOCATOR_CLIST_SLICE, Ty::Clist(Arc::new(element)))
                } else {
                    let list = Ty::List(Arc::new(element.clone()));
                    self.check_slice_element(&list, &element, at)?;
                    (code::ALLOCATOR_LIST_SLICE, list)
                };
                let bound = crate::sum::option(Ty::I64);
                (
                    opcode,
                    vec![
                        handle,
                        Ty::Ref(Arc::new(list.clone()), false),
                        bound.clone(),
                        bound,
                        Ty::I64,
                    ],
                    Some(crate::sum::result(list.clone(), crate::sum::alloc_error())),
                    Some(list),
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
            _ => {
                let opcode = allocator_storage::set_result_opcode(name)
                    .expect("checked allocator intrinsic");
                let [set] = types else {
                    return Err(at.error("allocator set result requires a set type"));
                };
                let Some(set @ Ty::Set(_)) = set.resolve(self.aliases)? else {
                    return Err(at.error("allocator set result requires a set type"));
                };
                if crate::boxed::erasure_barrier(&set) {
                    return Err(at.error(format!("allocator cannot construct `{set}`: hidden task/channel ownership is unsupported")));
                }
                let key = set.element().unwrap();
                let conversion = opcode == code::ALLOCATOR_KEY_VIEW_TO_SET;
                if !key.copy_facts().copyable {
                    let operation = if conversion { "to_set" } else { "set algebra" };
                    return Err(at.error(format!("{operation} requires copying keys, but {key} contains custom cleanup (__del__)")));
                }
                let inputs = if conversion {
                    vec![handle, Ty::KeyView(Arc::new(key))]
                } else {
                    let source = Ty::Ref(Arc::new(set.clone()), false);
                    vec![handle, source.clone(), source]
                };
                (
                    opcode,
                    inputs,
                    Some(crate::sum::result(set.clone(), crate::sum::alloc_error())),
                    Some(set),
                )
            }
        };
        if descriptor.is_none() && !types.is_empty() {
            return Err(at.error("allocator handle operations take no type arguments"));
        }
        if args.len() != inputs.len() {
            return Err(at.error(format!("`{name}` requires {} arguments", inputs.len())));
        }
        for (index, (arg, expected)) in args.iter().zip(&mut inputs).enumerate() {
            let actual = self.value(arg, ops)?;
            if matches!(
                opcode,
                code::ALLOCATOR_SET_UNION..=code::ALLOCATOR_SET_SYMMETRIC_DIFFERENCE
            ) && matches!(index, 1 | 2)
                && matches!(actual, Ty::KeyView(_))
                && allocator_storage::algebra_key(&actual)
                    == allocator_storage::algebra_key(expected)
            {
                *expected = actual;
            } else {
                self.same(Some(actual), Some(expected.clone()), &arg.at)?;
            }
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
        if opcode == code::ALLOCATOR_BYTES_SLICE {
            let Ty::Class(class) = self.bytes_class(at)? else {
                unreachable!("verified Bytes class")
            };
            return self
                .wrap_bytes_slice(&class, output.unwrap(), at, ops)
                .map(Some);
        }
        Ok(output)
    }
}
