//! Waits that skip a later cancel (issue #355). A task or pool job that holds
//! a token can wait for it to be cancelled. A join, `result()`, or pool
//! shutdown that this function reaches before cancelling that token can then
//! wait forever. Tokens are matched by binding: a share, borrow, or move named
//! in the arguments of `spawn` or `submit`, from `Op::Tokens` markers. See
//! `book/src/design/facilities/03-concurrency/cancellation.md`.

use super::{Checker, Error, Hazard, Kind, Result, Site, early_exit};
use crate::control::ControlOp;
use crate::executor::ExecutorOp;
use crate::hash::HashMap;
use crate::op::{Op, TokenMark, Ty};
use crate::threading::{ThreadOp, owns_executor};

fn token(ty: &Ty) -> bool {
    match ty {
        Ty::CancellationToken => true,
        Ty::Ref(inner, _) => token(inner),
        _ => false,
    }
}

/// Visit every sequence in `ops`, outer first.
fn sequences<'a>(ops: &'a [Op], visit: &mut impl FnMut(&'a [Op])) {
    visit(ops);
    for op in ops {
        match op {
            Op::Try { cleanup, .. } => sequences(cleanup, visit),
            Op::Match(arms) => arms.iter().for_each(|arm| sequences(&arm.body, visit)),
            Op::Loop { condition, body } => {
                sequences(condition, visit);
                sequences(body, visit);
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                sequences(then, visit);
                for arm in elifs.iter() {
                    sequences(&arm.condition, visit);
                    sequences(&arm.body, visit);
                }
                sequences(otherwise, visit);
            }
            _ => {}
        }
    }
}

/// The operation that produced the value `ops[index]` consumes, past markers
/// and the operations that read through a reference.
fn operand(ops: &[Op], index: usize) -> Option<&Op> {
    ops[..index].iter().rev().find(|op| {
        !matches!(
            op,
            Op::Site(_)
                | Op::CallerSite(_)
                | Op::UseLoan(_)
                | Op::EndLoan(_)
                | Op::ReadRef(_)
                | Op::Access(..)
                | Op::Loan(_)
                | Op::FieldMove(_)
                | Op::Tokens(_)
        )
    })
}

fn local(op: Option<&Op>) -> Option<u8> {
    match op? {
        Op::LoadLocal(slot) | Op::MoveLocal(slot) | Op::BorrowLocal(slot, _) => Some(*slot),
        _ => None,
    }
}

/// The marker directly before a start or submission.
fn holds(ops: &[Op], index: usize) -> &[u8] {
    match ops[..index]
        .iter()
        .rev()
        .find(|op| !matches!(op, Op::Site(_) | Op::CallerSite(_)))
    {
        Some(Op::Tokens(TokenMark::Holds(slots))) => slots,
        _ => &[],
    }
}

#[derive(Clone)]
struct Path {
    /// By root slot: cancelled on every path reaching here.
    cancelled: Vec<bool>,
    /// By task slot: joined explicitly on every path reaching here.
    joined: Vec<bool>,
    /// Token roots of jobs submitted and not yet drained by a shutdown, with
    /// their first submission.
    pending: Vec<(u8, Option<Site>)>,
}

impl Path {
    fn merge(paths: &[Path]) -> Option<Path> {
        let (first, rest) = paths.split_first()?;
        let mut merged = first.clone();
        for path in rest {
            for (into, other) in merged.cancelled.iter_mut().zip(&path.cancelled) {
                *into &= other;
            }
            for (into, other) in merged.joined.iter_mut().zip(&path.joined) {
                *into &= other;
            }
            for job in &path.pending {
                if !merged.pending.iter().any(|(root, _)| *root == job.0) {
                    merged.pending.push(*job);
                }
            }
        }
        Some(merged)
    }
}

struct Loop {
    breaks: Vec<Path>,
}

/// A cancel, by position in source order.
#[derive(Clone, Copy)]
struct Cancel {
    position: usize,
    site: Option<Site>,
    /// The cancel is the exit of a `cancel_on_exit()` guard.
    guard: bool,
}

/// How a function exit leaves, and the state there.
struct Exit {
    path: Path,
    site: Option<Site>,
    what: &'static str,
}

struct Walk<'c, 'a, 'p> {
    checker: &'c Checker<'a, 'p>,
    /// By task slot: the token slots its start holds.
    starts: Vec<&'a [u8]>,
    /// A pool the function owns, which its exits shut down.
    pool: Option<usize>,
    /// Root slot of each token slot: the binding a share or borrow came from.
    roots: Vec<u8>,
    /// Source-order position of each operation that cancels or waits.
    positions: HashMap<*const Op, usize>,
    /// Last cancel of each root.
    last: Vec<Option<Cancel>>,
    /// Exits other than the normal end, for the worker-side rule.
    exits: Vec<Exit>,
}

pub(super) fn check<'a>(checker: &Checker<'a, '_>) -> Result<()> {
    let body = checker.body;
    let mut cancels = false;
    sequences(body, &mut |ops| {
        cancels |= ops
            .iter()
            .any(|op| matches!(op, Op::Control(ControlOp::Cancel)));
    });
    if !cancels {
        return Ok(());
    }
    let locals = checker.locals;
    // A token slot stored from exactly one other token slot shares its root.
    let mut sources: Vec<Option<Option<u8>>> = vec![None; locals.len()];
    let mut guards = vec![false; locals.len()];
    sequences(body, &mut |ops| {
        for (index, op) in ops.iter().enumerate() {
            match op {
                Op::StoreLocal(slot) if token(&locals[*slot as usize]) => {
                    let source = local(operand(ops, index))
                        .filter(|source| *source != *slot && token(&locals[*source as usize]));
                    let entry = &mut sources[*slot as usize];
                    *entry = match entry {
                        None => Some(source),
                        Some(previous) if *previous == source => Some(source),
                        Some(_) => Some(None),
                    };
                }
                Op::Tokens(TokenMark::Guard(slot)) => guards[*slot as usize] = true,
                _ => {}
            }
        }
    });
    let roots: Vec<u8> = (0..locals.len())
        .map(|slot| {
            let mut root = slot as u8;
            // Bounded: a cycle of single sources has no outside binding.
            for _ in 0..locals.len() {
                match sources[root as usize] {
                    Some(Some(source)) => root = source,
                    _ => break,
                }
            }
            root
        })
        .collect();
    let mut positions = HashMap::default();
    let mut last: Vec<Option<Cancel>> = vec![None; locals.len()];
    let mut counter = 0;
    number(
        body,
        None,
        &mut counter,
        &mut |op, ops, index, site, position| {
            positions.insert(op as *const Op, position);
            if matches!(op, Op::Control(ControlOp::Cancel))
                && let Some(slot) = local(operand(ops, index))
                && token(&locals[slot as usize])
            {
                last[roots[slot as usize] as usize] = Some(Cancel {
                    position,
                    site,
                    guard: guards[slot as usize],
                });
            }
        },
    );
    if last.iter().all(Option::is_none) {
        return Ok(());
    }
    let mut starts = vec![&[][..]; locals.len()];
    sequences(body, &mut |ops| {
        for (index, op) in ops.iter().enumerate() {
            if let Op::Thread(ThreadOp::Start(_, slot)) = op {
                starts[*slot as usize] = holds(ops, index);
            }
        }
    });
    let pool = (checker.parameters..locals.len()).find(|&slot| owns_executor(&locals[slot]));
    let mut walk = Walk {
        checker,
        starts,
        pool,
        roots,
        positions,
        last,
        exits: Vec::new(),
    };
    let mut path = Path {
        cancelled: vec![false; locals.len()],
        joined: vec![false; locals.len()],
        pending: Vec::new(),
    };
    let end = walk.sequence(body, &mut path, &mut Vec::new(), None, None)?;
    let normal = match end {
        Some(path) => Some(path),
        // The function's final `return` is its normal end.
        None => walk
            .exits
            .last()
            .filter(|exit| exit.what == "the function's end")
            .map(|exit| exit.path.clone()),
    };
    walk.worker(normal)
}

/// Number the operations in source order: each sequence's operations in
/// turn, with a nested body before the operation that follows it.
fn number<'a>(
    ops: &'a [Op],
    mut site: Option<Site>,
    counter: &mut usize,
    visit: &mut impl FnMut(&'a Op, &'a [Op], usize, Option<Site>, usize),
) {
    for (index, op) in ops.iter().enumerate() {
        if let Op::Site(at) = op {
            site = Some(*at);
        }
        *counter += 1;
        visit(op, ops, index, site, *counter);
        match op {
            Op::Try { cleanup, .. } => number(cleanup, site, counter, visit),
            Op::Match(arms) => {
                for arm in arms.iter() {
                    number(&arm.body, site, counter, visit)
                }
            }
            Op::Loop { condition, body } => {
                number(condition, site, counter, visit);
                number(body, site, counter, visit);
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                number(then, site, counter, visit);
                for arm in elifs.iter() {
                    number(&arm.condition, site, counter, visit);
                    number(&arm.body, site, counter, visit);
                }
                number(otherwise, site, counter, visit);
            }
            _ => {}
        }
    }
}

/// What reaches a wait that skips a cancel.
enum Wait {
    /// The end of a task's `with` block.
    Leaving,
    Join,
    Result,
    /// A pool shutdown, automatic at the end of its scope or not.
    Shutdown(bool),
}

impl Walk<'_, '_, '_> {
    fn name(&self, root: u8) -> String {
        match self
            .checker
            .origins
            .names
            .get(root as usize)
            .and_then(|name| name.as_deref())
        {
            Some(name) => format!("`{name}`"),
            None => "the token".into(),
        }
    }

    /// The root whose later cancel the wait at `op` skips, if any.
    fn skipped(&self, roots: impl IntoIterator<Item = u8>, op: &Op, path: &Path) -> Option<u8> {
        let position = self.positions.get(&(op as *const Op)).copied()?;
        roots.into_iter().find(|&root| {
            !path.cancelled[root as usize]
                && self.last[root as usize].is_some_and(|cancel| cancel.position > position)
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn reject(
        &self,
        wait: Wait,
        root: u8,
        site: Option<Site>,
        exit: Option<(Option<Site>, &str)>,
        submitted: Option<Site>,
    ) -> Box<Error> {
        let name = self.name(root);
        let cancel = self.last[root as usize].expect("a skipped cancel exists");
        let (what, holder, waits, advice) = match wait {
            Wait::Leaving => (
                "leaving this `with` block joins".to_owned(),
                "a task",
                "the join",
                if cancel.guard {
                    format!(
                        "put `{}.cancel_on_exit()` directly after each task that holds it, so it \
                         cancels first: `with spawn(...)?, {}.cancel_on_exit():`",
                        bare(&name),
                        bare(&name)
                    )
                } else {
                    format!(
                        "cancel it on every exit: `with spawn(...)?, {}.cancel_on_exit():`",
                        bare(&name)
                    )
                },
            ),
            Wait::Join => (
                "this join waits for".to_owned(),
                "a task",
                "the join",
                format!("cancel {name} before joining"),
            ),
            Wait::Result => (
                "this result() waits for".to_owned(),
                "a job",
                "the result wait",
                format!("cancel {name} before waiting for the result"),
            ),
            Wait::Shutdown(automatic) => (
                if automatic {
                    "leaving its scope shuts down a pool running".to_owned()
                } else {
                    "this shuts down a pool running".to_owned()
                },
                "a job",
                "the shutdown",
                if automatic {
                    format!(
                        "cancel it on every exit: `with ThreadPoolExecutor(...)? as pool, \
                         {}.cancel_on_exit():`",
                        bare(&name)
                    )
                } else {
                    format!("cancel {name} before shutting the pool down")
                },
            ),
        };
        let message = format!(
            "{what} {holder} that holds {name} before this function cancels it: \
             {holder} waiting for the cancel would keep {waits} waiting forever; {advice}"
        );
        let mut error = self
            .checker
            .error(Kind::Uncancelled { message }, None, site);
        if let Some((Some(at), exit)) = exit {
            let note = match wait {
                Wait::Shutdown(true) => format!("{exit} leaves the pool's scope here"),
                _ => format!("{exit} leaves the block here"),
            };
            error.notes.push((at, note));
        }
        if let Some(at) = submitted {
            error.notes.push((at, "the job is submitted here".into()));
        }
        if let Some(at) = cancel.site {
            let note = if cancel.guard {
                format!(
                    "`{}.cancel_on_exit()` cancels it here, too late",
                    bare(&name)
                )
            } else {
                format!("{name} is cancelled here")
            };
            error.notes.push((at, note));
        }
        error
    }

    /// Check the pool shutdown at `op`; it drains every pending job.
    fn shutdown(
        &self,
        op: &Op,
        path: &mut Path,
        site: Option<Site>,
        automatic: bool,
        exit: Option<(Option<Site>, &str)>,
    ) -> Result<()> {
        let pending = std::mem::take(&mut path.pending);
        let roots = pending.iter().map(|(root, _)| *root);
        if let Some(root) = self.skipped(roots, op, path) {
            let submitted = pending.iter().find(|(r, _)| *r == root).and_then(|j| j.1);
            return Err(self.reject(Wait::Shutdown(automatic), root, site, exit, submitted));
        }
        Ok(())
    }

    fn roots(&self, slots: &[u8]) -> Vec<u8> {
        slots
            .iter()
            .map(|&slot| self.roots[slot as usize])
            .filter(|&root| self.last[root as usize].is_some())
            .collect()
    }

    /// Walk `ops` on `path`. Returns the state at the end when it falls
    /// through. `failing` is the `?` whose cleanup this is.
    fn sequence(
        &mut self,
        ops: &[Op],
        path: &mut Path,
        loops: &mut Vec<Loop>,
        mut site: Option<Site>,
        failing: Option<Option<Site>>,
    ) -> Result<Option<Path>> {
        let mut written = site.filter(|site| !site.scope_end);
        // Jobs that `submit(...)?` hands to a pool only on success.
        let mut accepted: Vec<(u8, Option<Site>)> = Vec::new();
        // The exit an automatic join or shutdown at `index` belongs to.
        let exit = |index: usize, written: Option<Site>| match failing {
            Some(at) => Some((at, "`?`")),
            None => early_exit(&ops[index + 1..], written).map(|exit| (written, exit)),
        };
        for (index, op) in ops.iter().enumerate() {
            match op {
                Op::Site(at) => {
                    site = Some(*at);
                    if !at.scope_end {
                        written = site;
                    }
                }
                Op::Control(ControlOp::Cancel) => {
                    if let Some(slot) = local(operand(ops, index))
                        && token(&self.checker.locals[slot as usize])
                    {
                        path.cancelled[self.roots[slot as usize] as usize] = true;
                    }
                }
                Op::StoreLocal(slot) => {
                    let slot = *slot as usize;
                    if self.roots[slot] as usize == slot {
                        path.cancelled[slot] = false;
                    }
                    path.joined[slot] = false;
                }
                Op::Thread(ThreadOp::Start(_, slot)) => path.joined[*slot as usize] = false,
                Op::Thread(ThreadOp::Join(_)) => {
                    let Some(slot) = local(operand(ops, index)) else {
                        continue;
                    };
                    path.joined[slot as usize] = true;
                    let roots = self.roots(self.started(slot));
                    if let Some(root) = self.skipped(roots, op, path) {
                        return Err(self.reject(Wait::Join, root, site, None, None));
                    }
                }
                Op::Thread(ThreadOp::Finish(_, slot)) => {
                    if !path.joined[*slot as usize] {
                        let roots = self.roots(self.started(*slot));
                        if let Some(root) = self.skipped(roots, op, path) {
                            let exit = exit(index, written);
                            return Err(self.reject(Wait::Leaving, root, site, exit, None));
                        }
                    }
                }
                Op::Executor(ExecutorOp::Submit(..)) => {
                    let jobs = self.roots(holds(ops, index)).into_iter().map(|r| (r, site));
                    fn next(ops: &[Op]) -> &[Op] {
                        let start = ops.iter().position(|op| {
                            !matches!(
                                op,
                                Op::UseLoan(_) | Op::EndLoan(_) | Op::Site(_) | Op::CallerSite(_)
                            )
                        });
                        &ops[start.unwrap_or(ops.len())..]
                    }
                    let mut result = next(&ops[index + 1..]);
                    // `submit(...).drop_job()?` also hands the job over only on success.
                    if let Some(rest) = super::futures::after_drop_job(result) {
                        result = next(rest);
                    }
                    if matches!(result.first(), Some(Op::Try { .. })) {
                        accepted.extend(jobs);
                    } else {
                        add(&mut path.pending, jobs);
                    }
                }
                Op::Executor(ExecutorOp::Shutdown) => self.shutdown(op, path, site, false, None)?,
                Op::Executor(ExecutorOp::Result(_)) => {
                    let jobs = self.checker.futures.before(&ops[..index]);
                    let held: Vec<u8> = jobs
                        .iter()
                        .filter_map(|(hazard, _)| match hazard {
                            Hazard::Cancels(slot) => Some(*slot),
                            _ => None,
                        })
                        .collect();
                    if let Some(root) = self.skipped(self.roots(&held), op, path) {
                        let submitted = jobs
                            .iter()
                            .find(|(hazard, _)| {
                                matches!(hazard, Hazard::Cancels(slot)
                                    if self.roots[*slot as usize] == root)
                            })
                            .and_then(|(_, at)| *at);
                        return Err(self.reject(Wait::Result, root, site, None, submitted));
                    }
                }
                Op::MoveLocal(slot)
                    if matches!(ops.get(index + 1), Some(Op::Drop))
                        && owns_executor(&self.checker.locals[*slot as usize]) =>
                {
                    self.shutdown(op, path, site, false, None)?;
                }
                Op::DropLocal(slot) if owns_executor(&self.checker.locals[*slot as usize]) => {
                    // An automatic shutdown is reported where its pool starts.
                    let at = self.checker.bound(*slot as usize).or(site);
                    self.shutdown(op, path, at, true, exit(index, written))?;
                }
                Op::Try { cleanup, .. } => {
                    let mut failed = path.clone();
                    if let Some(end) =
                        self.sequence(cleanup, &mut failed, &mut Vec::new(), site, Some(site))?
                    {
                        self.leave(op, end, site, "`?`")?;
                    }
                    add(&mut path.pending, accepted.drain(..));
                }
                Op::Match(arms) => {
                    let mut continuing = Vec::new();
                    for arm in arms.iter() {
                        let mut next = path.clone();
                        if let Some(end) = self.sequence(&arm.body, &mut next, loops, site, None)? {
                            continuing.push(end);
                        }
                    }
                    let Some(merged) = Path::merge(&continuing) else {
                        return Ok(None);
                    };
                    *path = merged;
                }
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    let mut continuing = Vec::new();
                    let mut next = path.clone();
                    if let Some(end) = self.sequence(then, &mut next, loops, site, None)? {
                        continuing.push(end);
                    }
                    let mut reached = true;
                    for arm in elifs.iter() {
                        let Some(end) = self.sequence(&arm.condition, path, loops, site, None)?
                        else {
                            reached = false;
                            break;
                        };
                        *path = end;
                        let mut next = path.clone();
                        if let Some(end) = self.sequence(&arm.body, &mut next, loops, site, None)? {
                            continuing.push(end);
                        }
                    }
                    if reached
                        && let Some(end) = self.sequence(otherwise, path, loops, site, None)?
                    {
                        continuing.push(end);
                    }
                    let Some(merged) = Path::merge(&continuing) else {
                        return Ok(None);
                    };
                    *path = merged;
                }
                Op::Loop { condition, body } => {
                    let Some(zero) = self.sequence(condition, path, &mut Vec::new(), site, None)?
                    else {
                        return Ok(None);
                    };
                    let pending = zero.pending.len();
                    loops.push(Loop {
                        breaks: vec![zero.clone()],
                    });
                    let mut next = zero.clone();
                    let end = self.sequence(body, &mut next, loops, site, None)?;
                    let Loop { breaks, .. } = loops.pop().unwrap();
                    // A job submitted in one iteration is still in its pool
                    // in the next, so shutdowns before the submit see it too.
                    let grown = end
                        .iter()
                        .chain(&breaks)
                        .find(|p| p.pending.len() > pending);
                    if let Some(grown) = grown {
                        let mut again = zero.clone();
                        add(&mut again.pending, grown.pending.iter().copied());
                        loops.push(Loop { breaks: Vec::new() });
                        let result = self.sequence(body, &mut again, loops, site, None);
                        loops.pop();
                        result?;
                    }
                    *path = Path::merge(&breaks).expect("the loop's condition can fail");
                }
                Op::Continue => return Ok(None),
                Op::Break => {
                    if let Some(target) = loops.last_mut() {
                        target.breaks.push(path.clone());
                    }
                    return Ok(None);
                }
                Op::Return => {
                    let what = if index + 1 == ops.len() && std::ptr::eq(ops, self.checker.body) {
                        "the function's end"
                    } else {
                        "`return`"
                    };
                    self.leave(op, path.clone(), written, what)?;
                    return Ok(None);
                }
                Op::TailCall(_) | Op::TailCallIndirect(_) => {
                    self.leave(op, path.clone(), written, "this tail call")?;
                    return Ok(None);
                }
                Op::Unreachable => return Ok(None),
                _ => {}
            }
        }
        Ok(Some(path.clone()))
    }

    /// The function exits at `op`: its remaining pools shut down.
    fn leave(
        &mut self,
        op: &Op,
        mut path: Path,
        site: Option<Site>,
        what: &'static str,
    ) -> Result<()> {
        if !path.pending.is_empty()
            && let Some(pool) = self.pool
        {
            let exit = (what != "the function's end").then_some((site, what));
            // An automatic shutdown is reported where its pool starts.
            let at = self.checker.bound(pool).or(site);
            self.shutdown(op, &mut path, at, true, exit)?;
        }
        self.exits.push(Exit { path, site, what });
        Ok(())
    }

    /// Token slots the task started into `slot` holds.
    fn started(&self, slot: u8) -> &[u8] {
        self.starts[slot as usize]
    }

    /// A function whose normal end cancels a token parameter must cancel it
    /// on every exit: a caller can be waiting for that cancel.
    fn worker(&self, normal: Option<Path>) -> Result<()> {
        let Some(normal) = normal else {
            return Ok(());
        };
        for parameter in 0..self.checker.parameters {
            let root = self.roots[parameter];
            if root as usize != parameter
                || !token(&self.checker.locals[parameter])
                || !normal.cancelled[parameter]
            {
                continue;
            }
            let Some(exit) = self
                .exits
                .iter()
                .find(|exit| exit.what != "the function's end" && !exit.path.cancelled[parameter])
            else {
                continue;
            };
            let name = self.name(root);
            let message = format!(
                "{} leaves this function without cancelling {name}, which its normal end \
                 cancels: a caller waiting for the cancel would wait forever; cancel it on \
                 every exit by putting the body in `with {}.cancel_on_exit():`",
                exit.what,
                bare(&name)
            );
            let mut error = self
                .checker
                .error(Kind::Uncancelled { message }, None, exit.site);
            if let Some(cancel) = self.last[parameter]
                && let Some(at) = cancel.site
            {
                error.notes.push((at, format!("{name} is cancelled here")));
            }
            return Err(error);
        }
        Ok(())
    }
}

fn add(pending: &mut Vec<(u8, Option<Site>)>, jobs: impl IntoIterator<Item = (u8, Option<Site>)>) {
    for job in jobs {
        if !pending.iter().any(|(root, _)| *root == job.0) {
            pending.push(job);
        }
    }
}

/// A rendered name without its backticks, for code in advice.
fn bare(name: &str) -> &str {
    name.trim_matches('`')
}
