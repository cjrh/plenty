//! Source positions for runtime traps.
//!
//! An operation that can trap passes the runtime a pointer to a record in one
//! read-only table per object. The runtime reads the record only when the
//! trap fires, so the passing path costs one address computation.
//!
//! The table is local to the object and reached only by pointer: two Plenty
//! static libraries linked into one program each keep their own.
//!
//! Layout, matching `Site` in the runtime: one sixteen-byte record per distinct
//! position (`line`, `column`, `file`, `standard_library`, each a `u32`), then the file names, each
//! a `u32` length followed by UTF-8 bytes. `file` is the distance in bytes from
//! its record to its name, so the table needs no relocations.

use super::*;
use cranelift_codegen::ir::{GlobalValue, Value};

const RECORD: usize = 16;

type Key = (Option<Arc<str>>, u32, u32, bool);

/// The positions an object's functions refer to, in first-use order. Bodies
/// are lowered one at a time in declaration order, so the order is the same
/// for every build of a program.
#[derive(Default)]
pub(super) struct Sites {
    data: Option<DataId>,
    offsets: HashMap<Key, i64>,
    records: Vec<Key>,
}

#[cfg(test)]
mod tests;

impl Sites {
    fn record(&mut self, module: &mut ObjectModule, key: Key) -> Result<(DataId, i64)> {
        let data = match self.data {
            Some(data) => data,
            None => {
                let data =
                    module.declare_data("plenty_trap_sites", Linkage::Local, false, false)?;
                self.data = Some(data);
                data
            }
        };
        let offset = match self.offsets.get(&key) {
            Some(offset) => *offset,
            None => {
                let offset = (self.records.len() * RECORD) as i64;
                self.offsets.insert(key.clone(), offset);
                self.records.push(key);
                offset
            }
        };
        Ok((data, offset))
    }

    /// Emit the table. Call once, after every body is lowered.
    pub(super) fn define(&self, module: &mut ObjectModule) -> Result<()> {
        let Some(data) = self.data else {
            return Ok(());
        };
        let names_start = self.records.len() * RECORD;
        let mut names = Vec::new();
        let mut name_offsets: HashMap<&str, usize> = HashMap::new();
        let mut bytes = Vec::with_capacity(names_start);
        for (i, (file, line, column, standard_library)) in self.records.iter().enumerate() {
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
        module.define_data(data, &description)?;
        Ok(())
    }
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
    table: Option<GlobalValue>,
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
    }

    pub(super) fn set_call(&mut self, site: &crate::ownership::Site) {
        self.call_at = Some((site.line, site.column));
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
        self.site_record(self.position.at)
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
        let (data, offset) = self.runtime.sites.borrow_mut().record(self.module, key)?;
        let table = match self.position.table {
            Some(table) => table,
            None => {
                let table = self.module.declare_data_in_func(data, self.bcx.func);
                self.position.table = Some(table);
                table
            }
        };
        let base = self.bcx.ins().global_value(PTR_TY, table);
        Ok(if offset == 0 {
            base
        } else {
            self.bcx.ins().iadd_imm(base, offset)
        })
    }

    /// A normal call restores its caller context on return, including calls
    /// made by std to user callbacks. Tail calls transfer the context without
    /// growing a stack. Null preserves context for std and synthetic adapters.
    pub(super) fn enter_call(&mut self) -> Result<Option<Value>> {
        if !self.runtime.standard_calls {
            return Ok(None);
        }
        let site = if self.position.standard_library {
            self.bcx.ins().iconst(PTR_TY, 0)
        } else {
            self.site_record(self.position.call_at.or(self.position.at))?
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
