//! Native back end of `std.tls`: rustls with the ring provider. Nothing here
//! implements a primitive or touches a socket.
//!
//! Trust roots, configurations and sessions are runtime objects whose prefix
//! is empty byte storage, so `std.tls` holds them as `ByteStorage` values with
//! ordinary reference counting. Every operation checks the object's destructor
//! and stops the process on any other storage.
//!
//! A session is a sans-I/O rustls connection behind a mutex: `std.tls` feeds
//! it ciphertext read from a `std.net` stream and sends what it takes out, so
//! the stream's deadlines and cancellation bound every handshake and transfer.
//! Each configuration carries its own provider instead of a process-wide
//! default, so nothing outlives the objects that use it.
//!
//! rustls allocates through Rust's global allocator, which stops the process
//! when memory runs out; only this module's own objects report allocation
//! failure as a value.
use super::byte_storage::{range, Storage};
use super::wrap;
use crate::memory::{self, AllocError, Header};
use crate::strings::{self, Text};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{CertificateError, ClientConfig, Connection, Error, RootCertStore, ServerConfig};
use std::io::{ErrorKind, Read, Write};
use std::ptr;
use std::sync::{Arc, Mutex, MutexGuard};

/// Codes shared with `std.tls`.
const WOULD_BLOCK: i64 = -1;
const UNTRUSTED: i64 = -2;
const EXPIRED: i64 = -3;
const WRONG_HOST: i64 = -4;
const INVALID_CERTIFICATE: i64 = -5;
const REJECTED: i64 = -6;
const PROTOCOL: i64 = -7;
const TRUNCATED: i64 = -8;
const INVALID_NAME: i64 = -9;
const INVALID_CONFIGURATION: i64 = -10;
const ALLOCATION: i64 = -11;
const NO_ROOTS: i64 = -12;

/// `__tls_state` bits.
const HANDSHAKING: i64 = 1;
const WANTS_WRITE: i64 = 2;
const WANTS_READ: i64 = 4;

#[repr(C)]
struct Roots {
    storage: Storage,
    store: Arc<RootCertStore>,
}

enum Settings {
    Client(Arc<ClientConfig>),
    Server(Arc<ServerConfig>),
}

#[repr(C)]
struct Config {
    storage: Storage,
    settings: Settings,
}

#[repr(C)]
struct Session {
    storage: Storage,
    connection: Mutex<Connection>,
}

fn empty_storage(destroy: unsafe extern "C" fn(*mut Header)) -> Storage {
    Storage {
        header: Header::new(destroy),
        bytes: Vec::new(),
    }
}

unsafe extern "C" fn destroy_roots(pointer: *mut Header) {
    // SAFETY: the last owner has gone; `create` allocated `Roots`.
    unsafe {
        ptr::drop_in_place(pointer.cast::<Roots>());
        memory::free::<Roots, u8>(pointer.cast(), 0);
    }
}

unsafe extern "C" fn destroy_config(pointer: *mut Header) {
    // SAFETY: as for `destroy_roots`. Sessions hold their own `Arc`.
    unsafe {
        ptr::drop_in_place(pointer.cast::<Config>());
        memory::free::<Config, u8>(pointer.cast(), 0);
    }
}

unsafe extern "C" fn destroy_session(pointer: *mut Header) {
    // SAFETY: as for `destroy_roots`. rustls zeroes its traffic secrets
    // when the connection drops.
    unsafe {
        ptr::drop_in_place(pointer.cast::<Session>());
        memory::free::<Session, u8>(pointer.cast(), 0);
    }
}

/// Moves `value` into a fresh runtime object.
fn create<T>(value: T) -> Result<u128, AllocError> {
    let owner = memory::try_allocate::<T, u8>(0)?;
    // SAFETY: fresh, exclusively owned memory with `T`'s layout.
    unsafe { owner.write(value) };
    Ok(owner as u128)
}

fn opened(value: Result<u128, i64>) -> u128 {
    match value {
        Ok(value) => wrap(value, 0),
        Err(code) => wrap(code as u64 as u128, 1),
    }
}

fn allocation(_: AllocError) -> i64 {
    ALLOCATION
}

/// The object behind a handle operand, or a stopped process for any other.
unsafe fn object<T>(
    value: u128,
    destroy: unsafe extern "C" fn(*mut Header),
    what: &str,
) -> &'static T {
    // SAFETY: every byte-storage-typed operand starts with a live `Header`;
    // the destructor identifies the layout. The caller's loan outlives the
    // call, and these objects are only read through shared references.
    unsafe {
        let owner = (*(value as *const Header)).destroy;
        if owner.map(|f| f as usize) != Some(destroy as usize) {
            crate::fail(&format!("operand is not a std.tls {what}"));
        }
        &*(value as *const T)
    }
}

unsafe fn connection(value: u128) -> MutexGuard<'static, Connection> {
    // SAFETY: forwarded from the caller.
    let session = unsafe { object::<Session>(value, destroy_session, "session") };
    // A panic aborts the process, so the lock is never poisoned.
    session
        .connection
        .lock()
        .unwrap_or_else(|_| crate::fail("std.tls session lock poisoned"))
}

fn code(error: &Error) -> i64 {
    use CertificateError as C;
    match error {
        Error::InvalidCertificate(certificate) => match certificate {
            C::UnknownIssuer | C::BadSignature => UNTRUSTED,
            C::Expired
            | C::ExpiredContext { .. }
            | C::NotValidYet
            | C::NotValidYetContext { .. } => EXPIRED,
            C::NotValidForName | C::NotValidForNameContext { .. } => WRONG_HOST,
            _ => INVALID_CERTIFICATE,
        },
        Error::AlertReceived(_) => REJECTED,
        _ => PROTOCOL,
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The certificates of every `CERTIFICATE` block; other blocks are skipped.
fn certificates(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, i64> {
    CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| INVALID_CERTIFICATE)
}

/// Certificates webpki cannot use as anchors are skipped, as system bundles
/// may hold some; none usable is `NO_ROOTS`.
fn roots(pem: &[u8]) -> Result<u128, i64> {
    let mut store = RootCertStore::empty();
    let (added, _) = store.add_parsable_certificates(certificates(pem)?);
    if added == 0 {
        return Err(NO_ROOTS);
    }
    create(Roots {
        storage: empty_storage(destroy_roots),
        store: Arc::new(store),
    })
    .map_err(allocation)
}

fn client_config(roots: &Roots) -> Result<u128, i64> {
    let mut config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|_| INVALID_CONFIGURATION)?
        .with_root_certificates(roots.store.clone())
        .with_no_client_auth();
    // No resumption: a session cache is shared mutable memory that outlives
    // connections, and std.tls offers no way to bound or clear it.
    config.resumption = rustls::client::Resumption::disabled();
    create(Config {
        storage: empty_storage(destroy_config),
        settings: Settings::Client(Arc::new(config)),
    })
    .map_err(allocation)
}

fn server_config(chain: &[u8], key: &[u8]) -> Result<u128, i64> {
    let chain = certificates(chain).map_err(|_| INVALID_CONFIGURATION)?;
    if chain.is_empty() {
        return Err(INVALID_CONFIGURATION);
    }
    let key = PrivateKeyDer::from_pem_slice(key).map_err(|_| INVALID_CONFIGURATION)?;
    let mut config = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|_| INVALID_CONFIGURATION)?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|_| INVALID_CONFIGURATION)?;
    // Clients cannot resume, so tickets would only cost memory, and an
    // unread ticket makes a client's close reset the connection.
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    config.send_tls13_tickets = 0;
    create(Config {
        storage: empty_storage(destroy_config),
        settings: Settings::Server(Arc::new(config)),
    })
    .map_err(allocation)
}

fn session(connection: Result<Connection, Error>) -> Result<u128, i64> {
    let connection = connection.map_err(|error| code(&error))?;
    create(Session {
        storage: empty_storage(destroy_session),
        connection: Mutex::new(connection),
    })
    .map_err(allocation)
}

fn client(config: &Config, name: &[u8]) -> Result<u128, i64> {
    let Settings::Client(config) = &config.settings else {
        crate::fail("std.tls client session needs a client configuration");
    };
    let name = std::str::from_utf8(name)
        .ok()
        .and_then(|name| ServerName::try_from(name).ok())
        .ok_or(INVALID_NAME)?
        .to_owned();
    session(rustls::ClientConnection::new(config.clone(), name).map(Connection::from))
}

fn server(config: &Config) -> Result<u128, i64> {
    let Settings::Server(config) = &config.settings else {
        crate::fail("std.tls server session needs a server configuration");
    };
    session(rustls::ServerConnection::new(config.clone()).map(Connection::from))
}

/// Consumes ciphertext and processes complete records; the count consumed,
/// which falls short while decrypted data waits to be read. An empty input
/// marks the end of the peer's stream.
fn feed(connection: &mut Connection, mut data: &[u8]) -> i64 {
    let total = data.len();
    // An error means rustls's plaintext buffer is full until the reader
    // drains it; `std.tls` always reads plaintext before feeding more.
    while connection.read_tls(&mut data).is_ok() {
        if let Err(error) = connection.process_new_packets() {
            return code(&error);
        }
        if data.is_empty() {
            break;
        }
    }
    (total - data.len()) as i64
}

/// Moves pending ciphertext into `target`; the count written.
fn take(connection: &mut Connection, mut target: &mut [u8]) -> i64 {
    let total = target.len();
    while connection.wants_write() && !target.is_empty() {
        match connection.write_tls(&mut target) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
    (total - target.len()) as i64
}

/// Decrypted bytes; 0 once the peer has closed cleanly.
fn read(connection: &mut Connection, target: &mut [u8]) -> i64 {
    match connection.reader().read(target) {
        Ok(count) => count as i64,
        Err(error) if error.kind() == ErrorKind::WouldBlock => WOULD_BLOCK,
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => TRUNCATED,
        Err(_) => PROTOCOL,
    }
}

/// Plaintext accepted for encryption, bounded by rustls's buffer limit.
fn write(connection: &mut Connection, data: &[u8]) -> i64 {
    match connection.writer().write(data) {
        Ok(count) => count as i64,
        Err(_) => PROTOCOL,
    }
}

fn state(connection: &Connection) -> i64 {
    let mut bits = 0;
    if connection.is_handshaking() {
        bits |= HANDSHAKING;
    }
    if connection.wants_write() {
        bits |= WANTS_WRITE;
    }
    if connection.wants_read() {
        bits |= WANTS_READ;
    }
    bits
}

/// Operands are live values of the statically checked signature; `std.tls`
/// has validated each range, and the runtime repeats the check.
pub(super) unsafe fn call(op: i64, operands: [u128; 4]) -> u128 {
    let [a, b, c, d] = operands;
    // SAFETY: see above. Byte views borrow a buffer's heap allocation, never
    // a TLS object.
    unsafe {
        let view = |value: u128, start: u128, stop: u128| {
            let bytes = &(*(value as *const Storage)).bytes;
            &bytes[range(start, stop, bytes.len())]
        };
        let view_mut = |value: u128, start: u128, stop: u128| {
            let bytes = &mut (*(value as *mut Storage)).bytes;
            let range = range(start, stop, bytes.len());
            &mut bytes[range]
        };
        let roots_of = |value| object::<Roots>(value, destroy_roots, "trust root set");
        let config_of = |value| object::<Config>(value, destroy_config, "configuration");
        // Text operands stay in their words, which inline texts' views borrow.
        let (text_a, text_b) = (a as *const Text, b as *const Text);
        match op {
            470 => opened(roots(strings::bytes(&text_a))),
            471 => roots_of(a).store.len() as u128,
            472 => opened(client_config(roots_of(a))),
            473 => opened(server_config(
                strings::bytes(&text_a),
                strings::bytes(&text_b),
            )),
            474 => opened(client(config_of(a), strings::bytes(&text_b))),
            475 => opened(server(config_of(a))),
            476 => feed(&mut connection(a), view(b, c, d)) as u128,
            477 => take(&mut connection(a), view_mut(b, c, d)) as u128,
            478 => read(&mut connection(a), view_mut(b, c, d)) as u128,
            479 => write(&mut connection(a), view(b, c, d)) as u128,
            480 => state(&connection(a)) as u128,
            481 => {
                connection(a).send_close_notify();
                0
            }
            _ => crate::fail("unknown tls operation"),
        }
    }
}
