//! Checked native tail transfers. Lifetime checking precedes this pass: the ABI
//! describes representations, not whether a reference outlives the current frame.
use crate::op::{CompiledFn, FnSig, Op, Ty};
use cranelift_codegen::ir::{types, Type};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResultArea {
    pub bytes: usize,
    pub alignment: usize,
    pub offsets: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub inputs: Vec<Type>,
    pub outputs: Vec<Type>,
    pub result_area: Option<ResultArea>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TailCallPlan {
    pub layout: Layout,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TailCallRejection {
    OwnedInlineArgument { index: usize, ty: Ty },
    IncompatibleResults,
}

impl fmt::Display for TailCallRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnedInlineArgument { index, ty } => write!(
                f,
                "argument {} of type `{ty}` needs owned inline storage in the departing frame",
                index + 1
            ),
            Self::IncompatibleResults => write!(
                f,
                "caller and callee have incompatible result storage or native result layouts"
            ),
        }
    }
}

pub(crate) fn native_type(ty: &Ty) -> Type {
    if ty.wide() || matches!(ty, Ty::Ref(..)) {
        return types::I128;
    }
    match ty {
        Ty::I8 | Ty::U8 | Ty::Bool => types::I8,
        Ty::I16 | Ty::U16 => types::I16,
        Ty::I32 | Ty::U32 => types::I32,
        Ty::F32 => types::F32,
        Ty::F64 => types::F64,
        _ => types::I64,
    }
}

pub(crate) fn layout(sig: &FnSig) -> Layout {
    let mut bytes = 0;
    let offsets = sig
        .outputs
        .iter()
        .map(|ty| {
            let offset = bytes;
            bytes += ty.inline_bytes();
            offset
        })
        .collect();
    let result_area = sig
        .outputs
        .iter()
        .any(Ty::has_inline_storage)
        .then_some(ResultArea {
            bytes,
            alignment: 16,
            offsets,
        });
    let mut inputs: Vec<_> = sig.inputs.iter().map(|(_, ty)| native_type(ty)).collect();
    if result_area.is_some() {
        inputs.push(types::I64);
    }
    Layout {
        inputs,
        outputs: sig.outputs.iter().map(native_type).collect(),
        result_area,
    }
}

pub(crate) fn classify(caller: &FnSig, callee: &FnSig) -> Result<TailCallPlan, TailCallRejection> {
    let layout = layout(callee);
    let caller_layout = self::layout(caller);
    // Exact source results also guarantee identical nested relocation metadata.
    if caller.outputs != callee.outputs
        || caller_layout.outputs != layout.outputs
        || caller_layout.result_area != layout.result_area
    {
        return Err(TailCallRejection::IncompatibleResults);
    }
    for (index, (_, ty)) in callee.inputs.iter().enumerate() {
        if ty.has_inline_storage() {
            return Err(TailCallRejection::OwnedInlineArgument {
                index,
                ty: ty.clone(),
            });
        }
    }
    Ok(TailCallPlan { layout })
}

/// Make ordinary fallbacks explicit before native lowering. This runs after the
/// original operation/loan check, so cleanup insertion cannot hide a loan use.
pub(crate) fn prepare(ops: &[Op]) -> Vec<Op> {
    fn collect(ops: &[Op], sigs: &mut HashMap<String, Rc<FnSig>>) {
        for op in ops {
            match op {
                Op::DefineFn(name, f) => {
                    sigs.insert(name.clone(), f.sig.clone());
                    collect(&f.body, sigs);
                }
                Op::Match(arms) => {
                    for arm in arms.iter() {
                        collect(&arm.body, sigs);
                    }
                }
                Op::Loop { condition, body } => {
                    collect(condition, sigs);
                    collect(body, sigs);
                }
                _ => {}
            }
        }
    }
    fn body(ops: &[Op], caller: Option<&CompiledFn>, sigs: &HashMap<String, Rc<FnSig>>) -> Vec<Op> {
        let mut result = Vec::new();
        for op in ops {
            let mut op = op.clone();
            match &mut op {
                Op::DefineFn(_, f) => {
                    f.body = body(&f.body, Some(f), sigs).into();
                }
                Op::Match(arms) => {
                    for arm in Rc::make_mut(arms) {
                        arm.body = body(&arm.body, caller, sigs).into();
                    }
                }
                Op::Loop {
                    condition,
                    body: nested,
                } => {
                    *condition = body(condition, caller, sigs).into();
                    *nested = body(nested, caller, sigs).into();
                }
                _ => {}
            }
            let callee = match &op {
                Op::TailCall(name) => sigs.get(name).cloned(),
                Op::TailCallIndirect(sig) => Some(Rc::new(sig.function())),
                _ => None,
            };
            if let (Some(caller), Some(callee)) = (caller, callee) {
                // ABI compatibility alone cannot authorize removing a frame
                // that an argument might borrow. Only the frontend's fact
                // does: without it, a reference may point at a caller local.
                let forwards = matches!(result.last(), Some(Op::ForwardsReferences));
                let borrowed =
                    !forwards && callee.inputs.iter().any(|(_, ty)| ty.contains_reference());
                if classify(&caller.sig, &callee).is_err() || borrowed {
                    if !borrowed {
                        // Keep the fact next to its call, after the cleanup.
                        if forwards {
                            result.pop();
                        }
                        let count = caller.sig.inputs.len() + caller.locals.len();
                        result.extend((0..count).rev().map(|i| Op::DropLocal(i as u8)));
                        if forwards {
                            result.push(Op::ForwardsReferences);
                        }
                    }
                    result.push(match op {
                        Op::TailCall(name) => Op::Call(name),
                        Op::TailCallIndirect(sig) => Op::CallIndirect(sig),
                        _ => unreachable!(),
                    });
                    result.push(Op::Return);
                    continue;
                }
            }
            result.push(op);
        }
        result
    }
    let mut sigs = HashMap::new();
    collect(ops, &mut sigs);
    body(ops, None, &sigs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(inputs: Vec<Ty>, outputs: Vec<Ty>) -> FnSig {
        FnSig {
            inputs: inputs
                .into_iter()
                .enumerate()
                .map(|(i, ty)| (i.to_string(), ty))
                .collect(),
            outputs,
        }
    }

    #[test]
    fn result_plan_records_capacity_alignment_offsets_and_every_native_component() {
        let range = Ty::Range(Rc::new(Ty::I64));
        let caller = sig(vec![], vec![Ty::I8, range.clone(), Ty::F64, range.clone()]);
        let callee = sig(vec![Ty::I64; 20], caller.outputs.clone());
        let plan = classify(&caller, &callee).unwrap();
        assert_eq!(
            plan.layout.outputs,
            vec![types::I8, types::I64, types::F64, types::I64]
        );
        assert_eq!(plan.layout.inputs.len(), 21);
        assert_eq!(
            plan.layout.result_area,
            Some(ResultArea {
                bytes: 64,
                alignment: 16,
                offsets: vec![0, 0, 32, 32]
            })
        );
        assert_eq!(
            classify(&sig(vec![], vec![Ty::I64]), &callee),
            Err(TailCallRejection::IncompatibleResults)
        );
        assert_eq!(
            classify(&sig(vec![], vec![range]), &callee),
            Err(TailCallRejection::IncompatibleResults)
        );
    }

    #[test]
    fn external_reference_layout_is_independent_of_owned_referent_layout() {
        let range = Ty::Range(Rc::new(Ty::I64));
        let caller = sig(vec![], vec![Ty::I64]);
        let borrowed = sig(
            vec![Ty::Ref(Rc::new(range.clone()), false)],
            caller.outputs.clone(),
        );
        assert_eq!(
            classify(&caller, &borrowed).unwrap().layout.inputs,
            vec![types::I128]
        );
        let owned = sig(vec![Ty::I64, range.clone()], caller.outputs.clone());
        assert_eq!(
            classify(&caller, &owned),
            Err(TailCallRejection::OwnedInlineArgument {
                index: 1,
                ty: range
            })
        );
    }

    #[test]
    fn rejected_transfers_become_explicit_calls_and_approved_results_stay_tail_calls() {
        let source = "def consume(r: range[i64]) -> i64:\n    len(r)\ndef fallback(r: range[i64]) -> i64:\n    consume(r)\ndef result(n: i64) -> range[i64]:\n    result(n)\ndef main() -> ():\n    pass\n";
        let mut heap = crate::value::Heap::default();
        let program = crate::frontend::compile(source, &mut heap).unwrap();
        crate::op::check(&program.ops).unwrap();
        let ops = prepare(&program.ops);
        let function = |name: &str| {
            ops.iter()
                .find_map(|op| match op {
                    Op::DefineFn(n, f) if n == name => Some(f),
                    _ => None,
                })
                .unwrap()
        };
        let fallback = &function("fallback").body;
        assert!(matches!(fallback.last(), Some(Op::Return)));
        assert!(matches!(&fallback[fallback.len() - 2], Op::Call(name) if name == "consume"));
        assert!(
            matches!(function("result").body.last(), Some(Op::TailCall(name)) if name == "result")
        );
    }

    #[test]
    fn a_reference_call_stays_a_transfer_only_with_the_frontend_fact() {
        let source = "class Item:\n    value: i64\ndef read(item: &Item, n: i64) -> i64:\n    n\ndef keep(item: &Item, owned: Item) -> i64:\n    0\ndef forwards(item: &Item) -> i64:\n    read(item, 1)\ndef stages(item: &Item, owned: Item) -> i64:\n    keep(item, owned)\ndef main() -> ():\n    pass\n";
        let mut heap = crate::value::Heap::default();
        let program = crate::frontend::compile(source, &mut heap).unwrap();
        crate::op::check(&program.ops).unwrap();
        let body = |ops: &[Op], name: &str| {
            ops.iter()
                .find_map(|op| match op {
                    Op::DefineFn(n, f) if n == name => Some(f.body.to_vec()),
                    _ => None,
                })
                .unwrap()
        };
        let prepared = prepare(&program.ops);
        assert!(matches!(
            body(&prepared, "forwards")[..],
            [.., Op::ForwardsReferences, Op::TailCall(_)]
        ));
        // The owned inline argument keeps the frame. The fact still lets its
        // locals go before the call, and stays next to that call.
        assert!(matches!(
            body(&prepared, "stages")[..],
            [
                ..,
                Op::DropLocal(_),
                Op::ForwardsReferences,
                Op::Call(_),
                Op::Return
            ]
        ));
        // Without the fact the reference may point at a caller local, so the
        // locals are dropped only by the return.
        let unproven: Vec<Op> = program
            .ops
            .iter()
            .map(|op| match op {
                Op::DefineFn(name, f) => {
                    let mut f = f.clone();
                    f.body = f
                        .body
                        .iter()
                        .filter(|op| !matches!(op, Op::ForwardsReferences))
                        .cloned()
                        .collect();
                    Op::DefineFn(name.clone(), f)
                }
                op => op.clone(),
            })
            .collect();
        let forwards = body(&prepare(&unproven), "forwards");
        assert!(matches!(forwards[..], [.., Op::Call(_), Op::Return]));
        assert!(!forwards.iter().any(|op| matches!(op, Op::DropLocal(_))));
    }
}
