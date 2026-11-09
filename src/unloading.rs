//! Explicit, conservative lifecycle certification for shared libraries.
//!
//! This is independent of worker eligibility: memory-only actor heaps are
//! thread-bound, while a TLS cache can be thread-safe yet prevent dlclose.
//! New operation families are denied until their lifetime contract is audited.
use crate::hash::{HashMap, HashSet};
use crate::op::{Op, Ty};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) const HEADER_CONTRACT: &str = "/* Plenty unloadable-library profile v1 (Linux GNU x86_64).\n * Before final dlclose: finish every call, destroy every Plenty owner/handle,\n * stop callbacks, and discard all pointers into this library. Host threads\n * may remain alive. Foreign bindings must leave no callback, TLS destructor,\n * background task, or process registration referring to this library.\n * Thread/aliasing contracts still apply; this is not a real-time guarantee.\n * Other loader references or NODELETE can independently retain the mapping. */\n";

pub(crate) fn check(ops: &[Op], exports: &[crate::exports::Export]) -> Result<()> {
    let mut functions = HashMap::default();
    crate::threading::walk(ops, &mut |op| {
        if let Op::DefineFn(name, function) = op {
            functions.insert(name.as_str(), function.as_ref());
        }
    });
    let mut pending: Vec<_> = exports.iter().map(|e| e.function.clone()).collect();
    let mut seen = HashSet::default();
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let function = functions
            .get(name.as_str())
            .ok_or_else(|| format!("unloadable library: cannot inspect function `{name}`"))?;
        if function.generator.is_some() {
            return Err(
                format!("unloadable library: generator `{name}` is not supported yet").into(),
            );
        }
        let mut types: Vec<_> = function
            .sig
            .inputs
            .iter()
            .map(|(_, t)| t.clone())
            .chain(function.sig.outputs.iter().cloned())
            .chain(function.locals.iter().cloned())
            .collect();
        let mut denied = None;
        crate::threading::walk(&function.body, &mut |op| {
            crate::threading::function_references(op, &mut |callee| {
                pending.push(callee.to_owned())
            });
            if !allowed(op) && denied.is_none() {
                denied = Some(format!("{op:?}"));
            }
            match op {
                Op::Collection(op) => {
                    let (inputs, output) = op.signature();
                    types.extend(inputs);
                    types.push(output);
                }
                Op::Class(op) => {
                    if let Some((inputs, output)) = op.signature() {
                        types.extend(inputs);
                        types.push(output);
                    }
                }
                Op::Enum(op) => {
                    if let Some((inputs, output)) = op.signature() {
                        types.extend(inputs);
                        types.push(output);
                    }
                }
                Op::Box(op) => {
                    let (inputs, output) = op.signature();
                    types.extend(inputs);
                    types.push(output);
                }
                Op::ForeignCall { sig, .. } => {
                    types.extend(sig.inputs.iter().map(|(_, t)| t.clone()));
                    types.extend(sig.outputs.iter().cloned());
                }
                Op::ReadRef(t) | Op::WriteRef(t) | Op::FillRef(t) | Op::ReplaceRef(t) => {
                    types.push(t.clone())
                }
                _ => {}
            }
        });
        if let Some(operation) = denied {
            return Err(format!("unloadable library: `{name}` reaches unsupported operation {operation}; its runtime lifetime has not been certified for unloading").into());
        }
        check_types(types, &name, &mut pending)?;
    }
    Ok(())
}

#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
fn check_types(mut pending: Vec<Ty>, function: &str, calls: &mut Vec<String>) -> Result<()> {
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        if !seen.insert(ty.clone()) {
            continue;
        }
        match &ty {
            Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
            | Ty::F32 | Ty::F64 | Ty::Bool | Ty::Unit | Ty::Str | Ty::ByteStr | Ty::Text
            | Ty::ByteStorage
            | Ty::ForeignPtr(..) => {}
            Ty::List(t) | Ty::Clist(t) | Ty::Set(t) | Ty::KeyView(t) | Ty::DictView(t, _)
            | Ty::Ref(t, _) | Ty::View(t, _) | Ty::Box(t) | Ty::Shared(t) | Ty::Range(t)
            | Ty::Iter(t, _) => pending.push((**t).clone()),
            // Thin callable values have no environment or destructor. Their
            // only producers are FunctionAddress operations, whose targets
            // the reachable-function walk visits. C exports cannot accept a
            // Plenty callable and raw foreign addresses are distinct types.
            Ty::Callable(signature) => {
                pending.extend(signature.inputs.iter().cloned());
                pending.extend(signature.output.iter().cloned());
            }
            Ty::Dict(k, v) => pending.extend([(**k).clone(), (**v).clone()]),
            Ty::Class(t) => {
                pending.extend(t.get().fields.iter().map(|(_, t)| t.clone()));
                // Include implicit generated handle destruction, not just a
                // destructor armed by an explicitly visible constructor.
                calls.extend(t.get().destructor.iter().cloned());
            }
            Ty::Enum(t) => pending.extend(t.get().variants.iter().flat_map(|v| v.fields.iter().cloned())),
            _ => return Err(format!("unloadable library: `{function}` uses unsupported type `{ty}`; its runtime lifetime has not been certified for unloading").into()),
        }
    }
    Ok(())
}

fn allowed(op: &Op) -> bool {
    match op {
        Op::Collection(op) => collection_allowed(op),
        // Trusted native interfaces already assert pointer and ownership
        // contracts. This profile additionally requires unload-safe teardown;
        // the compiler cannot inspect either named or indirect foreign code.
        Op::ForeignCall { .. } | Op::ForeignNull(_) => true,
        Op::Class(_)
        | Op::Enum(_)
        | Op::Box(_)
        | Op::Numeric(_)
        | Op::CallIndirect(_)
        | Op::TailCallIndirect(_)
        | Op::FunctionAddress(..)
        | Op::Call(_)
        | Op::TailCall(_)
        | Op::BorrowLocal(..)
        | Op::ReadRef(_)
        | Op::Reborrow(_)
        | Op::WriteRef(_)
        | Op::FillRef(_)
        | Op::ReplaceRef(_)
        | Op::Loan(_)
        | Op::UseLoan(_)
        | Op::EndLoan(_)
        | Op::Access(..)
        | Op::Site(_)
        | Op::CallerSite(_)
        | Op::Returning(..)
        | Op::Leaving
        | Op::Building(_)
        | Op::Passes(_)
        | Op::Forwards(_)
        | Op::Tokens(_)
        | Op::MoveLocal(_)
        | Op::DropLocal(_)
        | Op::EmptyLocal(_)
        | Op::FieldMove(_)
        | Op::Split(..)
        | Op::SplitClass(_)
        | Op::Unreachable
        | Op::Try { .. }
        | Op::CheckedBlock { .. }
        | Op::CheckedFunction { .. }
        | Op::CheckedArith { .. }
        | Op::Assumed(_)
        | Op::Loop { .. }
        | Op::If { .. }
        | Op::PushInt(_)
        | Op::PushFloat { .. }
        | Op::PushUnit
        | Op::PipelineToken(..)
        | Op::FloatNeg
        | Op::PushStr(_)
        | Op::PushBytes(_)
        | Op::PushBool(_)
        | Op::Add
        | Op::Sub
        | Op::Mul
        | Op::Div
        | Op::FloorDiv(_)
        | Op::Modulo(_)
        | Op::Integer(_)
        | Op::Eq
        | Op::Lt
        | Op::Gt
        | Op::Not
        | Op::Ne
        | Op::Le
        | Op::Ge
        | Op::Drop
        | Op::Dup
        | Op::Swap
        | Op::ForwardsReferences
        | Op::CallerReferent(_)
        | Op::Return
        | Op::Break
        | Op::Continue
        | Op::LoadLocal(_)
        | Op::StoreLocal(_)
        | Op::Match(_)
        | Op::Cast(_)
        | Op::CheckedCast(_)
        | Op::TryCast(_)
        | Op::RangeLength(_)
        | Op::RangeParts(_)
        | Op::Contains => true,
        _ => false,
    }
}

// Explicitly list audited operations. Effect::Compute alone is insufficient:
// actor heaps and third-party engines can retain memory or registrations.
fn collection_allowed(op: &crate::collection::CollectionOp) -> bool {
    use crate::collection::CollectionOp::*;
    match op {
        Text(text) => match text {
            crate::text::TextOp::AsciiEqualFolded | crate::text::TextOp::FindIn => true,
        },
        TextView(view) => {
            use crate::text::TextViewOp::*;
            match view {
                Apply(original) => collection_allowed(original),
                OfStr
                | Subview
                | Len
                | Bytes
                | FindIn
                | AsciiEqualFolded
                | Copy
                | Compare(..)
                | Scalar
                | ScalarBefore
                | Index
                | Search { .. } => true,
            }
        }
        Bytes(bytes) => {
            use crate::bytes::BytesOp::*;
            match bytes {
                New | Len | Capacity | Get | Set | Reserve | Resize | Push | Append | Overwrite
                | FromList | ToList | FromText | ToText | WriteFloat | WriteFloatSpec | Find
                | FindIn | FindBytes | Count | Matches | MatchesText | Run | Fill | Item
                | SetItem | ValueItem | CopyWithin | Slice | SliceLiteral | RFindBytes => true,
            }
        }
        // Unbuffered stderr: a call-scoped lock and restored signal mask,
        // with no TLS destructor, buffered output, or exit registration.
        // std.fmt's memory formatter shares this conditional diagnostic path.
        ReportWrite
        | Trap
        | EncodeUtf8
        | DecodeUtf8
        | ParseNumber(_)
        | FormatScalar(_)
        | FormatValue(_)
        | ListUnpack(_)
        | ElementRef(..)
        | TryCopy(_)
        | Next(_)
        | TryNew(_)
        | Clist(_)
        | TryReserve(_)
        | TryInsert(_)
        | ListTryInsert(_)
        | TryExtend(_)
        | DictTryUpdate(_)
        | SetTryUpdate(_)
        | SetIsSubset(_)
        | KeyView(_)
        | DictView(..)
        | ListRetag(..)
        | KeyViewToSet(_)
        | KeySetRelation(..)
        | KeySetAlgebra(..)
        | SetIsSuperset(_)
        | SetIsDisjoint(_)
        | SetTryUnion(_)
        | SetTryIntersection(_)
        | SetTryDifference(_)
        | SetTrySymmetricDifference(_)
        | KeySetFilter(..)
        | SetIntersectionUpdate(_)
        | SetDifferenceUpdate(_)
        | Get(_)
        | ListGet(_)
        | ListCount(_)
        | ListFind(_)
        | ListRFind(_)
        | DictGet(_)
        | DictPop(_)
        | DictGetRef(..)
        | ListGetRef(..)
        | DictGetOrInsert(_)
        | ListPop(_)
        | SetDiscard(_)
        | ListReverse(_)
        | ListSwap(_)
        | ListSort(_)
        | OrderCompare(_)
        | Clear(_)
        | Len(_)
        | ListLenRef(_)
        | QueryRef(_)
        | ListScalarGetRef(_)
        | ListScalarGetView(_)
        | ListScalarSetRef(_)
        | ListScalarSetView(_)
        | BytesGetView(_)
        | BytesSetView(_)
        | ElementRefView(..)
        | ListScalarSwapRef(_)
        | ListRowsRef(_)
        | ListRowBytesRef(_)
        | ListRowRef(..)
        | ListRowTake(_)
        | ViewNew(..)
        | ViewLen(_)
        | ViewRow(..)
        | ViewItem(..)
        | ViewGetRef(..)
        | ViewEqual(_)
        | ViewSwap(_)
        | ViewSort(_)
        | ListSortBy(_)
        | IterGet(_)
        | HashIterFirst(_)
        | HashIterNext(_)
        | HashIterGet(_)
        | HashIterKeyRef(_)
        | IterTake(_)
        | Contains(_)
        | Range(_)
        | TryKeys(_)
        | TryValues(_)
        | ListTrySlice(_)
        | ListSlice(_)
        | TextSlice
        | TextByteLen
        | TextIndex
        | TextTryConcat
        | TextTryJoin(_)
        | TextTrySplit
        | TextTrySplitLines
        | TextGet
        | TextTrySlice
        | TextTryReplace
        | TextStartsWith
        | TextEndsWith
        | TextFind
        | TextRFind
        | TextCount
        | TextTryStrip
        | TextTryLStrip
        | TextTryRStrip
        | TextTryRepeat
        | TextTryRemovePrefix
        | TextTryRemoveSuffix
        | TextIsAscii
        | TextIsSpace
        | TextIsDigit
        | TextIsAlpha
        | TextIsAlnum
        | TextTryLower
        | TextTryUpper
        | TextTrySplitWhitespace
        | TextOrd
        | TextChr
        | TextCompare => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::CollectionOp;

    #[test]
    fn compute_effect_is_not_a_lifecycle_certificate() {
        // B28: these need their own cleanup contracts before admission.
        assert_eq!(
            CollectionOp::ActorHeap.effect(),
            crate::effects::Effect::Compute
        );
        for operation in [
            CollectionOp::ActorHeap,
            CollectionOp::Input,
            CollectionOp::OpenFile,
            CollectionOp::Process(crate::process::ProcessOp::SignalListen),
            CollectionOp::Net(crate::net::NetOp::Resolve),
        ] {
            assert!(!collection_allowed(&operation), "{operation:?}");
        }
        assert!(collection_allowed(&CollectionOp::TryNew(Ty::List(
            std::sync::Arc::new(Ty::I64)
        ))));
    }
}
