//! Source tail positions survive lowering independently of native eligibility.
use super::*;

pub(super) struct Candidate {
    at: Token,
    callee: String,
    fallback: Rejection,
}

enum Rejection {
    ContextExit(crate::ownership::Site),
    LocalReference(Option<Arc<str>>, Option<crate::ownership::Site>),
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
        let mut endings = HashSet::default();
        expression_endings(ops, None, &mut endings);
        let candidates = self.tail_candidates.split_off(start);
        self.tail_candidates
            .extend(candidates.into_iter().filter(|candidate| {
                endings.contains(&(
                    candidate.callee.clone(),
                    candidate.at.line,
                    candidate.at.column,
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
        // Argument evaluation may have left a different position. Calls also
        // need their own site for std caller notes, even outside tail position.
        self.mark_call(at, ops);
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
    let last = ops.iter().rev().find(|op| {
        !matches!(
            op,
            Op::Loan(_)
                | Op::UseLoan(_)
                | Op::EndLoan(_)
                | Op::Site(_)
                | Op::CallerSite(_)
                | Op::Returning(..)
                | Op::Leaving
                | Op::Building(_)
                | Op::Passes(_)
                | Op::Forwards(_)
                | Op::Tokens(_)
        )
    });
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
        Some(Op::If {
            then,
            elifs,
            otherwise,
        }) => {
            expression_endings(then, site, endings);
            let mut site = site;
            for arm in elifs.iter() {
                site = crate::ownership::site_after(&arm.condition, site);
                expression_endings(&arm.body, site, endings);
            }
            expression_endings(otherwise, site, endings);
        }
        _ => {}
    }
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(super) fn validate(
    ops: &[Op],
    generics: &generics::Engine,
    sigs: &functions::Functions,
) -> Result<()> {
    // Only a function with a call in source tail position has anything to
    // validate. Without one, skip the whole-program tables built below.
    if generics.tail_candidates.is_empty() {
        return Ok(());
    }
    // Every definition by name. Generators are not part of the call graph,
    // but their signatures are visible to the normalization below.
    let mut definitions: HashMap<&str, usize> =
        HashMap::with_capacity_and_hasher(ops.len(), Default::default());
    for (index, op) in ops.iter().enumerate() {
        if let Op::DefineFn(name, _) = op {
            definitions.insert(name, index);
        }
    }
    let function_at = |index: usize| match &ops[index] {
        Op::DefineFn(name, function) => (name.as_str(), &**function),
        _ => unreachable!("definitions index function definitions"),
    };
    let node = |name: &str| {
        definitions
            .get(name)
            .copied()
            .filter(|&index| function_at(index).1.generator.is_none())
    };
    let mut callers: Vec<usize> = generics
        .tail_candidates
        .keys()
        .filter_map(|name| node(name))
        .collect();
    // Report the first failure in program order, whatever the table order.
    callers.sort_unstable();
    // A candidate is checked only when its callee is in the caller's
    // recursion cycle, and a cycle lies within what the caller reaches. Only
    // that part of the call graph is built: walking every body cost time per
    // function in every program that imports a module with a tail call
    // (issue #57).
    let mut nodes: Vec<usize> = Vec::new();
    let mut local: HashMap<usize, usize> = HashMap::default();
    for &caller in &callers {
        if let std::collections::hash_map::Entry::Vacant(entry) = local.entry(caller) {
            entry.insert(nodes.len());
            nodes.push(caller);
        }
    }
    let mut graph: Vec<Vec<usize>> = Vec::new();
    while graph.len() < nodes.len() {
        let mut edges = Vec::new();
        crate::threading::walk(&function_at(nodes[graph.len()]).1.body, &mut |op| {
            if let Op::Call(name) | Op::TailCall(name) = op
                && let Some(index) = node(name)
            {
                let next = nodes.len();
                edges.push(*local.entry(index).or_insert_with(|| {
                    nodes.push(index);
                    next
                }));
            }
        });
        edges.sort_unstable();
        edges.dedup();
        graph.push(edges);
    }
    let components = components(&graph);
    // This is the same normalization codegen consumes. A raw TailCall marker
    // alone does not prove that its reference/storage transfer was approved.
    let signatures = |name: &str| {
        definitions
            .get(name)
            .map(|&index| Arc::clone(&function_at(index).1.sig))
    };
    for caller in callers {
        let (name, function) = function_at(caller);
        let candidates = &generics.tail_candidates[name];
        let caller_index = local[&caller];
        let prepared = crate::tail_abi::prepare_function(function, &signatures);
        let mut native = HashSet::default();
        tail_sites(
            prepared.as_deref().unwrap_or(&function.body),
            None,
            &mut native,
        );
        for candidate in candidates {
            let Some(callee) = node(&candidate.callee) else {
                continue;
            };
            let Some(&callee_index) = local.get(&callee) else {
                continue;
            };
            if components[caller_index] != components[callee_index] {
                continue;
            }
            let reason = crate::tail_abi::classify(&function.sig, &function_at(callee).1.sig)
                .err()
                .map(|reason| reason.to_string())
                .or_else(|| {
                    (!native.contains(&(
                        candidate.callee.clone(),
                        candidate.at.line,
                        candidate.at.column,
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
                let message = format!(
                    "recursive call to `{}`{cycle} is in tail position but cannot be a tail call: {reason}",
                    generics.diagnostic_name(&candidate.callee)
                );
                return Err(match generics.requirement_of(sigs, name) {
                    Some(required) => required
                        .report(&format!("{}: ", candidate.at.location()), &message)
                        .into(),
                    None => candidate.at.error(message),
                });
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
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                tail_sites(then, site, sites);
                let mut site = site;
                for arm in elifs.iter() {
                    tail_sites(&arm.condition, site, sites);
                    site = crate::ownership::site_after(&arm.condition, site);
                    tail_sites(&arm.body, site, sites);
                }
                tail_sites(otherwise, site, sites);
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
