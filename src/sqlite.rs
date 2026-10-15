//! Native primitives behind `std.db.sqlite`: the vendored SQLite library in
//! the runtime. Typed reads, the error model and transaction ownership live in
//! `std.db.sqlite`; see book/src/design/stdlib/sqlite.md.
use crate::effects::Effect;
use crate::{op::Ty, sum};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SqliteOp {
    Open,
    Message,
    Execute,
    Prepare,
    Changes,
    LastRowid,
    BusyTimeout,
    Close,
    Begin,
    Commit,
    Rollback,
    Step,
    Reset,
    ClearBindings,
    ParameterCount,
    ParameterIndex,
    BindNull,
    BindInteger,
    BindReal,
    BindText,
    BindBlob,
    ColumnCount,
    ColumnType,
    ColumnInteger,
    ColumnReal,
    ColumnBytes,
    ColumnText,
    ColumnBlob,
    ColumnName,
}

impl SqliteOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__sqlite_open" => Self::Open,
            "__sqlite_message" => Self::Message,
            "__sqlite_execute" => Self::Execute,
            "__sqlite_prepare" => Self::Prepare,
            "__sqlite_changes" => Self::Changes,
            "__sqlite_last_rowid" => Self::LastRowid,
            "__sqlite_busy_timeout" => Self::BusyTimeout,
            "__sqlite_close" => Self::Close,
            "__sqlite_begin" => Self::Begin,
            "__sqlite_commit" => Self::Commit,
            "__sqlite_rollback" => Self::Rollback,
            "__sqlite_step" => Self::Step,
            "__sqlite_reset" => Self::Reset,
            "__sqlite_clear_bindings" => Self::ClearBindings,
            "__sqlite_parameter_count" => Self::ParameterCount,
            "__sqlite_parameter_index" => Self::ParameterIndex,
            "__sqlite_bind_null" => Self::BindNull,
            "__sqlite_bind_integer" => Self::BindInteger,
            "__sqlite_bind_real" => Self::BindReal,
            "__sqlite_bind_text" => Self::BindText,
            "__sqlite_bind_blob" => Self::BindBlob,
            "__sqlite_column_count" => Self::ColumnCount,
            "__sqlite_column_type" => Self::ColumnType,
            "__sqlite_column_integer" => Self::ColumnInteger,
            "__sqlite_column_real" => Self::ColumnReal,
            "__sqlite_column_bytes" => Self::ColumnBytes,
            "__sqlite_column_text" => Self::ColumnText,
            "__sqlite_column_blob" => Self::ColumnBlob,
            "__sqlite_column_name" => Self::ColumnName,
            _ => return None,
        })
    }

    /// Handles are byte storage owned by `std.db.sqlite`. An `i64` result is
    /// a SQLite result code (0 is success; 100 and 101 are a row and the
    /// end of a statement) or one of the module's negative codes.
    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let handle = || Ty::ByteStorage;
        let opened = || sum::result(handle(), Ty::I64);
        match self {
            // Path, open flags, value length limit.
            Self::Open => (vec![Ty::Str, Ty::I64, Ty::I64], opened()),
            Self::Message => (vec![], sum::result(Ty::Str, sum::alloc_error())),
            Self::Execute | Self::ParameterIndex => (vec![handle(), Ty::Str], Ty::I64),
            Self::Prepare => (vec![handle(), Ty::Str], opened()),
            Self::Begin => (vec![handle(), Ty::I64], opened()),
            Self::Changes
            | Self::LastRowid
            | Self::Close
            | Self::Commit
            | Self::Rollback
            | Self::Step
            | Self::Reset
            | Self::ClearBindings
            | Self::ParameterCount
            | Self::ColumnCount => (vec![handle()], Ty::I64),
            Self::BusyTimeout
            | Self::BindNull
            | Self::ColumnType
            | Self::ColumnInteger
            | Self::ColumnBytes => (vec![handle(), Ty::I64], Ty::I64),
            Self::BindInteger => (vec![handle(), Ty::I64, Ty::I64], Ty::I64),
            Self::BindReal => (vec![handle(), Ty::I64, Ty::F64], Ty::I64),
            Self::BindText => (vec![handle(), Ty::I64, Ty::Str], Ty::I64),
            Self::BindBlob => (vec![handle(), Ty::I64, handle()], Ty::I64),
            Self::ColumnReal => (vec![handle(), Ty::I64], Ty::F64),
            Self::ColumnText | Self::ColumnName => {
                (vec![handle(), Ty::I64], sum::result(Ty::Str, Ty::I64))
            }
            Self::ColumnBlob => (
                vec![handle(), Ty::I64],
                sum::result(handle(), sum::alloc_error()),
            ),
        }
    }

    /// Opening resolves a path; everything else acts through a handle the
    /// caller owns or borrows. SQLite runs in serialized mode, so a handle
    /// moved to a worker stays safe to use there.
    pub fn effect(self) -> Effect {
        match self {
            Self::Open => Effect::Path,
            Self::Message => Effect::Compute,
            Self::Execute
            | Self::Prepare
            | Self::Changes
            | Self::LastRowid
            | Self::BusyTimeout
            | Self::Close
            | Self::Begin
            | Self::Commit
            | Self::Rollback
            | Self::Step
            | Self::Reset
            | Self::ClearBindings
            | Self::ParameterCount
            | Self::ParameterIndex
            | Self::BindNull
            | Self::BindInteger
            | Self::BindReal
            | Self::BindText
            | Self::BindBlob
            | Self::ColumnCount
            | Self::ColumnType
            | Self::ColumnInteger
            | Self::ColumnReal
            | Self::ColumnBytes
            | Self::ColumnText
            | Self::ColumnBlob
            | Self::ColumnName => Effect::Handle,
        }
    }

    pub fn opcode(self) -> i64 {
        440 + self as i64
    }
}
