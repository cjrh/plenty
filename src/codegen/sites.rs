//! Source positions for runtime traps.
//!
//! An operation that can trap passes the runtime a pointer to a record in a
//! read-only table of its function's positions. The runtime reads the record
//! only when the trap fires, so the passing path costs one address
//! computation.
//!
//! Each function has its own table so that its offsets are known while its IR
//! is built on any thread. The tables are local to the object and reached
//! only by pointer: two Plenty static libraries linked into one program each
//! keep their own.
//!
//! Layout, matching `Site` in the runtime: one sixteen-byte record per distinct
//! position (`line`, `column`, `file`, `standard_library`, each a `u32`), then the file names, each
//! a `u32` length followed by UTF-8 bytes. `file` is the distance in bytes from
//! its record to its name, so the table needs no relocations.

use super::*;
use cranelift_codegen::ir::Value;

const RECORD: usize = 16;

pub(super) type Key = (Option<Arc<str>>, u32, u32, bool);

/// The positions one function refers to, in first-use order.
#[derive(Default)]
pub(super) struct Sites {
    offsets: HashMap<Key, i64>,
    records: Vec<Key>,
}

#[cfg(test)]
mod tests;

impl Sites {
    /// Offset of `key`'s record in the table.
    pub(super) fn record(&mut self, key: Key) -> i64 {
        if let Some(offset) = self.offsets.get(&key) {
            return *offset;
        }
        let offset = (self.records.len() * RECORD) as i64;
        self.offsets.insert(key.clone(), offset);
        self.records.push(key);
        offset
    }

    pub(super) fn into_records(self) -> Vec<Key> {
        self.records
    }
}

/// Define a table of `records` in `module`.
pub(super) fn define(module: &mut ObjectModule, records: &[Key]) -> Result<DataId> {
    let names_start = records.len() * RECORD;
    let mut names = Vec::new();
    let mut name_offsets: HashMap<&str, usize> = HashMap::default();
    let mut bytes = Vec::with_capacity(names_start);
    for (i, (file, line, column, standard_library)) in records.iter().enumerate() {
        let file = match file {
            None => 0,
            Some(name) => {
                let at = *name_offsets.entry(name).or_insert_with(|| {
                    let at = names.len();
                    names.extend((name.len() as u32).to_ne_bytes());
                    names.extend(name.as_bytes());
                    at
                });
                u32::try_from(names_start + at - i * RECORD)?
            }
        };
        for field in [*line, *column, file, u32::from(*standard_library)] {
            bytes.extend(field.to_ne_bytes());
        }
    }
    bytes.extend(names);
    let mut description = DataDescription::new();
    description.define(bytes.into_boxed_slice());
    description.set_align(4);
    let data = module.declare_anonymous_data(false, false)?;
    module.define_data(data, &description)?;
    Ok(data)
}

/// Where the operation being lowered is in the source.
#[derive(Default)]
pub(super) struct Position {
    /// File of the function's body: for a generic instance or an imported
    /// function, the file that defines it.
    file: Option<Arc<str>>,
    standard_library: bool,
    /// Line and column of the last `Op::Site` marker.
    at: Option<(u32, u32)>,
    call_at: Option<(u32, u32)>,
    /// In a cleanup block shared by several exits, the variables holding the
    /// trap and caller records of the exit taken, until a marker in the block
    /// fixes the position (see [`super::exits`]).
    shared_at: Option<Variable>,
    shared_call: Option<Variable>,
}

impl Position {
    pub(super) fn in_file(file: Option<Arc<str>>, standard_library: bool) -> Self {
        Self {
            file,
            standard_library,
            ..Default::default()
        }
    }

    pub(super) fn set(&mut self, site: &crate::ownership::Site) {
        self.at = Some((site.line, site.column));
        self.call_at = None;
        self.shared_at = None;
        self.shared_call = None;
    }

    pub(super) fn set_call(&mut self, site: &crate::ownership::Site) {
        self.call_at = Some((site.line, site.column));
        self.shared_call = None;
    }

    /// Read the position from `trap` and `call` until the next marker.
    pub(super) fn share(&mut self, trap: Variable, call: Variable) {
        self.shared_at = Some(trap);
        self.shared_call = Some(call);
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn in_standard_library(&self) -> bool {
        self.position.standard_library
    }
    /// Pointer to the current position's record for a runtime helper that may
    /// trap. Null before the first marker: the token front end and compiler
    /// adapters have no position, and the runtime then prints the message alone.
    pub(super) fn trap_site(&mut self) -> Result<Value> {
        if let Some(trap) = self.position.shared_at {
            return Ok(self.bcx.use_var(trap));
        }
        self.site_record(self.position.at)
    }

    /// The record a user-code call names as its caller.
    pub(super) fn call_site(&mut self) -> Result<Value> {
        if let Some(call) = self.position.shared_call {
            return Ok(self.bcx.use_var(call));
        }
        self.site_record(self.position.call_at.or(self.position.at))
    }

    fn site_record(&mut self, at: Option<(u32, u32)>) -> Result<Value> {
        let Some((line, column)) = at else {
            return Ok(self.bcx.ins().iconst(PTR_TY, 0));
        };
        let key = (
            self.position.file.clone(),
            line,
            column,
            self.position.standard_library,
        );
        let (table, offset) = self.module.site(key, self.bcx.func);
        let base = self.bcx.ins().global_value(PTR_TY, table);
        Ok(if offset == 0 {
            base
        } else {
            self.bcx.ins().iadd_imm(base, offset)
        })
    }

    /// Saves the caller context and, in user code, names this call site as
    /// the caller. A normal call restores the context on return; a user tail
    /// call transfers it without growing a stack. In std the site is null, so
    /// the save matters only when [`Self::leave_call`] follows a callee that
    /// may change the context; see [`mark_context_preserving`].
    pub(super) fn enter_call(&mut self) -> Result<Option<Value>> {
        if !self.runtime.standard_calls {
            return Ok(None);
        }
        let site = if self.position.standard_library {
            self.bcx.ins().iconst(PTR_TY, 0)
        } else {
            self.call_site()?
        };
        let enter = self
            .module
            .declare_func_in_func(self.runtime.caller_enter, self.bcx.func);
        let call = self.bcx.ins().call(enter, &[site]);
        Ok(Some(self.bcx.inst_results(call)[0]))
    }

    pub(super) fn leave_call(&mut self, saved: Option<Value>) {
        if let Some(saved) = saved {
            let leave = self
                .module
                .declare_func_in_func(self.runtime.caller_leave, self.bcx.func);
            self.bcx.ins().call(leave, &[saved]);
        }
    }
}

/// Marks each function that returns with the caller context it was entered
/// with, so a standard-library call to it needs no save and restore.
///
/// Only a call that sets a site and then gives up its frame leaves the context
/// changed: a user tail call into std or an adapter, or any indirect tail call
/// (from std its callee is unknown). Every other operation preserves it:
/// - an ordinary call from user code into std or an adapter, and an indirect
///   call, save and restore around the callee;
/// - runtime helpers restore it after each callback into compiled code
///   (`CallerScope` in the runtime);
/// - a direct tail call, a user-to-user call, or a std call to a preserving
///   callee preserves it exactly when that callee does.
///
/// The result is the greatest fixed point, so recursion through preserving
/// functions stays preserving. Generator constructors and functions without
/// a source file are left unmarked.
pub(super) fn mark_context_preserving(
    bodies: &[(String, Arc<[Op]>)],
    fns: &mut HashMap<String, UserFn>,
) {
    let index: HashMap<&str, usize> = bodies
        .iter()
        .enumerate()
        .map(|(i, (name, _))| (name.as_str(), i))
        .collect();
    let mut preserving = vec![true; bodies.len()];
    // `callers[g]`: functions that preserve the context only if `g` does.
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); bodies.len()];
    for (i, (name, body)) in bodies.iter().enumerate() {
        let caller = &fns[name];
        let mut depends = Vec::new();
        let mut changes = caller.generator.is_some() || caller.source.is_none();
        walk_body(body, &mut |op| match op {
            Op::TailCallIndirect(_) => changes = true,
            Op::TailCall(callee) | Op::Call(callee) => {
                let Some(&g) = index.get(callee.as_str()) else {
                    changes = true;
                    return;
                };
                let decl = &fns[callee];
                let sets_site =
                    !caller.standard_library && (decl.standard_library || decl.source.is_none());
                match op {
                    Op::TailCall(_) if sets_site => changes = true,
                    // Restored by its caller: std restores unless the callee
                    // preserves, user code whenever it sets a site.
                    Op::Call(_) if caller.standard_library || sets_site => {}
                    _ => depends.push(g),
                }
            }
            _ => {}
        });
        if changes {
            preserving[i] = false;
        }
        for g in depends {
            callers[g].push(i);
        }
    }
    let mut pending: Vec<usize> = (0..bodies.len()).filter(|&i| !preserving[i]).collect();
    while let Some(g) = pending.pop() {
        for &f in &callers[g] {
            if preserving[f] {
                preserving[f] = false;
                pending.push(f);
            }
        }
    }
    for ((name, _), preserves) in bodies.iter().zip(preserving) {
        fns.get_mut(name).unwrap().preserves_caller = preserves;
    }
}

/// Visits every operation of one function, not of the functions it defines.
fn walk_body<'a>(ops: &'a [Op], visit: &mut impl FnMut(&'a Op)) {
    let mut pending = vec![ops];
    while let Some(ops) = pending.pop() {
        for op in ops {
            visit(op);
            match op {
                Op::Match(arms) => pending.extend(arms.iter().map(|arm| arm.body.as_ref())),
                Op::Loop { condition, body } => pending.extend([condition.as_ref(), body.as_ref()]),
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    pending.extend([then.as_ref(), otherwise.as_ref()]);
                    for arm in elifs.iter() {
                        pending.extend([arm.condition.as_ref(), arm.body.as_ref()]);
                    }
                }
                Op::Try { cleanup, .. } => pending.push(cleanup),
                _ => {}
            }
        }
    }
}
