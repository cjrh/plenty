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
use super::byte_storage::{Storage, range};
use super::wrap;
use crate::memory::{self, AllocError, Header};
use crate::strings::{self, Text};
use rustls::client::{WebPkiServerVerifier, verify_server_name};
use rustls::crypto::CryptoProvider;
use rustls::crypto::ring::ALL_CIPHER_SUITES;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, CertificateRevocationListDer, PrivateKeyDer, ServerName};
use rustls::server::{ClientHello, ParsedCertificate, ResolvesServerCert, WebPkiClientVerifier};
use rustls::sign::{CertifiedKey, SingleCertAndKey};
use rustls::{
    CertificateError, ClientConfig, Connection, Error, ProtocolVersion, RootCertStore,
    ServerConfig, SupportedCipherSuite, SupportedProtocolVersion, version,
};
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
const REVOKED: i64 = -13;
const CERTIFICATE_REQUIRED: i64 = -14;
const INCOMPATIBLE: i64 = -15;
/// A session detail that is not there, such as an unnegotiated protocol.
const ABSENT: i64 = -1;

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
            C::Revoked => REVOKED,
            _ => INVALID_CERTIFICATE,
        },
        Error::NoCertificatesPresented => CERTIFICATE_REQUIRED,
        Error::PeerIncompatible(_) | Error::NoApplicationProtocol => INCOMPATIBLE,
        Error::AlertReceived(_) => REJECTED,
        _ => PROTOCOL,
    }
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

/// What a builder has gathered; `build` turns it into a configuration.
struct Plan {
    server: bool,
    /// A client's anchors for servers; a server's for clients, if it asks.
    roots: Option<Arc<RootCertStore>>,
    clients_required: bool,
    /// A server's chains in selection order; a client's one chain.
    certificates: Vec<Arc<CertifiedKey>>,
    revocations: Vec<CertificateRevocationListDer<'static>>,
    protocols: Vec<Vec<u8>>,
    tls13_only: bool,
    /// Empty means the provider's defaults.
    suites: Vec<SupportedCipherSuite>,
}

#[repr(C)]
struct Builder {
    storage: Storage,
    plan: Mutex<Plan>,
}

unsafe extern "C" fn destroy_builder(pointer: *mut Header) {
    // SAFETY: as for `destroy_roots`.
    unsafe {
        ptr::drop_in_place(pointer.cast::<Builder>());
        memory::free::<Builder, u8>(pointer.cast(), 0);
    }
}

impl Plan {
    fn new(server: bool) -> Self {
        Plan {
            server,
            roots: None,
            clients_required: false,
            certificates: Vec::new(),
            revocations: Vec::new(),
            protocols: Vec::new(),
            tls13_only: false,
            suites: Vec::new(),
        }
    }

    fn certificate(&mut self, chain: &[u8], key: &[u8]) -> i64 {
        if !self.server && !self.certificates.is_empty() {
            return INVALID_CONFIGURATION;
        }
        let chain = match certificates(chain) {
            Ok(chain) if !chain.is_empty() => chain,
            _ => return INVALID_CONFIGURATION,
        };
        let Ok(key) = PrivateKeyDer::from_pem_slice(key) else {
            return INVALID_CONFIGURATION;
        };
        match CertifiedKey::from_der(chain, key, &rustls::crypto::ring::default_provider()) {
            Ok(certified) => {
                self.certificates.push(Arc::new(certified));
                0
            }
            Err(_) => INVALID_CONFIGURATION,
        }
    }

    fn protocol(&mut self, name: &[u8]) -> i64 {
        if name.is_empty() || name.len() > 255 {
            return INVALID_CONFIGURATION;
        }
        self.protocols.push(name.to_vec());
        0
    }

    fn suite(&mut self, index: i64) -> i64 {
        let Some(suite) = usize::try_from(index)
            .ok()
            .and_then(|index| ALL_CIPHER_SUITES.get(index))
        else {
            return INVALID_CONFIGURATION;
        };
        if !self.suites.contains(suite) {
            self.suites.push(*suite);
        }
        0
    }

    fn revocations(&mut self, pem: &[u8]) -> i64 {
        match CertificateRevocationListDer::pem_slice_iter(pem).collect::<Result<Vec<_>, _>>() {
            Ok(lists) if !lists.is_empty() => {
                self.revocations.extend(lists);
                0
            }
            _ => INVALID_CONFIGURATION,
        }
    }

    fn build(&self) -> Result<u128, i64> {
        let mut provider = rustls::crypto::ring::default_provider();
        if !self.suites.is_empty() {
            provider.cipher_suites = self.suites.clone();
        }
        let provider = Arc::new(provider);
        let versions: &[&'static SupportedProtocolVersion] = if self.tls13_only {
            &[&version::TLS13]
        } else {
            &[&version::TLS12, &version::TLS13]
        };
        let settings = if self.server {
            Settings::Server(Arc::new(self.server_config(provider, versions)?))
        } else {
            Settings::Client(Arc::new(self.client_config(provider, versions)?))
        };
        create(Config {
            storage: empty_storage(destroy_config),
            settings,
        })
        .map_err(allocation)
    }

    fn client_config(
        &self,
        provider: Arc<CryptoProvider>,
        versions: &[&'static SupportedProtocolVersion],
    ) -> Result<ClientConfig, i64> {
        let roots = self.roots.clone().ok_or(INVALID_CONFIGURATION)?;
        // Certificates without a list from their issuer are not checked, so
        // a list for one authority leaves servers under others reachable.
        let verifier = WebPkiServerVerifier::builder_with_provider(roots, provider.clone())
            .with_crls(self.revocations.clone())
            .allow_unknown_revocation_status()
            .build()
            .map_err(|_| INVALID_CONFIGURATION)?;
        let builder = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(versions)
            .map_err(|_| INVALID_CONFIGURATION)?
            .with_webpki_verifier(verifier);
        let mut config = match self.certificates.first() {
            Some(certified) => builder
                .with_client_cert_resolver(Arc::new(SingleCertAndKey::from(certified.clone()))),
            None => builder.with_no_client_auth(),
        };
        config.alpn_protocols = self.protocols.clone();
        // No resumption: a session cache is shared mutable memory that outlives
        // connections, and std.tls offers no way to bound or clear it.
        config.resumption = rustls::client::Resumption::disabled();
        Ok(config)
    }

    fn server_config(
        &self,
        provider: Arc<CryptoProvider>,
        versions: &[&'static SupportedProtocolVersion],
    ) -> Result<ServerConfig, i64> {
        if self.certificates.is_empty() {
            return Err(INVALID_CONFIGURATION);
        }
        let builder = ServerConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(versions)
            .map_err(|_| INVALID_CONFIGURATION)?;
        let builder = match &self.roots {
            Some(roots) => {
                let mut verifier =
                    WebPkiClientVerifier::builder_with_provider(roots.clone(), provider)
                        .with_crls(self.revocations.clone())
                        .allow_unknown_revocation_status();
                if !self.clients_required {
                    verifier = verifier.allow_unauthenticated();
                }
                builder
                    .with_client_cert_verifier(verifier.build().map_err(|_| INVALID_CONFIGURATION)?)
            }
            // Revocation lists only apply to client certificates here.
            None if !self.revocations.is_empty() => return Err(INVALID_CONFIGURATION),
            None => builder.with_no_client_auth(),
        };
        let mut config = builder.with_cert_resolver(Arc::new(ByName(self.certificates.clone())));
        config.alpn_protocols = self.protocols.clone();
        // Clients cannot resume, so tickets would only cost memory, and an
        // unread ticket makes a client's close reset the connection.
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        config.send_tls13_tickets = 0;
        Ok(config)
    }
}

/// A server's chains: the first whose leaf names the host the client asked
/// for (SNI), otherwise the first.
#[derive(Debug)]
struct ByName(Vec<Arc<CertifiedKey>>);

impl ResolvesServerCert for ByName {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let wanted = hello
            .server_name()
            .and_then(|name| ServerName::try_from(name).ok());
        if let Some(wanted) = wanted {
            let names = |certified: &&Arc<CertifiedKey>| {
                certified
                    .end_entity_cert()
                    .ok()
                    .and_then(|leaf| ParsedCertificate::try_from(leaf).ok())
                    .is_some_and(|leaf| verify_server_name(&leaf, &wanted).is_ok())
            };
            if let Some(found) = self.0.iter().find(names) {
                return Some(found.clone());
            }
        }
        self.0.first().cloned()
    }
}

fn builder(server: bool) -> Result<u128, i64> {
    create(Builder {
        storage: empty_storage(destroy_builder),
        plan: Mutex::new(Plan::new(server)),
    })
    .map_err(allocation)
}

fn client_config(roots: &Roots) -> Result<u128, i64> {
    let mut plan = Plan::new(false);
    plan.roots = Some(roots.store.clone());
    plan.build()
}

fn server_config(chain: &[u8], key: &[u8]) -> Result<u128, i64> {
    let mut plan = Plan::new(true);
    match plan.certificate(chain, key) {
        0 => plan.build(),
        code => Err(code),
    }
}

/// Copies a session detail into `target`; its full length, or `ABSENT`.
fn detail(value: Option<&[u8]>, target: &mut [u8]) -> i64 {
    let Some(value) = value else {
        return ABSENT;
    };
    let count = value.len().min(target.len());
    target[..count].copy_from_slice(&value[..count]);
    value.len() as i64
}

fn server_name(connection: &Connection) -> Option<&[u8]> {
    match connection {
        Connection::Server(server) => server.server_name().map(str::as_bytes),
        Connection::Client(_) => None,
    }
}

fn version(connection: &Connection) -> i64 {
    match connection.protocol_version() {
        Some(ProtocolVersion::TLSv1_2) => 12,
        Some(ProtocolVersion::TLSv1_3) => 13,
        _ => ABSENT,
    }
}

/// The negotiated suite's index in the ring provider's list, which
/// `std.tls`'s `CipherSuite` follows.
fn suite(connection: &Connection) -> i64 {
    connection
        .negotiated_cipher_suite()
        .and_then(|negotiated| {
            ALL_CIPHER_SUITES
                .iter()
                .position(|suite| suite.suite() == negotiated.suite())
        })
        .map_or(ABSENT, |index| index as i64)
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
// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
pub(super) unsafe fn call(op: i64, operands: [u128; 4]) -> u128 {
    let [a, b, c, d] = operands;
    // SAFETY: see above. Byte views borrow a buffer's heap allocation, never
    // a TLS object.
    unsafe {
        // A read-only view may also be a `bytes` literal being written.
        let view = |value: u128, start: u128, stop: u128| {
            let bytes = super::byte_storage::source(value);
            &bytes[range(start, stop, bytes.len())]
        };
        let view_mut = |value: u128, start: u128, stop: u128| {
            let bytes = &mut (*(value as *mut Storage)).bytes;
            let range = range(start, stop, bytes.len());
            &mut bytes[range]
        };
        let roots_of = |value| object::<Roots>(value, destroy_roots, "trust root set");
        let config_of = |value| object::<Config>(value, destroy_config, "configuration");
        let plan = |value| {
            object::<Builder>(value, destroy_builder, "configuration builder")
                .plan
                .lock()
                .unwrap_or_else(|_| crate::fail("std.tls builder lock poisoned"))
        };
        // Text operands stay in their words, which inline texts' views borrow.
        let (text_a, text_b, text_c) = (a as *const Text, b as *const Text, c as *const Text);
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
            482 => opened(builder(a != 0)),
            483 => {
                let mut plan = plan(a);
                plan.roots = Some(roots_of(b).store.clone());
                plan.clients_required = c != 0;
                0
            }
            484 => plan(a).certificate(strings::bytes(&text_b), strings::bytes(&text_c)) as u128,
            485 => plan(a).protocol(strings::bytes(&text_b)) as u128,
            486 => {
                plan(a).tls13_only = b as i64 == 13;
                0
            }
            487 => plan(a).suite(b as u64 as i64) as u128,
            488 => plan(a).revocations(strings::bytes(&text_b)) as u128,
            489 => opened(plan(a).build()),
            490 => detail(connection(a).alpn_protocol(), view_mut(b, c, d)) as u128,
            491 => detail(server_name(&connection(a)), view_mut(b, c, d)) as u128,
            492 => {
                let connection = connection(a);
                let leaf = connection
                    .peer_certificates()
                    .and_then(|chain| chain.first())
                    .map(|leaf| leaf.as_ref());
                detail(leaf, view_mut(b, c, d)) as u128
            }
            493 => version(&connection(a)) as u128,
            494 => suite(&connection(a)) as u128,
            _ => crate::fail("unknown tls operation"),
        }
    }
}
