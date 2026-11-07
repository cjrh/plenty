//! Native stack budgets for calls into checked serialization code.
//!
//! Checks run in the caller, before the callee's prologue reserves its frame.
//! The budget includes the longest unguarded call segment, the failure helper,
//! and recovery space. A missing frame or an unguarded cycle fails closed.
use super::*;
use crate::collection::CollectionOp;
use crate::hash::HashSet;
use std::sync::{Arc, Mutex};

pub(super) type Requests = Arc<Mutex<HashMap<(FuncId, FuncId), DataId>>>;

const RECOVERY_BYTES: u64 = 65536;

#[derive(Default)]
pub(super) struct Budgets {
    pub requests: Requests,
    protected: HashSet<FuncId>,
    calls: HashMap<FuncId, Vec<FuncId>>,
    inlined: HashSet<FuncId>,
    frames: HashMap<FuncId, Option<u64>>,
    tail_arguments: HashMap<FuncId, u64>,
}

impl Budgets {
    pub(super) fn protected_names(ops: &[Op]) -> HashSet<String> {
        let mut names = HashSet::default();
        crate::threading::walk(ops, &mut |op| {
            if let Op::Collection(CollectionOp::SerialStackReady { callee, .. }) = op {
                names.insert(callee.clone());
            }
        });
        names
    }

    pub(super) fn prepare(
        &mut self,
        bodies: &[(String, Arc<[Op]>)],
        functions: &HashMap<String, UserFn>,
        protected: &HashSet<String>,
    ) {
        if protected.is_empty() {
            return;
        }
        self.protected = protected.iter().map(|name| functions[name].id).collect();
        for (name, body) in bodies {
            let function = &functions[name];
            let mut calls = Vec::new();
            let mut arguments = function.sig.inputs.len() as u64;
            crate::threading::walk(body, &mut |op| {
                if let Op::Call(name) | Op::TailCall(name) = op
                    && let Some(target) = functions.get(name)
                {
                    calls.push(target.id);
                    arguments = arguments.max(target.sig.inputs.len() as u64);
                }
            });
            calls.sort_unstable();
            calls.dedup();
            self.calls.insert(function.id, calls);
            // Preserve a bound on widened tail-call argument areas, which the
            // pinned backend's frame-to-FP metadata deliberately excludes.
            self.tail_arguments.insert(function.id, arguments * 16);
            if function.inline.is_some() || function.inline_body.is_some() {
                self.inlined.insert(function.id);
            }
        }
    }

    pub(super) fn frame(&mut self, function: FuncId, bytes: Option<u64>) {
        if !self.protected.is_empty() {
            self.frames.insert(function, bytes);
        }
    }

    fn segment(
        &self,
        function: FuncId,
        active: &mut HashSet<FuncId>,
        memo: &mut HashMap<FuncId, Option<u64>>,
    ) -> Option<u64> {
        if let Some(value) = memo.get(&function) {
            return *value;
        }
        if !active.insert(function) {
            return None;
        }
        let result = (|| {
            let frame = match self.frames.get(&function) {
                Some(frame) => (*frame)?,
                // Inlined functions' storage is already in their callers'
                // measured frames, but their non-inlined callees still count.
                None if self.inlined.contains(&function) => 0,
                None => return None,
            };
            let mut children = 0;
            for callee in self.calls.get(&function)? {
                if !self.protected.contains(callee) {
                    children = children.max(self.segment(*callee, active, memo)?);
                }
            }
            frame
                .checked_add(*self.tail_arguments.get(&function)?)?
                .checked_add(children)
        })();
        active.remove(&function);
        memo.insert(function, result);
        result
    }

    pub(super) fn define(&self, module: &mut ObjectModule) -> Result<()> {
        let mut requests: Vec<_> = self
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|(key, id)| (*key, *id))
            .collect();
        requests.sort_by_key(|(_, id)| id.as_u32());
        let mut memo = HashMap::default();
        for ((callee, failure), id) in requests {
            let failure_bytes = self.segment(failure, &mut HashSet::default(), &mut memo)
                .ok_or("cannot safely report serialization stack exhaustion: the adapter's reject path has an unguarded recursive call or unavailable frame budget")?;
            let required = self
                .segment(callee, &mut HashSet::default(), &mut memo)
                .and_then(|target| target.checked_add(failure_bytes))
                .and_then(|bytes| bytes.checked_add(RECOVERY_BYTES))
                .unwrap_or(u64::MAX);
            let mut data = DataDescription::new();
            data.set_align(8);
            data.define(required.to_le_bytes().to_vec().into_boxed_slice());
            module.define_data(id, &data)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_edges_stop_recursion_but_missing_frames_and_unguarded_cycles_fail_closed() {
        let a = FuncId::from_u32(0);
        let b = FuncId::from_u32(1);
        let mut budgets = Budgets::default();
        budgets.frames.extend([(a, Some(100)), (b, Some(200))]);
        budgets.tail_arguments.extend([(a, 16), (b, 32)]);
        budgets.calls.extend([(a, vec![b]), (b, vec![a])]);
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default()),
            None
        );
        budgets.protected.insert(a);
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default()),
            Some(348)
        );
        budgets.frames.remove(&b);
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default()),
            None
        );
        budgets.inlined.insert(b);
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default()),
            Some(148)
        );
    }
}
