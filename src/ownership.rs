//! Conservative definite ownership for affine locals over structured control flow.
//! Public references are absent; this pass does not claim to be a borrow checker.
use crate::op::{Op, Ty};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type State = Vec<bool>;
#[derive(Default)]
struct Loop {
    entry: State,
    breaks: Vec<State>,
}

pub fn check(ops: &[Op], locals: &[Ty], parameters: usize) -> Result<()> {
    let mut state = (0..locals.len()).map(|i| i < parameters).collect();
    sequence(ops, locals, &mut state, &mut Vec::new())?;
    Ok(())
}
fn merge(states: &[State], into: &mut State) {
    if let Some(first) = states.first() {
        *into = first.clone();
        for state in &states[1..] {
            for (a, b) in into.iter_mut().zip(state) {
                *a &= b;
            }
        }
    }
}
fn backedge(state: &[bool], entry: &[bool], locals: &[Ty]) -> Result<()> {
    if locals
        .iter()
        .enumerate()
        .any(|(i, ty)| ty.affine() && entry[i] && !state[i])
    {
        return Err(
            "generator moved across a loop backedge; reinitialize it before continuing".into(),
        );
    }
    Ok(())
}
fn sequence(ops: &[Op], locals: &[Ty], state: &mut State, loops: &mut Vec<Loop>) -> Result<bool> {
    for op in ops {
        match op {
            Op::MoveLocal(i, site) | Op::Next(i, site) => {
                if !state.get(*i as usize).copied().unwrap_or(false) {
                    return Err(format!(
                        "{site}: use of moved or possibly moved generator binding"
                    )
                    .into());
                }
                if matches!(op, Op::MoveLocal(..)) {
                    state[*i as usize] = false;
                }
            }
            Op::LoadLocal(i) if locals.get(*i as usize).is_some_and(Ty::affine) => {
                return Err("cannot copy a generator".into())
            }
            Op::StoreLocal(i) => state[*i as usize] = true,
            Op::DropLocal(i) => state[*i as usize] = false,
            Op::Match(arms) => {
                let initial = state.clone();
                let mut continuing = Vec::new();
                for arm in arms.iter() {
                    let mut next = initial.clone();
                    if sequence(&arm.body, locals, &mut next, loops)? {
                        continuing.push(next);
                    }
                }
                if continuing.is_empty() {
                    return Ok(false);
                }
                merge(&continuing, state);
            }
            Op::Loop { condition, body } => {
                let entry = state.clone();
                sequence(condition, locals, state, &mut Vec::new())?;
                let zero = state.clone();
                loops.push(Loop {
                    entry,
                    breaks: vec![zero.clone()],
                });
                if sequence(body, locals, state, loops)? {
                    backedge(state, &loops.last().unwrap().entry, locals)?;
                }
                let exits = loops.pop().unwrap().breaks;
                merge(&exits, state);
            }
            Op::Continue => {
                let target = loops.last().ok_or("continue outside loop")?;
                backedge(state, &target.entry, locals)?;
                return Ok(false);
            }
            Op::Break => {
                loops
                    .last_mut()
                    .ok_or("break outside loop")?
                    .breaks
                    .push(state.clone());
                return Ok(false);
            }
            Op::Return | Op::TailCall(_) | Op::Unreachable => return Ok(false),
            _ => {}
        }
    }
    Ok(true)
}
