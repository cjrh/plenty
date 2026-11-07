//! Caller-side native stack checks for typed serialization call segments.
use super::*;

#[derive(Clone)]
pub(super) struct Guard {
    error: Ty,
    adapter: Ty,
    argument: usize,
    mutable: bool,
    decoding: bool,
    result: crate::nominal::Nominal<crate::sum::EnumType>,
}

pub(super) fn identify(
    template: &Function,
    concrete: &Function,
    actual: &[Argument],
    replacements: &generics::Substitution,
    aliases: &TypeAliases,
) -> Result<Option<Guard>> {
    if matches!(
        template.name.as_str(),
        "std.serial.read_field" | "std.serial.require_field"
    ) || template.name.starts_with("std.serial.decode_stack_error")
        || template.name.starts_with("std.serial.encode_stack_error")
    {
        return Ok(None);
    }
    let candidates: Vec<_> = template
        .type_params
        .iter()
        .enumerate()
        .filter_map(|(i, (_, bound))| {
            let bound = bound.as_ref()?;
            let decoding = match bound.name.as_deref()? {
                "std.serial.Decoder" => true,
                "std.serial.Encoder" => false,
                _ => return None,
            };
            (bound.args.len() == 1).then_some((i, bound, decoding))
        })
        .collect();
    if candidates.is_empty() {
        return Ok(None);
    }
    let Some(Ty::Enum(result)) = concrete.output.resolve(aliases)? else {
        return Ok(None);
    };
    if !result.is_result() {
        return Ok(None);
    }
    let definition = result.get();
    let Some((declaration, arguments)) =
        aliases.data.origin_class(&definition.variants[1].fields[0])
    else {
        return Ok(None);
    };
    if declaration.name != "std.serial.SerialError" || arguments.len() != 1 {
        return Ok(None);
    }
    let Some(output_error) = arguments[0].as_type() else {
        return Ok(None);
    };
    let mut references = Vec::with_capacity(concrete.inputs.len());
    for (_, written) in &concrete.inputs {
        let Some(Ty::Ref(inner, mutable)) = written.resolve(aliases)? else {
            return Ok(None);
        };
        references.push((inner, mutable));
    }
    for (index, bound, decoding) in candidates {
        let Some(adapter) = actual[index].as_type() else {
            continue;
        };
        let mut error = bound.args[0].clone();
        generics::substitute(&mut error, replacements)?;
        let error = error.resolve(aliases)?.unwrap_or(Ty::Unit);
        if error != *output_error {
            continue;
        }
        // Distinguish the adapter parameter from another input whose concrete
        // type happens to equal it (for example encode[T, E, T]).
        let matching = |i: usize| *references[i].0 == *adapter;
        let parameter = &template.type_params[index].0;
        let declared = (0..references.len()).find(|&i| {
            matching(i)
                && template.inputs[i]
                    .1
                    .args
                    .first()
                    .and_then(|t| t.name.as_ref())
                    == Some(parameter)
        });
        if let Some(argument) = declared.or_else(|| (0..references.len()).find(|&i| matching(i))) {
            return Ok(Some(Guard {
                error,
                adapter: adapter.clone(),
                argument,
                mutable: references[argument].1,
                decoding,
                result: result.clone(),
            }));
        }
    }
    Ok(None)
}

impl Lower<'_> {
    /// Preserve the existing forwarding-call cleanup rule across a compiler
    /// guard. The readiness check touches no borrowed program storage; both
    /// call branches still forward only references rooted in caller parameters.
    pub(super) fn finish_serial_guard_tail(&mut self, start: usize, ops: &mut Vec<Op>) -> bool {
        let Some(index) = ops
            .iter()
            .rposition(|op| !matches!(op, Op::Loan(_) | Op::UseLoan(_) | Op::EndLoan(_)))
        else {
            return false;
        };
        let Some(Op::If { then, elifs, .. }) = ops.get(index) else {
            return false;
        };
        if !elifs.is_empty()
            || !matches!(then.as_ref(), [.., Op::ForwardsReferences, Op::Call(_)])
            || index == 0
            || !matches!(
                ops[index - 1],
                Op::Collection(CollectionOp::SerialStackReady { .. })
            )
        {
            return false;
        }
        let mut cleanup = ops.split_off(index + 1);
        self.finish_temporaries(start, &mut cleanup);
        let branch = ops.pop().unwrap();
        let check = ops.pop().unwrap();
        ops.extend(cleanup);
        ops.extend([check, branch]);
        true
    }

    /// Arguments have already been evaluated exactly once. Both branches consume
    /// the same reference operands; only the adapter reaches the failure helper.
    pub(super) fn guarded_serial_call(
        &mut self,
        callee: &str,
        inputs: &[(String, Ty)],
        loans: &[usize],
        mut call: Vec<Op>,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let guard = self.generics.serial_stack[callee].clone();
        let helper = format!(
            "std.serial.{}_stack_error_{}",
            if guard.decoding { "decode" } else { "encode" },
            if guard.mutable { "mut" } else { "ref" }
        );
        let failure = self.specialize(
            &helper,
            vec![Argument::Type(guard.error), Argument::Type(guard.adapter)],
            at,
        )?;
        let mut otherwise = Vec::with_capacity(inputs.len() * 2 + 4);
        // Keep the adapter on the operand stack. No hidden reference local is
        // introduced, so forwarding calls retain their native tail-call proof.
        otherwise.extend((guard.argument + 1..inputs.len()).map(|_| Op::Drop));
        for _ in 0..guard.argument {
            otherwise.extend([Op::Swap, Op::Drop]);
        }
        // Call provenance belongs immediately beside the real call, inside
        // its branch. Keeping it outside the condition would sever the future
        // ownership analysis's predecessor walk.
        let mut facts = Vec::new();
        while matches!(
            ops.last(),
            Some(Op::Passes(_) | Op::Forwards(Forward::Call(..)))
        ) {
            facts.push(ops.pop().unwrap());
        }
        facts.reverse();
        facts.append(&mut call);
        call = facts;
        let start = otherwise.len();
        let signature = self.sigs.get(&failure).unwrap().clone();
        let helper_loans: Vec<_> = loans.get(guard.argument).copied().into_iter().collect();
        self.record_direct_call(&failure, at, &helper_loans, &mut otherwise);
        self.passes(
            start,
            &signature.inputs,
            signature.outputs.first(),
            &mut otherwise,
        );
        self.push_call(
            Op::Call(failure.clone()),
            &signature.inputs,
            &helper_loans,
            at,
            &mut otherwise,
        )?;
        otherwise.push(Op::Enum(crate::sum::EnumOp::New(guard.result, 1)));
        ops.push(Op::Collection(CollectionOp::SerialStackReady {
            callee: callee.into(),
            failure,
        }));
        ops.push(Op::If {
            then: call.into(),
            elifs: Arc::from([]),
            otherwise: otherwise.into(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_codec_guard_preserves_forwarding_tail_call() {
        let source = r#"
import std.json
@serial
class Node[T]:
    value: T
    next: Option[Box[Node[T]]]
def main() -> ():
    drop(std.json.load[Node[i64]](b"{}", &std.json.codec_options()))
"#;
        let mut heap = Heap::default();
        let program = super::super::compile(source, &mut heap).unwrap();
        let mut forwarding = false;
        crate::threading::walk(&program.ops, &mut |op| {
            if let Op::If { then, .. } = op
                && matches!(then.as_ref(), [.., Op::ForwardsReferences, Op::TailCall(_)])
            {
                forwarding = true;
            }
        });
        assert!(
            forwarding,
            "recursive codec forwarding remains a native tail call"
        );
    }
}
