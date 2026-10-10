//! Source tail positions survive lowering independently of native eligibility.
use super::*;

pub(super) struct Candidate {
    at: Token,
    callee: String,
    fallback: Rejection,
}

enum Rejection {
    ContextExit(crate::ownership::Site),
    LocalReference(Option<Rc<str>>, Option<crate::ownership::Site>),
    ReferenceProof,
    PostCall,
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContextExit(at) => write!(
                f,
                "the context opened at {}:{} requires a post-call exit action",
                at.line, at.column
            ),
            Self::LocalReference(name, at) => {
                match name {
                    Some(name) => write!(f, "the reference argument borrows local `{name}`, whose storage does not survive the jump")?,
                    None => f.write_str("the reference argument borrows a temporary whose storage does not survive the jump")?,
                }
                if let Some(at) = at {
                    write!(f, " (borrow at {}:{})", at.line, at.column)?;
                }
                Ok(())
            }
            Self::ReferenceProof => {
                f.write_str("the outgoing reference has not been proven to outlive this frame")
            }
            Self::PostCall => f.write_str(
                "required post-call reference or cleanup operations prevent a native transfer",
            ),
        }
    }
}

impl Lower<'_> {
    pub(super) fn tail_expression(
        &mut self,
        expression: &Expr,
        expected: Type,
        ops: &mut Vec<Op>,
        tail: bool,
    ) -> Result<Type> {
        let expression = ungroup(expression);
        let saved = std::mem::take(&mut self.source_tail);
        if tail {
            source_sites(expression, &mut self.source_tail);
        }
        let start = self.tail_candidates.len();
        let result = self.expr_expected(expression, expected, ops);
        self.source_tail = saved;
        // A receiver and its method can share the starting token. Keep only
        // the root call, and exclude propagation, conversions and builtin work
        // after that call, before scope cleanup has changed the operation tail.
        let mut endings = HashSet::new();
        expression_endings(ops, None, &mut endings);
        let candidates = self.tail_candidates.split_off(start);
        self.tail_candidates
            .extend(candidates.into_iter().filter(|candidate| {
                endings.contains(&(
                    candidate.callee.clone(),
                    candidate.at.line as u32,
                    candidate.at.column as u32,
                ))
            }));
        result
    }

    pub(super) fn record_direct_call(
        &mut self,
        callee: &str,
        at: &Token,
        loans: &[usize],
        ops: &mut Vec<Op>,
    ) {
        if !self.source_tail.contains(&site(at, false)) {
            return;
        }
        self.mark(at, ops);
        let fallback = if let Some(context) = self.contexts.last() {
            Rejection::ContextExit(context.at)
        } else if let Some(loan) = loans.iter().map(|id| &self.loans[*id]).find(|loan| {
            !self.sigs[&self.function_name]
                .inputs
                .get(loan.root as usize)
                .is_some_and(|(_, ty)| matches!(ty, Ty::Ref(..)))
        }) {
            let mut at = None;
            let mut borrow = None;
            for op in ops.iter() {
                match op {
                    Op::Site(site) => at = Some(*site),
                    Op::Loan(created) if created.id == loan.id => borrow = at,
                    _ => {}
                }
            }
            Rejection::LocalReference(
                self.slot_names.get(loan.root as usize).cloned().flatten(),
                borrow,
            )
        } else if !loans.is_empty() {
            Rejection::ReferenceProof
        } else {
            Rejection::PostCall
        };
        self.tail_candidates.push(Candidate {
            at: at.clone(),
            callee: callee.into(),
            fallback,
        });
    }
}

fn source_sites(expression: &Expr, sites: &mut Vec<crate::ownership::Site>) {
    let expression = ungroup(expression);
    match &expression.kind {
        Expression::Conditional { yes, no, .. } => {
            source_sites(yes, sites);
            source_sites(no, sites);
        }
        Expression::Binary(operator, _, right) if operator == "and" || operator == "or" => {
            source_sites(right, sites);
        }
        Expression::Call(..)
        | Expression::GenericCall(..)
        | Expression::Method(..)
        | Expression::GenericMethod(..)
        | Expression::Invoke(..) => sites.push(site(&expression.at, false)),
        _ => {}
    }
}

fn expression_endings(
    ops: &[Op],
    inherited: Option<crate::ownership::Site>,
    endings: &mut HashSet<(String, u32, u32)>,
) {
    let site = ops
        .iter()
        .rev()
        .find_map(|op| {
            if let Op::Site(at) = op {
                Some(*at)
            } else {
                None
            }
        })
        .or(inherited);
    let last = ops
        .iter()
        .rev()
        .find(|op| !matches!(op, Op::Loan(_) | Op::UseLoan(_) | Op::Site(_)));
    match last {
        Some(Op::Call(name)) => {
            if let Some(at) = site {
                endings.insert((name.clone(), at.line, at.column));
            }
        }
        Some(Op::Match(arms)) => {
            for arm in arms.iter() {
                expression_endings(&arm.body, site, endings);
            }
        }
        _ => {}
    }
}

pub(super) fn validate(ops: &[Op], generics: &generics::Engine) -> Result<()> {
    // This is the same normalization codegen consumes. A raw TailCall marker
    // alone does not prove that its reference/storage transfer was approved.
    let prepared = crate::tail_abi::prepare(ops);
    let prepared_functions: HashMap<_, _> = prepared
        .iter()
        .filter_map(|op| match op {
            Op::DefineFn(name, function) => Some((name.as_str(), function)),
            _ => None,
        })
        .collect();
    let functions: Vec<_> = ops
        .iter()
        .filter_map(|op| match op {
            Op::DefineFn(name, function) if function.generator.is_none() => {
                Some((name.as_str(), function))
            }
            _ => None,
        })
        .collect();
    let indices: HashMap<_, _> = functions
        .iter()
        .enumerate()
        .map(|(index, (name, _))| (*name, index))
        .collect();
    let graph: Vec<Vec<usize>> = functions
        .iter()
        .map(|(_, function)| {
            let mut edges = Vec::new();
            crate::threading::walk(&function.body, &mut |op| {
                if let Op::Call(name) | Op::TailCall(name) = op {
                    if let Some(index) = indices.get(name.as_str()) {
                        edges.push(*index);
                    }
                }
            });
            edges.sort_unstable();
            edges.dedup();
            edges
        })
        .collect();
    let components = components(&graph);
    for (caller_index, (name, function)) in functions.iter().enumerate() {
        let Some(candidates) = generics.tail_candidates.get(*name) else {
            continue;
        };
        let mut native = HashSet::new();
        tail_sites(&prepared_functions[name].body, None, &mut native);
        for candidate in candidates {
            let Some(&callee_index) = indices.get(candidate.callee.as_str()) else {
                continue;
            };
            if components[caller_index] != components[callee_index] {
                continue;
            }
            let reason = crate::tail_abi::classify(&function.sig, &functions[callee_index].1.sig)
                .err()
                .map(|reason| reason.to_string())
                .or_else(|| {
                    (!native.contains(&(
                        candidate.callee.clone(),
                        candidate.at.line as u32,
                        candidate.at.column as u32,
                    )))
                    .then(|| candidate.fallback.to_string())
                });
            if let Some(reason) = reason {
                let cycle = if caller_index == callee_index {
                    String::new()
                } else {
                    format!(
                        " (in the same direct recursion cycle as `{}`)",
                        generics.diagnostic_name(name)
                    )
                };
                return Err(candidate.at.error(format!("recursive call to `{}`{cycle} is in tail position but cannot be a tail call: {reason}", generics.diagnostic_name(&candidate.callee))));
            }
        }
    }
    Ok(())
}

fn tail_sites(
    ops: &[Op],
    inherited: Option<crate::ownership::Site>,
    sites: &mut HashSet<(String, u32, u32)>,
) {
    let mut site = inherited;
    for op in ops {
        match op {
            Op::Site(at) => site = Some(*at),
            Op::TailCall(name) => {
                if let Some(at) = site {
                    sites.insert((name.clone(), at.line, at.column));
                }
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    tail_sites(&arm.body, site, sites);
                }
            }
            Op::Loop { condition, body } => {
                tail_sites(condition, site, sites);
                tail_sites(body, site, sites);
            }
            Op::Try { cleanup, .. } => tail_sites(cleanup, site, sites),
            _ => {}
        }
    }
}

/// Iterative Kosaraju traversal avoids consuming the compiler's stack on a
/// large generated call graph. Concrete specializations are distinct nodes.
fn components(graph: &[Vec<usize>]) -> Vec<usize> {
    let mut seen = vec![false; graph.len()];
    let mut order = Vec::with_capacity(graph.len());
    let mut reverse = vec![Vec::new(); graph.len()];
    for (node, edges) in graph.iter().enumerate() {
        for &next in edges {
            reverse[next].push(node);
        }
        let mut pending = vec![(node, false)];
        while let Some((node, exiting)) = pending.pop() {
            if exiting {
                order.push(node);
            } else if !std::mem::replace(&mut seen[node], true) {
                pending.push((node, true));
                pending.extend(graph[node].iter().rev().map(|&next| (next, false)));
            }
        }
    }
    let mut components = vec![usize::MAX; graph.len()];
    for root in order.into_iter().rev() {
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if components[node] != usize::MAX {
                continue;
            }
            components[node] = root;
            pending.extend(&reverse[node]);
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::components;

    #[test]
    fn concrete_cycles_include_non_tail_edges_but_not_one_way_neighbors() {
        let groups = components(&[vec![1], vec![2, 3], vec![0], vec![4], vec![], vec![5]]);
        assert_eq!(groups[0], groups[1]);
        assert_eq!(groups[1], groups[2]);
        assert_ne!(groups[2], groups[3]);
        assert_ne!(groups[3], groups[4]);
        assert_ne!(groups[4], groups[5]);
    }
}
