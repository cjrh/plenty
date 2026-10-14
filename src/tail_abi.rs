//! Checked native tail transfers. Lifetime checking precedes this pass: the ABI
//! describes representations, not whether a reference outlives the current frame.
use crate::op::{CompiledFn, ElifArm, FnSig, MatchArm, Op, Ty};
use cranelift_codegen::ir::{types, Type};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

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

/// The signature of a function by name, for the callee of a tail call.
pub(crate) type Signatures<'a> = dyn Fn(&str) -> Option<Arc<FnSig>> + 'a;

/// Make ordinary fallbacks explicit before native lowering. This runs after the
/// original operation/loan check, so cleanup insertion cannot hide a loan use.
///
/// Bodies are rewritten in place. Most need no fallback and keep their shared
/// operations: a full copy of every body was a quarter of the compiler's peak
/// memory (issue #54), and a copy of every later definition after the first
/// fallback cost time per function (issue #57).
pub(crate) fn prepare(ops: &mut [Op]) {
    let mut table: HashMap<&str, &Arc<FnSig>> = HashMap::with_capacity(ops.len());
    collect(ops, &mut table);
    let signatures = |name: &str| table.get(name).map(|sig| Arc::clone(sig));
    // Signatures borrow from `ops`, so the rewrites are applied afterwards.
    let rewrites: Vec<(usize, Op)> = ops
        .iter()
        .enumerate()
        .filter_map(|(index, op)| {
            let rewritten = match op {
                Op::DefineFn(name, f) => {
                    let mut f = f.clone();
                    f.body = prepare_function(&f, &signatures)?.into();
                    Op::DefineFn(name.clone(), f)
                }
                op => body(std::slice::from_ref(op), None, &signatures)?.pop()?,
            };
            Some((index, rewritten))
        })
        .collect();
    drop(table);
    for (index, op) in rewrites {
        ops[index] = op;
    }
}

/// The body of `f` with its fallbacks made explicit, or `None` when it needs
/// none. This is the normalization [`prepare`] applies to each definition.
pub(crate) fn prepare_function(f: &CompiledFn, signatures: &Signatures<'_>) -> Option<Vec<Op>> {
    body(&f.body, Some(f), signatures)
}

/// Signatures of every definition in `ops`, nested ones included. Keys and
/// signatures borrow from `ops`: nothing is copied per function.
fn collect<'a>(ops: &'a [Op], sigs: &mut HashMap<&'a str, &'a Arc<FnSig>>) {
    for op in ops {
        match op {
            Op::DefineFn(name, f) => {
                sigs.insert(name, &f.sig);
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
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                collect(then, sigs);
                for arm in elifs.iter() {
                    collect(&arm.condition, sigs);
                    collect(&arm.body, sigs);
                }
                collect(otherwise, sigs);
            }
            _ => {}
        }
    }
}

/// The rewritten nested body, or the shared original when nothing changed.
fn shared(
    nested: &Arc<[Op]>,
    caller: Option<&CompiledFn>,
    sigs: &Signatures<'_>,
    changed: &mut bool,
) -> Arc<[Op]> {
    match body(nested, caller, sigs) {
        Some(rewritten) => {
            *changed = true;
            rewritten.into()
        }
        None => Arc::clone(nested),
    }
}

/// `None` means that no operation in `ops` changes. The result starts as a
/// copy of the operations before the first change.
fn body(ops: &[Op], caller: Option<&CompiledFn>, sigs: &Signatures<'_>) -> Option<Vec<Op>> {
    let mut result: Option<Vec<Op>> = None;
    for (index, op) in ops.iter().enumerate() {
        let mut changed = false;
        let nested = match op {
            Op::DefineFn(name, f) => body(&f.body, Some(f), sigs).map(|rewritten| {
                let mut f = f.clone();
                f.body = rewritten.into();
                Op::DefineFn(name.clone(), f)
            }),
            Op::Match(arms) => {
                let rewritten: Arc<[MatchArm]> = arms
                    .iter()
                    .map(|arm| MatchArm {
                        pattern: arm.pattern,
                        body: shared(&arm.body, caller, sigs, &mut changed),
                    })
                    .collect();
                changed.then_some(Op::Match(rewritten))
            }
            Op::Loop {
                condition,
                body: nested,
            } => {
                let condition = shared(condition, caller, sigs, &mut changed);
                let body = shared(nested, caller, sigs, &mut changed);
                changed.then_some(Op::Loop { condition, body })
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                let then = shared(then, caller, sigs, &mut changed);
                let elifs = elifs
                    .iter()
                    .map(|arm| ElifArm {
                        condition: shared(&arm.condition, caller, sigs, &mut changed),
                        body: shared(&arm.body, caller, sigs, &mut changed),
                    })
                    .collect();
                let otherwise = shared(otherwise, caller, sigs, &mut changed);
                changed.then_some(Op::If {
                    then,
                    elifs,
                    otherwise,
                })
            }
            _ => None,
        };
        let callee = match op {
            Op::TailCall(name) => sigs(name),
            Op::TailCallIndirect(sig) => Some(Arc::new(sig.function())),
            _ => None,
        };
        if let (Some(caller), Some(callee)) = (caller, callee) {
            // ABI compatibility alone cannot authorize removing a frame
            // that an argument might borrow. Only the frontend's fact
            // does: without it, a reference may point at a caller local.
            let previous = match &result {
                Some(result) => result.last(),
                None => index.checked_sub(1).map(|previous| &ops[previous]),
            };
            let forwards = matches!(previous, Some(Op::ForwardsReferences));
            let borrowed = !forwards && callee.inputs.iter().any(|(_, ty)| ty.contains_reference());
            if classify(&caller.sig, &callee).is_err() || borrowed {
                let result = result.get_or_insert_with(|| ops[..index].to_vec());
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
                    Op::TailCall(name) => Op::Call(name.clone()),
                    Op::TailCallIndirect(sig) => Op::CallIndirect(sig.clone()),
                    _ => unreachable!(),
                });
                result.push(Op::Return);
                continue;
            }
        }
        if let Some(op) = nested {
            result.get_or_insert_with(|| ops[..index].to_vec()).push(op);
        } else if let Some(result) = &mut result {
            result.push(op.clone());
        }
    }
    result
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
        let range = Ty::Range(Arc::new(Ty::I64));
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
        let range = Ty::Range(Arc::new(Ty::I64));
        let caller = sig(vec![], vec![Ty::I64]);
        let borrowed = sig(
            vec![Ty::Ref(Arc::new(range.clone()), false)],
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
        let mut ops = program.ops;
        prepare(&mut ops);
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
        let mut prepared = program.ops.clone();
        prepare(&mut prepared);
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
        let mut unproven: Vec<Op> = program
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
        prepare(&mut unproven);
        let forwards = body(&unproven, "forwards");
        assert!(matches!(forwards[..], [.., Op::Call(_), Op::Return]));
        assert!(!forwards.iter().any(|op| matches!(op, Op::DropLocal(_))));
    }

    /// Copying every body here was a quarter of the compiler's peak memory
    /// (issue #54); copying every later definition cost time (issue #57).
    #[test]
    fn only_a_body_with_a_fallback_is_copied() {
        let source = "def consume(r: range[i64]) -> i64:\n    len(r)\ndef fallback(r: range[i64]) -> i64:\n    consume(r)\ndef result(n: i64) -> range[i64]:\n    result(n)\ndef main() -> ():\n    pass\n";
        let mut heap = crate::value::Heap::default();
        let program = crate::frontend::compile(source, &mut heap).unwrap();
        let body = |ops: &[Op], name: &str| {
            ops.iter()
                .find_map(|op| match op {
                    Op::DefineFn(n, f) if n == name => Some(f.body.clone()),
                    _ => None,
                })
                .unwrap()
        };
        let mut prepared = program.ops.clone();
        prepare(&mut prepared);
        for (name, shared) in [
            ("result", true),
            ("consume", true),
            ("fallback", false),
            ("main", true),
        ] {
            assert_eq!(
                Arc::ptr_eq(&body(&prepared, name), &body(&program.ops, name)),
                shared,
                "{name}"
            );
        }
    }
}
