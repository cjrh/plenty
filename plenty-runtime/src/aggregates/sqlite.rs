//! Native back end of `std.db.sqlite`: the vendored SQLite amalgamation,
//! compiled by the build script and bundled into the runtime archive.
//!
//! Connections, statements and transactions are runtime objects whose prefix
//! is empty byte storage, so `std.db.sqlite` holds them as `ByteStorage`
//! values with ordinary reference counting. Statements and transactions keep
//! their connection alive; closing a connection that still has either is
//! refused. Every operation checks the object's destructor and stops the
//! process on any other storage.
//!
//! SQLite is built thread-safe and every connection is opened in serialized
//! mode, so a handle moved to a worker thread stays safe to use. A failing
//! call's message is copied, under the connection's mutex, into a buffer of
//! the calling thread, where `std.db.sqlite` reads it.
//!
//! Values are copied out; no pointer into SQLite's row buffers crosses the
//! ABI, so a later step or reset cannot invalidate anything Plenty holds.
use super::byte_storage::{from_slice, result, Storage};
use super::wrap;
use crate::memory::{self, plenty_release, plenty_retain, AllocError, Header};
use crate::strings::{self, Text};
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

type Db = c_void;
type Stmt = c_void;

extern "C" {
    fn sqlite3_open_v2(
        name: *const c_char,
        db: *mut *mut Db,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close(db: *mut Db) -> c_int;
    fn sqlite3_close_v2(db: *mut Db) -> c_int;
    fn sqlite3_errmsg(db: *mut Db) -> *const c_char;
    fn sqlite3_errstr(code: c_int) -> *const c_char;
    fn sqlite3_db_mutex(db: *mut Db) -> *mut c_void;
    fn sqlite3_mutex_enter(mutex: *mut c_void);
    fn sqlite3_mutex_leave(mutex: *mut c_void);
    fn sqlite3_limit(db: *mut Db, id: c_int, value: c_int) -> c_int;
    fn sqlite3_busy_timeout(db: *mut Db, ms: c_int) -> c_int;
    fn sqlite3_changes64(db: *mut Db) -> i64;
    fn sqlite3_last_insert_rowid(db: *mut Db) -> i64;
    fn sqlite3_get_autocommit(db: *mut Db) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut Db,
        sql: *const c_char,
        bytes: c_int,
        stmt: *mut *mut Stmt,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: *mut Stmt) -> c_int;
    fn sqlite3_step(stmt: *mut Stmt) -> c_int;
    fn sqlite3_reset(stmt: *mut Stmt) -> c_int;
    fn sqlite3_clear_bindings(stmt: *mut Stmt) -> c_int;
    fn sqlite3_db_handle(stmt: *mut Stmt) -> *mut Db;
    fn sqlite3_bind_parameter_count(stmt: *mut Stmt) -> c_int;
    fn sqlite3_bind_parameter_index(stmt: *mut Stmt, name: *const c_char) -> c_int;
    fn sqlite3_bind_null(stmt: *mut Stmt, index: c_int) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut Stmt, index: c_int, value: i64) -> c_int;
    fn sqlite3_bind_double(stmt: *mut Stmt, index: c_int, value: f64) -> c_int;
    fn sqlite3_bind_text64(
        stmt: *mut Stmt,
        index: c_int,
        text: *const c_char,
        bytes: u64,
        destructor: isize,
        encoding: u8,
    ) -> c_int;
    fn sqlite3_bind_blob64(
        stmt: *mut Stmt,
        index: c_int,
        blob: *const c_void,
        bytes: u64,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_column_count(stmt: *mut Stmt) -> c_int;
    fn sqlite3_column_type(stmt: *mut Stmt, column: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut Stmt, column: c_int) -> i64;
    fn sqlite3_column_double(stmt: *mut Stmt, column: c_int) -> f64;
    fn sqlite3_column_text(stmt: *mut Stmt, column: c_int) -> *const u8;
    fn sqlite3_column_blob(stmt: *mut Stmt, column: c_int) -> *const c_void;
    fn sqlite3_column_bytes(stmt: *mut Stmt, column: c_int) -> c_int;
    fn sqlite3_column_name(stmt: *mut Stmt, column: c_int) -> *const c_char;
}

const OK: c_int = 0;
const NOMEM: c_int = 7;
const TOOBIG: c_int = 18;
const NULL: c_int = 5;
const ROW: c_int = 100;
const DONE: c_int = 101;
const OPEN_FULLMUTEX: c_int = 0x0001_0000;
const OPEN_EXRESCODE: c_int = 0x0200_0000;
const OPEN_ACCESS: c_int = 0x1 | 0x2 | 0x4;
const LIMIT_LENGTH: c_int = 0;
/// SQLite copies bound values before the bind call returns.
const TRANSIENT: isize = -1;
const UTF8: u8 = 1;

/// The module's own failures, kept apart from SQLite's positive codes.
const TRAILING_SQL: i64 = -1;
const EMPTY_SQL: i64 = -2;
const TRANSACTION_ACTIVE: i64 = -3;
const INVALID_UTF8: i64 = -4;
const IN_USE: i64 = -5;
const CLOSED: i64 = -6;
const TRANSACTION_FINISHED: i64 = -7;
const INVALID_PATH: i64 = -8;

const MESSAGE_CAPACITY: usize = 512;

thread_local! {
    /// The last failure's message on this thread, truncated to a character
    /// boundary. Fixed storage keeps error reporting free of allocation.
    static MESSAGE: RefCell<([u8; MESSAGE_CAPACITY], usize)> =
        const { RefCell::new(([0; MESSAGE_CAPACITY], 0)) };
}

fn remember(text: &[u8]) {
    let mut end = text.len().min(MESSAGE_CAPACITY);
    while end > 0 && std::str::from_utf8(&text[..end]).is_err() {
        end -= 1;
    }
    MESSAGE.with(|message| {
        let mut message = message.borrow_mut();
        message.0[..end].copy_from_slice(&text[..end]);
        message.1 = end;
    });
}

/// Records the connection's message for a failing code; returns the code.
/// The caller holds the connection's mutex, so the message is this call's.
unsafe fn checked(db: *mut Db, code: c_int) -> i64 {
    if !matches!(code, OK | ROW | DONE) {
        // SAFETY: `db` is a live connection, or null for SQLite's generic
        // text; both calls return static or connection-owned C strings.
        let text = unsafe {
            if db.is_null() {
                CStr::from_ptr(sqlite3_errstr(code))
            } else {
                CStr::from_ptr(sqlite3_errmsg(db))
            }
        };
        remember(text.to_bytes());
    }
    code as i64
}

/// Runs `f` holding the connection's mutex, so that no other thread's call
/// replaces the error message before it is copied.
unsafe fn locked<R>(db: *mut Db, f: impl FnOnce() -> R) -> R {
    // SAFETY: `db` is a live connection in serialized mode, whose mutex is
    // recursive and outlives the connection's calls.
    unsafe {
        let mutex = sqlite3_db_mutex(db);
        sqlite3_mutex_enter(mutex);
        let out = f();
        sqlite3_mutex_leave(mutex);
        out
    }
}

#[repr(C)]
struct Connection {
    storage: Storage,
    /// Null once closed.
    db: *mut Db,
    /// Live statements and transactions; closing waits for none.
    users: AtomicUsize,
    /// The generation of the transaction object that owns the open
    /// transaction, or 0.
    transaction: AtomicU64,
    generations: AtomicU64,
}

#[repr(C)]
struct Statement {
    storage: Storage,
    stmt: *mut Stmt,
    /// A retained reference to the owning connection.
    connection: *mut Connection,
}

#[repr(C)]
struct Transaction {
    storage: Storage,
    connection: *mut Connection,
    generation: u64,
}

fn empty_storage(destroy: unsafe extern "C" fn(*mut Header)) -> Storage {
    Storage {
        header: Header::new(destroy),
        bytes: Vec::new(),
    }
}

unsafe extern "C" fn destroy_connection(pointer: *mut Header) {
    // SAFETY: the last owner has gone; no statement or transaction retains
    // the connection, so nothing else uses `db`.
    unsafe {
        let connection = pointer.cast::<Connection>();
        if !(*connection).db.is_null() {
            sqlite3_close_v2((*connection).db);
        }
        ptr::drop_in_place(ptr::addr_of_mut!((*connection).storage.bytes));
        memory::free::<Connection, u8>(connection, 0);
    }
}

unsafe extern "C" fn destroy_statement(pointer: *mut Header) {
    // SAFETY: the last owner has gone; the statement is finalized once and
    // its retained connection released after it.
    unsafe {
        let statement = pointer.cast::<Statement>();
        sqlite3_finalize((*statement).stmt);
        let connection = (*statement).connection;
        ptr::drop_in_place(ptr::addr_of_mut!((*statement).storage.bytes));
        memory::free::<Statement, u8>(statement, 0);
        (*connection).users.fetch_sub(1, Ordering::Release);
        plenty_release(connection.cast());
    }
}

unsafe extern "C" fn destroy_transaction(pointer: *mut Header) {
    // SAFETY: the last owner has gone. A transaction still marked active
    // rolls back, so dropping one never commits.
    unsafe {
        let transaction = pointer.cast::<Transaction>();
        let connection = (*transaction).connection;
        let generation = (*transaction).generation;
        if (*connection).transaction.load(Ordering::Acquire) == generation {
            finish(connection, generation, c"ROLLBACK");
        }
        ptr::drop_in_place(ptr::addr_of_mut!((*transaction).storage.bytes));
        memory::free::<Transaction, u8>(transaction, 0);
        (*connection).users.fetch_sub(1, Ordering::Release);
        plenty_release(connection.cast());
    }
}

/// The object behind a handle operand, or a stopped process for any other.
unsafe fn object<T>(value: u128, destroy: unsafe extern "C" fn(*mut Header), what: &str) -> *mut T {
    // SAFETY: every byte-storage-typed operand starts with a live `Header`.
    let owner = unsafe { (*(value as *const Header)).destroy };
    if owner.map(|f| f as usize) != Some(destroy as usize) {
        crate::fail(&format!("operand is not a std.db.sqlite {what}"));
    }
    value as *mut T
}

unsafe fn connection(value: u128) -> *mut Connection {
    unsafe { object(value, destroy_connection, "connection") }
}

unsafe fn statement(value: u128) -> *mut Stmt {
    unsafe { (*object::<Statement>(value, destroy_statement, "statement")).stmt }
}

/// A NUL-terminated copy of a text operand, or `None` if it contains NUL.
unsafe fn c_text(value: u128) -> Result<Option<Vec<u8>>, AllocError> {
    let text = value as *const Text;
    // SAFETY: the compiler lends a live text for the call.
    let bytes = unsafe { strings::bytes(&text) };
    if bytes.contains(&0) {
        return Ok(None);
    }
    let mut out = Vec::new();
    memory::try_reserve(&mut out, bytes.len() + 1)?;
    out.extend_from_slice(bytes);
    out.push(0);
    Ok(Some(out))
}

fn opened(value: Result<u128, i64>) -> u128 {
    match value {
        Ok(value) => wrap(value, 0),
        Err(code) => wrap(code as u64 as u128, 1),
    }
}

fn allocation(error: AllocError) -> i64 {
    remember(match error {
        AllocError::OutOfMemory => b"out of memory",
        AllocError::CapacityOverflow => b"allocation size overflow",
    });
    NOMEM as i64
}

/// `std.db.sqlite` has rejected empty and in-memory paths for `connect`;
/// its `memory` constructor passes `:memory:` itself.
unsafe fn open(path: u128, flags: i64, limit: i64) -> Result<u128, i64> {
    let path = match unsafe { c_text(path) } {
        Ok(Some(path)) => path,
        Ok(None) => {
            remember(b"path contains NUL");
            return Err(INVALID_PATH);
        }
        Err(error) => return Err(allocation(error)),
    };
    let flags = (flags as c_int & OPEN_ACCESS) | OPEN_FULLMUTEX | OPEN_EXRESCODE;
    let owner = memory::try_allocate::<Connection, u8>(0).map_err(allocation)?;
    // SAFETY: fresh, exclusively owned memory of `Connection`'s layout; once
    // written, `destroy_connection` closes whatever was opened.
    unsafe {
        owner.write(Connection {
            storage: empty_storage(destroy_connection),
            db: ptr::null_mut(),
            users: AtomicUsize::new(0),
            transaction: AtomicU64::new(0),
            generations: AtomicU64::new(0),
        });
        let mut db = ptr::null_mut();
        let code = sqlite3_open_v2(path.as_ptr().cast(), &mut db, flags, ptr::null());
        (*owner).db = db;
        if code != OK {
            let code = checked(db, code);
            destroy_connection(owner.cast());
            return Err(code);
        }
        sqlite3_limit(db, LIMIT_LENGTH, limit.clamp(1, i32::MAX as i64) as c_int);
    }
    Ok(owner as u128)
}

/// The live database of a connection operand, or `CLOSED`.
unsafe fn database(value: u128) -> Result<*mut Db, i64> {
    // SAFETY: `connection` validates the operand.
    let db = unsafe { (*connection(value)).db };
    if db.is_null() {
        remember(b"connection is closed");
        return Err(CLOSED);
    }
    Ok(db)
}

/// Compiles the first statement of `sql`. `Ok(None)` means it held only
/// whitespace and comments.
unsafe fn compile(db: *mut Db, sql: &[u8]) -> Result<Option<(*mut Stmt, usize)>, i64> {
    let Ok(length) = c_int::try_from(sql.len()) else {
        remember(b"SQL text is too long");
        return Err(TOOBIG as i64);
    };
    let mut stmt = ptr::null_mut();
    let mut tail = ptr::null();
    let start = sql.as_ptr().cast::<c_char>();
    // SAFETY: `sql` is live for the call; SQLite reads at most `length` bytes.
    let code = unsafe { sqlite3_prepare_v2(db, start, length, &mut stmt, &mut tail) };
    if code != OK {
        return Err(unsafe { checked(db, code) });
    }
    if stmt.is_null() {
        return Ok(None);
    }
    // SAFETY: SQLite's tail points into `sql`, at or after `start`.
    let used = unsafe { tail.offset_from(start) } as usize;
    Ok(Some((stmt, used)))
}

unsafe fn execute(handle: u128, sql: u128) -> i64 {
    let db = match unsafe { database(handle) } {
        Ok(db) => db,
        Err(code) => return code,
    };
    let text = sql as *const Text;
    // SAFETY: the compiler lends a live text for the call.
    let mut sql = unsafe { strings::bytes(&text) };
    unsafe {
        locked(db, || loop {
            let (stmt, used) = match compile(db, sql) {
                Ok(Some(compiled)) => compiled,
                Ok(None) => return OK as i64,
                Err(code) => return code,
            };
            sql = &sql[used..];
            let mut code = sqlite3_step(stmt);
            while code == ROW {
                code = sqlite3_step(stmt);
            }
            let code = if code == DONE {
                OK as i64
            } else {
                checked(db, code)
            };
            sqlite3_finalize(stmt);
            if code != OK as i64 {
                return code;
            }
        })
    }
}

unsafe fn prepare(handle: u128, sql: u128) -> Result<u128, i64> {
    let db = unsafe { database(handle) }?;
    let connection = unsafe { connection(handle) };
    let text = sql as *const Text;
    // SAFETY: the compiler lends a live text for the call.
    let sql = unsafe { strings::bytes(&text) };
    let owner = memory::try_allocate::<Statement, u8>(0).map_err(allocation)?;
    // SAFETY: the statement is finalized on every path that does not hand it
    // to its owner object; `owner` is fresh and freed on failure.
    unsafe {
        let compiled = locked(db, || {
            let Some((stmt, used)) = compile(db, sql)? else {
                remember(b"SQL text contains no statement");
                return Err(EMPTY_SQL);
            };
            match compile(db, &sql[used..]) {
                Ok(None) => Ok(stmt),
                Ok(Some((rest, _))) => {
                    sqlite3_finalize(rest);
                    sqlite3_finalize(stmt);
                    remember(b"SQL text holds more than one statement");
                    Err(TRAILING_SQL)
                }
                Err(code) => {
                    sqlite3_finalize(stmt);
                    Err(code)
                }
            }
        });
        let stmt = match compiled {
            Ok(stmt) => stmt,
            Err(code) => {
                memory::free::<Statement, u8>(owner, 0);
                return Err(code);
            }
        };
        plenty_retain(connection.cast());
        (*connection).users.fetch_add(1, Ordering::Acquire);
        owner.write(Statement {
            storage: empty_storage(destroy_statement),
            stmt,
            connection,
        });
    }
    Ok(owner as u128)
}

unsafe fn close(handle: u128) -> i64 {
    // SAFETY: `connection` validates the operand; `std.db.sqlite` holds the
    // connection exclusively for the call.
    unsafe {
        let connection = connection(handle);
        let db = (*connection).db;
        if db.is_null() {
            return OK as i64;
        }
        if (*connection).users.load(Ordering::Acquire) != 0 {
            remember(b"connection has live statements or transactions");
            return IN_USE;
        }
        let code = locked(db, || match sqlite3_close(db) {
            OK => OK as i64,
            code => checked(db, code),
        });
        if code == OK as i64 {
            (*connection).db = ptr::null_mut();
        }
        code
    }
}

const BEGIN: [&CStr; 3] = [c"BEGIN DEFERRED", c"BEGIN IMMEDIATE", c"BEGIN EXCLUSIVE"];

unsafe fn begin(handle: u128, mode: i64) -> Result<u128, i64> {
    let db = unsafe { database(handle) }?;
    let connection = unsafe { connection(handle) };
    let sql = BEGIN
        .get(mode as usize)
        .unwrap_or_else(|| crate::fail("unknown std.db.sqlite transaction mode"));
    let owner = memory::try_allocate::<Transaction, u8>(0).map_err(allocation)?;
    // SAFETY: `owner` is fresh and freed when BEGIN fails.
    unsafe {
        let generation = locked(db, || {
            if (*connection).transaction.load(Ordering::Acquire) != 0
                || sqlite3_get_autocommit(db) == 0
            {
                remember(b"a transaction is already active");
                return Err(TRANSACTION_ACTIVE);
            }
            let code = run(db, sql);
            if code != OK as i64 {
                return Err(code);
            }
            let generation = (*connection).generations.fetch_add(1, Ordering::Relaxed) + 1;
            (*connection)
                .transaction
                .store(generation, Ordering::Release);
            Ok(generation)
        });
        let generation = match generation {
            Ok(generation) => generation,
            Err(code) => {
                memory::free::<Transaction, u8>(owner, 0);
                return Err(code);
            }
        };
        plenty_retain(connection.cast());
        (*connection).users.fetch_add(1, Ordering::Acquire);
        owner.write(Transaction {
            storage: empty_storage(destroy_transaction),
            connection,
            generation,
        });
    }
    Ok(owner as u128)
}

/// One statement without parameters or rows, under the caller's lock.
unsafe fn run(db: *mut Db, sql: &CStr) -> i64 {
    unsafe {
        let mut stmt = ptr::null_mut();
        let code = sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, ptr::null_mut());
        if code != OK {
            return checked(db, code);
        }
        let code = match sqlite3_step(stmt) {
            DONE | ROW => OK as i64,
            code => checked(db, code),
        };
        sqlite3_finalize(stmt);
        code
    }
}

/// Commits or rolls back the transaction `generation` if it is still the
/// connection's open transaction. A busy COMMIT leaves it open to retry;
/// any other outcome ends it.
unsafe fn finish(connection: *mut Connection, generation: u64, sql: &CStr) -> i64 {
    unsafe {
        let db = (*connection).db;
        locked(db, || {
            if (*connection).transaction.load(Ordering::Acquire) != generation {
                remember(b"transaction already finished");
                return TRANSACTION_FINISHED;
            }
            if sqlite3_get_autocommit(db) != 0 {
                (*connection).transaction.store(0, Ordering::Release);
                remember(b"transaction was ended by other SQL");
                return TRANSACTION_FINISHED;
            }
            let code = run(db, sql);
            if sqlite3_get_autocommit(db) != 0 {
                (*connection).transaction.store(0, Ordering::Release);
            }
            code
        })
    }
}

unsafe fn transaction(value: u128) -> *mut Transaction {
    unsafe { object(value, destroy_transaction, "transaction") }
}

fn index(value: u128) -> c_int {
    c_int::try_from(value as i64).unwrap_or(c_int::MAX)
}

/// Binding to an out-of-range index is SQLite's RANGE error (25).
unsafe fn bind(stmt: *mut Stmt, f: impl FnOnce() -> c_int) -> i64 {
    unsafe {
        let db = sqlite3_db_handle(stmt);
        locked(db, || checked(db, f()))
    }
}

unsafe fn step(stmt: *mut Stmt) -> i64 {
    unsafe {
        let db = sqlite3_db_handle(stmt);
        locked(db, || checked(db, sqlite3_step(stmt)))
    }
}

unsafe fn column_bytes(stmt: *mut Stmt, column: c_int) -> &'static [u8] {
    unsafe {
        let data = sqlite3_column_blob(stmt, column).cast::<u8>();
        let length = sqlite3_column_bytes(stmt, column).max(0) as usize;
        if data.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(data, length)
        }
    }
}

fn text_result(text: Result<&str, i64>) -> u128 {
    match text.and_then(|text| strings::try_new(text).map_err(allocation)) {
        Ok(text) => wrap(text as u128, 0),
        Err(code) => wrap(code as u64 as u128, 1),
    }
}

unsafe fn column_text(stmt: *mut Stmt, column: c_int) -> u128 {
    unsafe {
        let db = sqlite3_db_handle(stmt);
        locked(db, || {
            // Text first, then its length, as SQLite requires for a
            // converted value.
            let data = sqlite3_column_text(stmt, column);
            if data.is_null() {
                if sqlite3_column_type(stmt, column) != NULL {
                    return text_result(Err(checked(db, NOMEM)));
                }
                return text_result(Ok(""));
            }
            let length = sqlite3_column_bytes(stmt, column).max(0) as usize;
            let bytes = std::slice::from_raw_parts(data, length);
            text_result(std::str::from_utf8(bytes).map_err(|_| {
                remember(b"stored text is not valid UTF-8");
                INVALID_UTF8
            }))
        })
    }
}

unsafe fn column_name(stmt: *mut Stmt, column: c_int) -> u128 {
    unsafe {
        let name = sqlite3_column_name(stmt, column);
        if name.is_null() {
            return text_result(Err(allocation(AllocError::OutOfMemory)));
        }
        text_result(std::str::from_utf8(CStr::from_ptr(name).to_bytes()).map_err(|_| INVALID_UTF8))
    }
}

/// Column indices were checked against the column count by `std.db.sqlite`.
unsafe fn column(stmt: *mut Stmt, value: u128) -> c_int {
    let column = value as i64;
    if column < 0 || column >= unsafe { sqlite3_column_count(stmt) } as i64 {
        crate::fail("std.db.sqlite column out of range");
    }
    column as c_int
}

/// Operands are live values of the statically checked signature.
pub(super) unsafe fn call(op: i64, operands: [u128; 4]) -> u128 {
    let [a, b, c, _] = operands;
    // SAFETY: see above; each handle is validated before use.
    unsafe {
        match op {
            440 => opened(open(a, b as i64, c as i64)),
            441 => MESSAGE.with(|message| {
                let message = message.borrow();
                let text = std::str::from_utf8_unchecked(&message.0[..message.1]);
                result(strings::try_new(text).map(|text| text as u128))
            }),
            442 => execute(a, b) as u128,
            443 => opened(prepare(a, b)),
            444 => match database(a) {
                Ok(db) => sqlite3_changes64(db) as u128,
                Err(_) => 0,
            },
            445 => match database(a) {
                Ok(db) => sqlite3_last_insert_rowid(db) as u128,
                Err(_) => 0,
            },
            446 => match database(a) {
                Ok(db) => {
                    sqlite3_busy_timeout(db, (b as i64).clamp(0, i32::MAX as i64) as c_int) as u128
                }
                Err(code) => code as u128,
            },
            447 => close(a) as u128,
            448 => opened(begin(a, b as i64)),
            449 | 450 => {
                let transaction = transaction(a);
                let sql = if op == 449 { c"COMMIT" } else { c"ROLLBACK" };
                finish((*transaction).connection, (*transaction).generation, sql) as u128
            }
            451 => step(statement(a)) as u128,
            452 => {
                // Reset repeats the last step's error, which was reported then.
                sqlite3_reset(statement(a));
                OK as u128
            }
            453 => sqlite3_clear_bindings(statement(a)) as u128,
            454 => sqlite3_bind_parameter_count(statement(a)) as u128,
            455 => {
                let stmt = statement(a);
                match c_text(b) {
                    Ok(Some(name)) => {
                        sqlite3_bind_parameter_index(stmt, name.as_ptr().cast()) as u128
                    }
                    Ok(None) => 0,
                    Err(error) => (-allocation(error)) as u128,
                }
            }
            456 => {
                let stmt = statement(a);
                bind(stmt, || sqlite3_bind_null(stmt, index(b))) as u128
            }
            457 => {
                let stmt = statement(a);
                bind(stmt, || sqlite3_bind_int64(stmt, index(b), c as i64)) as u128
            }
            458 => {
                let stmt = statement(a);
                let value = f64::from_bits(c as u64);
                bind(stmt, || sqlite3_bind_double(stmt, index(b), value)) as u128
            }
            459 => {
                let stmt = statement(a);
                let text = c as *const Text;
                let bytes = strings::bytes(&text);
                bind(stmt, || {
                    sqlite3_bind_text64(
                        stmt,
                        index(b),
                        bytes.as_ptr().cast(),
                        bytes.len() as u64,
                        TRANSIENT,
                        UTF8,
                    )
                }) as u128
            }
            460 => {
                let stmt = statement(a);
                let bytes = (*(c as *const Storage)).bytes.as_slice();
                // A zero-length blob still binds a blob, not NULL.
                let data = if bytes.is_empty() {
                    c"".as_ptr().cast()
                } else {
                    bytes.as_ptr().cast()
                };
                bind(stmt, || {
                    sqlite3_bind_blob64(stmt, index(b), data, bytes.len() as u64, TRANSIENT)
                }) as u128
            }
            461 => sqlite3_column_count(statement(a)) as u128,
            462 => {
                let stmt = statement(a);
                sqlite3_column_type(stmt, column(stmt, b)) as u128
            }
            463 => {
                let stmt = statement(a);
                sqlite3_column_int64(stmt, column(stmt, b)) as u128
            }
            464 => {
                let stmt = statement(a);
                sqlite3_column_double(stmt, column(stmt, b)).to_bits() as u128
            }
            466 => {
                let stmt = statement(a);
                column_text(stmt, column(stmt, b))
            }
            467 => {
                let stmt = statement(a);
                let column = column(stmt, b);
                let db = sqlite3_db_handle(stmt);
                result(locked(db, || from_slice(column_bytes(stmt, column))))
            }
            468 => {
                let stmt = statement(a);
                column_name(stmt, column(stmt, b))
            }
            _ => crate::fail("unknown std.db.sqlite operation"),
        }
    }
}
