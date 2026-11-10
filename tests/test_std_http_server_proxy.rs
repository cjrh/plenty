//! `std.http.server` behind real reverse proxies (issue #103, B66): nginx and
//! Caddy, each in a stock container in front of a server running on the host.
//! Keep-alive, streamed and chunked bodies, `Expect: 100-continue`, forwarded
//! `Host`, `X-Forwarded-For` and `X-Forwarded-Proto`, TLS terminated at the
//! proxy, slow clients and upstreams, refusals with a lingering close, and a
//! drain on `SIGTERM`. See "Reverse proxies" in
//! `book/src/design/stdlib/http-qualification.md`.
//!
//! The tests need Docker and curl and run only with `PLENTY_DOCKER_TESTS=1`;
//! otherwise each prints that it is skipped. With the variable set, a missing
//! Docker or curl fails them. The proxy runs with `--network host` and listens
//! on loopback ports chosen at run time; the server program, linked against
//! the host's C library, runs on the host.

mod support;

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const SERVER: &str = include_str!("fixtures/proxy/server.plenty");
const NGINX_CONF: &str = include_str!("fixtures/proxy/nginx.conf");
const CADDYFILE: &str = include_str!("fixtures/proxy/Caddyfile");

/// Exact images; the qualification page records the same versions.
const NGINX_IMAGE: &str = "nginx:1.30.6-alpine3.24";
const CADDY_IMAGE: &str = "caddy:2.11.7-alpine";

/// Where the configuration and certificates are mounted in the container.
const MOUNT: &str = "/etc/plenty-proxy";

const SIGTERM: i32 = 15;

#[test]
fn proxy_handoff_blocks_upstream_and_port_selection() {
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let handoff = std::thread::spawn(move || {
        with_proxy_bind(|| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let upstream_tx = done_tx.clone();
    let upstream = std::thread::spawn(move || with_proxy_bind(|| upstream_tx.send(()).unwrap()));
    let selection = std::thread::spawn(move || {
        let registry = PortRegistry::default();
        let _ports = Ports::reserve_with(&registry, || {
            (
                TcpListener::bind("127.0.0.1:0").unwrap(),
                TcpListener::bind("127.0.0.1:0").unwrap(),
            )
        });
        done_tx.send(()).unwrap();
    });
    assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
    release_tx.send(()).unwrap();
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    handoff.join().unwrap();
    upstream.join().unwrap();
    selection.join().unwrap();
}

#[test]
fn proxy_fixture_type_checks_without_docker() {
    plenty::check_source(SERVER).unwrap();
}

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

/// Whether to run: `PLENTY_DOCKER_TESTS=1` with Docker and curl usable.
pub(super) fn enabled(test: &str) -> bool {
    if std::env::var("PLENTY_DOCKER_TESTS").as_deref() != Ok("1") {
        // Written past the test harness's capture, so the skip is visible.
        let _ = writeln!(
            std::io::stderr(),
            "{test}: skipped; set PLENTY_DOCKER_TESTS=1 to run the Docker proxy tests"
        );
        return false;
    }
    let docker = Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output();
    match docker {
        Ok(output) if output.status.success() => {}
        Ok(output) => panic!(
            "PLENTY_DOCKER_TESTS=1 but the Docker daemon is unavailable: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(error) => panic!("PLENTY_DOCKER_TESTS=1 but docker cannot run: {error}"),
    }
    if let Err(error) = Command::new("curl").arg("--version").output() {
        panic!("PLENTY_DOCKER_TESTS=1 but curl cannot run: {error}");
    }
    true
}

// Cooperating upstreams bind ephemeral ports. Keep them from binding a proxy
// port after its reservation is dropped and before the container listens.
pub(super) fn with_proxy_bind<T>(action: impl FnOnce() -> T) -> T {
    static BIND: Mutex<()> = Mutex::new(());
    let _guard = BIND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    action()
}

type PortRegistry = Arc<Mutex<HashSet<u16>>>;

/// Logical ownership survives releasing the OS sockets for Docker's host-network
/// bind. Both roles share one namespace; unrelated processes are not protected.
struct PortLease {
    registry: PortRegistry,
    ports: [u16; 2],
}

impl PortLease {
    fn claim(registry: &PortRegistry, ports: [u16; 2]) -> Option<Self> {
        let mut claimed = registry.lock().unwrap();
        if ports[0] == ports[1] || ports.iter().any(|port| claimed.contains(port)) {
            return None;
        }
        claimed.extend(ports);
        Some(Self {
            registry: Arc::clone(registry),
            ports,
        })
    }
}

impl Drop for PortLease {
    fn drop(&mut self) {
        let mut claimed = self.registry.lock().unwrap();
        for port in self.ports {
            claimed.remove(&port);
        }
    }
}

/// Keep both TCP ports and matching TLS UDP socket through configuration setup.
/// Caddy binds UDP for HTTP/3 even though these tests use TCP transports.
struct Ports {
    http: TcpListener,
    tls: TcpListener,
    _quic: UdpSocket,
    lease: PortLease,
}

impl Ports {
    fn reserve() -> Self {
        static REGISTRY: OnceLock<PortRegistry> = OnceLock::new();
        Self::reserve_with(REGISTRY.get_or_init(PortRegistry::default), || {
            (
                TcpListener::bind("127.0.0.1:0").unwrap(),
                TcpListener::bind("127.0.0.1:0").unwrap(),
            )
        })
    }

    fn reserve_with(
        registry: &PortRegistry,
        mut candidate: impl FnMut() -> (TcpListener, TcpListener),
    ) -> Self {
        for _ in 0..100 {
            let ports = with_proxy_bind(|| {
                let (http, tls) = candidate();
                let numbers = [
                    http.local_addr().unwrap().port(),
                    tls.local_addr().unwrap().port(),
                ];
                let lease = PortLease::claim(registry, numbers)?;
                let quic = UdpSocket::bind(tls.local_addr().unwrap()).ok()?;
                Some(Self {
                    http,
                    tls,
                    _quic: quic,
                    lease,
                })
            });
            if let Some(ports) = ports {
                return ports;
            }
        }
        panic!("cannot reserve two TCP ports and a matching UDP port");
    }

    fn release_sockets(self) -> PortLease {
        self.lease
    }
}

/// The server program on the host. It is killed if the test ends early.
struct Server {
    child: Child,
    lines: BufReader<ChildStdout>,
    port: u16,
    _dir: tempfile::TempDir,
}

impl Server {
    fn start() -> Server {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("main.plenty");
        let built = dir.path().join("built");
        let executable = dir.path().join("server");
        std::fs::write(&source, SERVER).unwrap();
        plenty::compile_file_to_executable(&source, &built, None)
            .unwrap_or_else(|error| panic!("{error}"));
        support::install_executable(&built, &executable);
        with_proxy_bind(|| {
            let mut child = Command::new(executable)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(std::fs::File::create(dir.path().join("stderr")).unwrap())
                .spawn()
                .unwrap();
            let mut lines = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            lines.read_line(&mut line).unwrap();
            let port = line
                .trim()
                .strip_prefix("port ")
                .unwrap_or_else(|| panic!("no port line: {line:?}"))
                .parse()
                .unwrap();
            Server {
                child,
                lines,
                port,
                _dir: dir,
            }
        })
    }

    /// Sends `SIGTERM`, then returns the report the server prints as it exits.
    fn terminate(&mut self) -> String {
        // SAFETY: plain integer arguments; the child is unreaped, so its pid is ours.
        assert_eq!(unsafe { kill(self.child.id() as i32, SIGTERM) }, 0);
        let mut rest = String::new();
        self.lines.read_to_string(&mut rest).unwrap();
        let status = self.child.wait().unwrap();
        assert!(status.success(), "{status:?}\n{rest}");
        rest.trim_end().to_owned()
    }

    fn diagnostics(&self) -> String {
        std::fs::read_to_string(self._dir.path().join("stderr")).unwrap_or_default()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("upstream {} stderr:\n{}", self.port, self.diagnostics());
        }
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Kind {
    Nginx,
    Caddy,
}

/// A proxy container in front of a server, removed when dropped (also when
/// the test panics). Only this container is ever stopped or removed.
pub(super) struct Proxy {
    kind: Kind,
    name: String,
    http: u16,
    tls: u16,
    identity: String,
    docker: Child,
    dir: tempfile::TempDir,
    // Fields drop after Proxy::drop completes container cleanup, including unwind.
    _ports: PortLease,
}

impl Proxy {
    pub(super) fn start(kind: Kind, upstream: u16) -> Proxy {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let (image, template, config, command): (_, _, _, &[&str]) = match kind {
            Kind::Nginx => (
                NGINX_IMAGE,
                NGINX_CONF,
                "nginx.conf",
                &[
                    "nginx",
                    "-c",
                    "/etc/plenty-proxy/nginx.conf",
                    "-g",
                    "daemon off;",
                ],
            ),
            Kind::Caddy => (
                CADDY_IMAGE,
                CADDYFILE,
                "Caddyfile",
                &[
                    "caddy",
                    "run",
                    "--config",
                    "/etc/plenty-proxy/Caddyfile",
                    "--adapter",
                    "caddyfile",
                ],
            ),
        };
        let present = Command::new("docker")
            .args(["image", "inspect", image])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        if !present.success() {
            let pulled = Command::new("docker")
                .args(["pull", image])
                .output()
                .unwrap();
            assert!(
                pulled.status.success(),
                "docker pull {image}: {}",
                String::from_utf8_lossy(&pulled.stderr)
            );
        }
        let ports = Ports::reserve();
        let http = ports.http.local_addr().unwrap().port();
        let tls = ports.tls.local_addr().unwrap().port();
        let name = format!(
            "plenty-proxy-test-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = tempfile::tempdir().unwrap();
        let text = template
            .replace("@UPSTREAM_PORT@", &upstream.to_string())
            .replace("@HTTP_PORT@", &http.to_string())
            .replace("@TLS_PORT@", &tls.to_string())
            .replace("@IDENTITY@", &name);
        std::fs::write(dir.path().join(config), text).unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls");
        for file in ["leaf.pem", "leaf.key"] {
            std::fs::copy(fixtures.join(file), dir.path().join(file)).unwrap();
        }
        let log = std::fs::File::create(dir.path().join("log")).unwrap();
        // `z` relabels the mount for SELinux hosts; the files are ours.
        // Host-network containers must bind these sockets themselves. Keep
        // the unavoidable external handoff window short. Retain logical ownership
        // so another fixture in this process cannot select either port meanwhile.
        // Identity probes still reject foreign listeners immediately.
        with_proxy_bind(|| {
            let lease = ports.release_sockets();
            let docker = Command::new("docker")
                .args(["run", "--rm", "--name", &name, "--network", "host"])
                .args(["--label", "plenty-test=proxy", "-v"])
                .arg(format!("{}:{MOUNT}:ro,z", dir.path().display()))
                .arg(image)
                .args(command)
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap();
            let mut proxy = Proxy {
                kind,
                name: name.clone(),
                http,
                tls,
                identity: name,
                docker,
                dir,
                _ports: lease,
            };
            proxy.wait_ready();
            proxy
        })
    }

    fn wait_ready(&mut self) {
        let start = Instant::now();
        for secure in [false, true] {
            let url = if secure {
                self.tls_url("/__plenty_proxy_ready")
            } else {
                self.url("/__plenty_proxy_ready")
            };
            loop {
                if let Ok(Some(status)) = self.docker.try_wait() {
                    panic!("proxy exited with {status}:\n{}", self.log());
                }
                let mut command = Command::new("curl");
                command.args([
                    "--silent",
                    "--show-error",
                    "--max-time",
                    "2",
                    "--noproxy",
                    "*",
                    "--interface",
                    "127.0.0.2",
                    "-w",
                    "\n%{http_code}",
                ]);
                if secure {
                    command.args(self.tls_args());
                }
                command
                    .arg("--dump-header")
                    .arg(self.dir.path().join("readiness.headers"));
                let output = command.arg(&url).output().unwrap();
                let text = String::from_utf8_lossy(&output.stdout);
                match readiness(&text, &self.identity) {
                    Ok(true) => {
                        assert!(
                            output.status.success(),
                            "readiness transport failed: {output:?}"
                        );
                        break;
                    }
                    Ok(false) => {}
                    Err(error) => panic!(
                        "proxy readiness {url}: {error}; response={text:?}; stderr={}\nheaders:\n{}\n{}",
                        String::from_utf8_lossy(&output.stderr),
                        std::fs::read_to_string(self.dir.path().join("readiness.headers"))
                            .unwrap_or_default(),
                        self.log()
                    ),
                }
                assert!(
                    start.elapsed() < Duration::from_secs(60),
                    "proxy not ready at {url}: {text:?}; stderr={}\n{}",
                    String::from_utf8_lossy(&output.stderr),
                    self.log()
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if let Ok(Some(status)) = self.docker.try_wait() {
            panic!(
                "proxy exited after readiness with {status}:\n{}",
                self.log()
            );
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("log")).unwrap_or_default()
    }

    pub(super) fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.http)
    }

    pub(super) fn tls_url(&self, path: &str) -> String {
        format!("https://localhost:{}{path}", self.tls)
    }

    /// curl arguments that reach the TLS port and trust the test CA.
    pub(super) fn tls_args(&self) -> Vec<String> {
        let ca = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls/ca.pem");
        vec![
            "--cacert".into(),
            ca.display().to_string(),
            "--resolve".into(),
            format!("localhost:{}:127.0.0.1", self.tls),
        ]
    }
}

/// Only an absent HTTP response is a startup condition. A foreign endpoint
/// or any HTTP error is evidence, not a condition to silently retry.
fn readiness(response: &str, identity: &str) -> Result<bool, String> {
    match response.rsplit_once('\n') {
        Some(("", "000")) => Ok(false),
        Some((body, "200")) if body == identity => Ok(true),
        _ => Err("unexpected status or proxy identity".into()),
    }
}

#[test]
fn proxy_readiness_rejects_http_errors_and_foreign_listeners() {
    assert_eq!(readiness("\n000", "ours"), Ok(false));
    assert_eq!(readiness("ours\n200", "ours"), Ok(true));
    for response in [
        "Bad Request\n400",
        "foreign\n200",
        "ours\n502",
        "ours\n000",
        "",
    ] {
        assert!(readiness(response, "ours").is_err(), "{response:?}");
    }
}

#[test]
fn proxy_ports_hold_distinct_tcp_and_matching_udp_reservations() {
    let ports = Ports::reserve();
    let http = ports.http.local_addr().unwrap();
    let tls = ports.tls.local_addr().unwrap();
    assert_ne!(http.port(), tls.port());
    assert!(TcpListener::bind(http).is_err());
    assert!(TcpListener::bind(tls).is_err());
    assert!(UdpSocket::bind(tls).is_err());
}

#[test]
fn proxy_ports_reselect_claimed_candidates_after_socket_release() {
    let registry = PortRegistry::default();
    let candidate = || {
        (
            TcpListener::bind("127.0.0.1:0").unwrap(),
            TcpListener::bind("127.0.0.1:0").unwrap(),
        )
    };
    let first = Ports::reserve_with(&registry, candidate);
    let http = first.http.local_addr().unwrap();
    let tls = first.tls.local_addr().unwrap();
    let lease = first.release_sockets();
    // No Docker listener exists: force the exact pair back into selection,
    // including swapped roles, rather than waiting for ephemeral-port chance.
    assert!(UdpSocket::bind(tls).is_ok());
    let mut attempts = 0;
    let second = Ports::reserve_with(&registry, || {
        attempts += 1;
        match attempts {
            1 => (
                TcpListener::bind(http).unwrap(),
                TcpListener::bind(tls).unwrap(),
            ),
            2 => (
                TcpListener::bind(tls).unwrap(),
                TcpListener::bind(http).unwrap(),
            ),
            _ => candidate(),
        }
    });
    assert!(attempts >= 3);
    assert!(
        !lease
            .ports
            .contains(&second.http.local_addr().unwrap().port())
    );
    assert!(
        !lease
            .ports
            .contains(&second.tls.local_addr().unwrap().port())
    );
    drop(second);
    drop(lease);
    let reused = Ports::reserve_with(&registry, || {
        (
            TcpListener::bind(http).unwrap(),
            TcpListener::bind(tls).unwrap(),
        )
    });
    assert_eq!(reused.lease.ports, [http.port(), tls.port()]);
}

#[test]
fn proxy_port_leases_reject_each_cross_role_without_blocking_independent_pairs() {
    let registry = PortRegistry::default();
    let first = PortLease::claim(&registry, [1, 2]).unwrap();
    for pair in [[1, 3], [3, 1], [2, 3], [3, 2], [2, 1], [3, 3]] {
        assert!(PortLease::claim(&registry, pair).is_none(), "{pair:?}");
    }
    let independent = PortLease::claim(&registry, [3, 4]).unwrap();
    drop(first);
    let reused = PortLease::claim(&registry, [2, 1]).unwrap();
    assert!(PortLease::claim(&registry, [3, 4]).is_none());
    drop(reused);
    drop(independent);
    assert!(registry.lock().unwrap().is_empty());
}

#[test]
fn proxy_port_leases_release_on_startup_error_and_unwind() {
    let registry = PortRegistry::default();
    let candidate = || {
        (
            TcpListener::bind("127.0.0.1:0").unwrap(),
            TcpListener::bind("127.0.0.1:0").unwrap(),
        )
    };
    let fail_startup = || -> io::Result<()> {
        let ports = Ports::reserve_with(&registry, candidate);
        let _lease = ports.release_sockets();
        Err(io::Error::other("simulated spawn failure"))
    };
    assert!(fail_startup().is_err());
    assert!(registry.lock().unwrap().is_empty());
    for after_handoff in [false, true] {
        let result = std::panic::catch_unwind(|| {
            let ports = Ports::reserve_with(&registry, candidate);
            if after_handoff {
                let _lease = ports.release_sockets();
                panic!("simulated readiness failure");
            } else {
                let _ports = ports;
                panic!("simulated configuration failure");
            }
        });
        assert!(result.is_err());
        assert!(registry.lock().unwrap().is_empty());
    }
    let reused = PortLease::claim(&registry, [1, 2]).unwrap();
    drop(reused);
    assert!(registry.lock().unwrap().is_empty());
}

#[test]
fn proxy_ports_release_claim_when_tls_udp_is_unavailable() {
    let registry = PortRegistry::default();
    let http = TcpListener::bind("127.0.0.1:0").unwrap();
    let tls = TcpListener::bind("127.0.0.1:0").unwrap();
    let numbers = [
        http.local_addr().unwrap().port(),
        tls.local_addr().unwrap().port(),
    ];
    let _udp = UdpSocket::bind(tls.local_addr().unwrap()).unwrap();
    let mut blocked = Some((http, tls));
    let mut spare = Some((
        TcpListener::bind("127.0.0.1:0").unwrap(),
        TcpListener::bind("127.0.0.1:0").unwrap(),
    ));
    let ports = Ports::reserve_with(&registry, || {
        blocked.take().or_else(|| spare.take()).unwrap_or_else(|| {
            (
                TcpListener::bind("127.0.0.1:0").unwrap(),
                TcpListener::bind("127.0.0.1:0").unwrap(),
            )
        })
    });
    assert!(PortLease::claim(&registry, numbers).is_some());
    drop(ports);
    assert!(registry.lock().unwrap().is_empty());
}

#[test]
fn proxy_port_leases_outlive_owner_cleanup() {
    // Proxy uses this same field-drop ordering: release only after its Drop
    // body has removed/waited for the container, not at the start of cleanup.
    struct Owner(PortLease);
    impl Drop for Owner {
        fn drop(&mut self) {
            assert!(PortLease::claim(&self.0.registry, self.0.ports).is_none());
            assert!(PortLease::claim(&self.0.registry, [3, 4]).is_some());
        }
    }
    let registry = PortRegistry::default();
    drop(Owner(PortLease::claim(&registry, [1, 2]).unwrap()));
    assert!(PortLease::claim(&registry, [1, 2]).is_some());
}

// Test-only coordination: a failed worker cancels every phase, and each wait
// has a deadline. Never hold this lock during port selection or assertions.
#[derive(Default)]
struct ProxyPortPhases {
    state: Mutex<ProxyPortPhaseState>,
    changed: std::sync::Condvar,
}

#[derive(Default)]
struct ProxyPortPhaseState {
    generation: usize,
    arrived: usize,
    aborted: bool,
}

impl ProxyPortPhases {
    const WORKERS: usize = 8;
    const WAIT_BOUND: Duration = Duration::from_secs(120);

    fn abort(&self) {
        self.state.lock().unwrap().aborted = true;
        self.changed.notify_all();
    }

    fn wait(&self, phase: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.aborted {
            return false;
        }
        let generation = state.generation;
        state.arrived += 1;
        if state.arrived == Self::WORKERS {
            state.arrived = 0;
            state.generation += 1;
        }
        self.changed.notify_all();
        let (mut state, timed) = self
            .changed
            .wait_timeout_while(state, Self::WAIT_BOUND, |state| {
                state.generation == generation && !state.aborted
            })
            .unwrap();
        if timed.timed_out() && state.generation == generation && !state.aborted {
            state.aborted = true;
            self.changed.notify_all();
            drop(state);
            panic!("proxy port test timed out in {phase}, generation {generation}");
        }
        !state.aborted
    }

    // Used only to inject failure after all seven peers are waiting in phase2.
    fn wait_for_independent_peers(&self) -> bool {
        let state = self.state.lock().unwrap();
        let (mut state, timed) = self
            .changed
            .wait_timeout_while(state, Self::WAIT_BOUND, |state| {
                state.arrived != Self::WORKERS - 1 && !state.aborted
            })
            .unwrap();
        if timed.timed_out() && !state.aborted {
            state.aborted = true;
            self.changed.notify_all();
            drop(state);
            panic!("proxy port test timed out awaiting independent-phase peers");
        }
        !state.aborted
    }
}

struct ProxyPortAbortOnDrop<'a> {
    phases: &'a ProxyPortPhases,
    completed: bool,
}

impl Drop for ProxyPortAbortOnDrop<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.phases.abort();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ProxyPortFailure {
    BeforeFirstPhase,
    IndependentPhase,
    PartialSpawn,
    Parent,
}

fn run_proxy_port_handoffs(registry: &PortRegistry, failure: Option<ProxyPortFailure>) {
    let phases = ProxyPortPhases::default();
    let pairs = Mutex::new([[0; 2]; ProxyPortPhases::WORKERS]);
    std::thread::scope(|scope| {
        // Must unwind before Scope joins: covers parent panic and partial spawn.
        let _parent_abort = ProxyPortAbortOnDrop {
            phases: &phases,
            completed: false,
        };
        let mut workers = Vec::new();
        for worker in 0..ProxyPortPhases::WORKERS {
            let (phases, pairs) = (&phases, &pairs);
            let spawned = if worker == 4 && failure == Some(ProxyPortFailure::PartialSpawn) {
                Err(io::Error::other(
                    "injected proxy port failure: PartialSpawn",
                ))
            } else {
                std::thread::Builder::new().spawn_scoped(scope, move || {
                    let mut abort = ProxyPortAbortOnDrop {
                        phases,
                        completed: false,
                    };
                    for round in 0..32 {
                        if worker == 0
                            && round == 0
                            && failure == Some(ProxyPortFailure::BeforeFirstPhase)
                        {
                            panic!("injected proxy port failure: BeforeFirstPhase");
                        }
                        let candidate = || {
                            (
                                TcpListener::bind("127.0.0.1:0").unwrap(),
                                TcpListener::bind("127.0.0.1:0").unwrap(),
                            )
                        };
                        let ports = Ports::reserve_with(registry, candidate);
                        let lease = ports.release_sockets();
                        pairs.lock().unwrap()[worker] = lease.ports;
                        if !phases.wait("original leases") {
                            return;
                        }
                        let outstanding = *pairs.lock().unwrap();
                        assert_eq!(
                            outstanding
                                .into_iter()
                                .flatten()
                                .collect::<HashSet<_>>()
                                .len(),
                            ProxyPortPhases::WORKERS * 2
                        );
                        for pair in outstanding {
                            // Every candidate is deliberately reused during the gap.
                            assert!(PortLease::claim(registry, pair).is_none());
                            assert!(PortLease::claim(registry, [pair[1], pair[0]]).is_none());
                        }
                        // A held lease must not serialize independent selection.
                        let independent = Ports::reserve_with(registry, candidate);
                        if worker == 0
                            && round == 0
                            && failure == Some(ProxyPortFailure::IndependentPhase)
                        {
                            if !phases.wait_for_independent_peers() {
                                return;
                            }
                            panic!("injected proxy port failure: IndependentPhase");
                        }
                        if !phases.wait("independent reservations") {
                            return;
                        }
                        drop(independent);
                        drop(lease);
                        if !phases.wait("released leases") {
                            return;
                        }
                    }
                    abort.completed = true;
                })
            };
            workers.push(spawned.unwrap_or_else(|error| {
                panic!("cannot spawn proxy port worker {worker}: {error}")
            }));
        }
        if failure == Some(ProxyPortFailure::Parent) {
            panic!("injected proxy port failure: Parent");
        }
        let mut panic = None;
        for worker in workers {
            if let Err(payload) = worker.join() {
                panic.get_or_insert(payload);
            }
        }
        // Peers return on cancellation, leaving the original panic as the error.
        if let Some(payload) = panic {
            std::panic::resume_unwind(payload);
        }
    });
}

#[test]
fn proxy_port_leases_concurrent_handoffs_keep_outstanding_ports_unique() {
    let registry = PortRegistry::default();
    run_proxy_port_handoffs(&registry, None);
    assert!(registry.lock().unwrap().is_empty());
}

fn assert_proxy_port_handoff_failure(failure: ProxyPortFailure) {
    let registry = PortRegistry::default();
    let panic = std::panic::catch_unwind(|| run_proxy_port_handoffs(&registry, Some(failure)))
        .expect_err("injected worker/parent failure must propagate");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap();
    let expected = if failure == ProxyPortFailure::PartialSpawn {
        "cannot spawn proxy port worker 4: injected proxy port failure: PartialSpawn".to_owned()
    } else {
        format!("injected proxy port failure: {failure:?}")
    };
    assert_eq!(message, expected);
    // Scope has joined every started worker before returning or propagating.
    assert!(registry.lock().unwrap().is_empty());
}

#[test]
fn proxy_port_handoffs_abort_on_failure_before_first_phase() {
    assert_proxy_port_handoff_failure(ProxyPortFailure::BeforeFirstPhase);
}

#[test]
fn proxy_port_handoffs_abort_while_peers_wait_with_independent_reservations() {
    assert_proxy_port_handoff_failure(ProxyPortFailure::IndependentPhase);
}

#[test]
fn proxy_port_handoffs_abort_on_partial_spawn_failure() {
    assert_proxy_port_handoff_failure(ProxyPortFailure::PartialSpawn);
}

#[test]
fn proxy_port_handoffs_abort_on_parent_failure() {
    assert_proxy_port_handoff_failure(ProxyPortFailure::Parent);
}

impl Drop for Proxy {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("proxy {} log:\n{}", self.name, self.log());
        }
        let _ = Command::new("docker")
            .args(["rm", "--force", &self.name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.docker.kill();
        let _ = self.docker.wait();
    }
}

/// curl, which must succeed. Requests come from 127.0.0.2, so the server can
/// tell the client from the proxy, which connects from 127.0.0.1.
fn curl(args: &[&str]) -> Output {
    let headers = tempfile::NamedTempFile::new().unwrap();
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "30",
            "--noproxy",
            "*",
        ])
        .args(["--interface", "127.0.0.2"])
        .arg("--dump-header")
        .arg(headers.path())
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "curl {args:?}: {}; response: {:?}; headers: {:?}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
        std::fs::read_to_string(headers.path()).unwrap()
    );
    output
}

/// The status code and the body as text.
fn fetch(args: &[&str]) -> (String, String) {
    let mut all = vec!["-w", "\n%{http_code}"];
    all.extend(args);
    let text = String::from_utf8(curl(&all).stdout).unwrap();
    let (body, code) = text.rsplit_once('\n').unwrap();
    (code.to_owned(), body.to_owned())
}

/// Receives while uploading: curl can abandon a final response when a send
/// fails after Caddy closes an unread request body. Only a complete, correctly
/// framed response permits an upload-side reset; no request is retried.
/// The body starts without waiting for `100 Continue`, as HTTP permits;
/// `bodies_and_continue` separately qualifies clients that wait for it.
fn relayed_upload(port: u16, path: &str, body: Vec<u8>, chunked: bool, expected: (&str, &str)) {
    use std::io::ErrorKind;
    use std::net::Shutdown;

    let mut stream = TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(5),
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    let mut sender = stream.try_clone().unwrap();
    sender
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let upload_path = path.to_owned();
    let upload = std::thread::spawn(move || -> std::io::Result<()> {
        let framing = if chunked {
            "Transfer-Encoding: chunked".to_owned()
        } else {
            format!("Content-Length: {}", body.len())
        };
        let expect = if body.len() > 1 << 20 {
            "Expect: 100-continue\r\n"
        } else {
            ""
        };
        write!(
            sender,
            "POST {upload_path} HTTP/1.1\r\nHost: localhost\r\n{framing}\r\n{expect}Connection: close\r\n\r\n"
        )?;
        for piece in body.chunks(16 * 1024) {
            if Instant::now() >= until {
                return Err(ErrorKind::TimedOut.into());
            }
            if chunked {
                write!(sender, "{:x}\r\n", piece.len())?;
            }
            sender.write_all(piece)?;
            if chunked {
                sender.write_all(b"\r\n")?;
            }
        }
        if chunked {
            sender.write_all(b"0\r\n\r\n")?;
        }
        Ok(())
    });
    let mut received = Vec::new();
    let mut buffer = [0; 4096];
    let reading = loop {
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break Err(std::io::Error::from(ErrorKind::TimedOut));
        }
        if let Err(error) = stream.set_read_timeout(Some(remaining)) {
            break Err(error);
        }
        match stream.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(count) => {
                received.extend_from_slice(&buffer[..count]);
                if received.len() > 64 * 1024 {
                    break Err(std::io::Error::other("oversized qualification response"));
                }
                if relay_response_complete(&received) {
                    break Ok(());
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => break Err(error),
        }
    };
    // Always stop and join the sender before inspecting the response, even on
    // a read failure. shutdown affects both cloned socket handles.
    let _ = stream.shutdown(Shutdown::Both);
    let uploaded = upload.join().unwrap();
    let text = String::from_utf8(received).unwrap_or_else(|error| {
        panic!(
            "relay {path} chunked={chunked}: response is not UTF-8: {}; bytes={:?}",
            error.utf8_error(),
            error.as_bytes()
        )
    });
    if uploaded.is_err() || reading.is_err() {
        eprintln!("relay {path} chunked={chunked}: upload={uploaded:?} read={reading:?}");
    }
    assert_complete_relay(&text, expected);
    for result in [reading, uploaded] {
        if let Err(error) = result {
            assert!(
                matches!(
                    error.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::BrokenPipe | ErrorKind::NotConnected
                ),
                "{path}: {error}; response={text:?}"
            );
        }
    }
}

/// Message framing completes an HTTP response even while the proxy drains an
/// unread upload before closing TCP. Final status/body validation stays separate.
fn relay_response_complete(mut response: &[u8]) -> bool {
    loop {
        let Some(end) = response.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
            return false;
        };
        let Ok(head) = std::str::from_utf8(&response[..end]) else {
            return false;
        };
        let body = &response[end + 4..];
        if head.starts_with("HTTP/1.1 100 ") {
            response = body;
            continue;
        }
        let mut length = None;
        for line in head.split("\r\n").skip(1) {
            let Some((name, value)) = line.split_once(':') else {
                return false;
            };
            if name.eq_ignore_ascii_case("Transfer-Encoding") {
                return false;
            }
            if name.eq_ignore_ascii_case("Content-Length") {
                if length.is_some() {
                    return false;
                }
                let Ok(value) = value.trim().parse::<usize>() else {
                    return false;
                };
                length = Some(value);
            }
        }
        // Include excess bytes in final validation, which rejects them.
        return length.is_some_and(|length| body.len() >= length);
    }
}

#[test]
fn relay_completes_before_the_peer_closes_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nearly").unwrap();
        // Wait for client shutdown rather than closing to delimit the answer.
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
    });
    relayed_upload(port, "/early", Vec::new(), false, ("200", "early"));
    peer.join().unwrap();
}

fn assert_complete_relay(mut response: &str, expected: (&str, &str)) {
    let original = response;
    while response.starts_with("HTTP/1.1 100 ") {
        response = response
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("incomplete interim head: {original:?}"))
            .1;
    }
    let (head, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("incomplete final head: {original:?}"));
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap();
    assert_eq!(
        status.split_whitespace().take(2).collect::<Vec<_>>(),
        ["HTTP/1.1", expected.0],
        "{response:?}"
    );
    let mut lengths = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').expect("response header field");
        assert!(
            !name.eq_ignore_ascii_case("Transfer-Encoding"),
            "unexpected framing: {response:?}"
        );
        if name.eq_ignore_ascii_case("Content-Length") {
            lengths.push(value.trim().parse::<usize>().unwrap());
        }
    }
    assert_eq!(lengths, [expected.1.len()], "{response:?}");
    assert_eq!(body, expected.1, "{response:?}");
}

#[test]
fn relayed_response_requires_a_complete_final_answer() {
    let complete = "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 413 Content Too Large\r\nContent-Length: 18\r\n\r\nContent Too Large\n";
    assert_complete_relay(complete, ("413", "Content Too Large\n"));
    assert!(relay_response_complete(complete.as_bytes()));
    for end in 0..complete.len() {
        assert!(!relay_response_complete(&complete.as_bytes()[..end]));
    }
    for response in [
        "HTTP/1.1 100 Continue\r\n\r\n",
        "HTTP/1.1 413 Content Too Large\r\nContent-Length: 18\r\n\r\nContent Too",
        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 18\r\n\r\nContent Too Large\n",
        "HTTP/1.1 413 Content Too Large\r\nContent-Length: 19\r\n\r\nContent Too Large\n",
        "HTTP/1.1 413 Content Too Large\r\nContent-Length: 18\r\nContent-Length: 18\r\n\r\nContent Too Large\n",
        "HTTP/1.1 413 Content Too Large\r\nContent-Length: 18\r\n\r\nContent Too Large\nextra",
        "HTTP/1.1 413 Content Too Large\r\nTransfer-Encoding: chunked\r\nContent-Length: 18\r\n\r\nContent Too Large\n",
    ] {
        assert!(
            std::panic::catch_unwind(|| assert_complete_relay(
                response,
                ("413", "Content Too Large\n")
            ))
            .is_err(),
            "accepted {response:?}"
        );
    }
}

fn connect(port: u16) -> TcpStream {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    stream
}

/// Reads until `received` contains `marker`.
fn read_until(stream: &mut impl Read, received: &mut Vec<u8>, marker: &str) -> io::Result<()> {
    let mut buffer = [0u8; 4096];
    while !String::from_utf8_lossy(received).contains(marker) {
        let count = match stream.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "ended before {marker:?}: {}",
                    String::from_utf8_lossy(received)
                ),
            ));
        }
        received.extend_from_slice(&buffer[..count]);
    }
    Ok(())
}

fn payload(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

/// The `name=value` lines `/info` returns, as a map.
fn info(body: &str) -> std::collections::BTreeMap<String, String> {
    body.lines()
        .map(|line| {
            let (name, value) = line.split_once('=').unwrap();
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

/// Parses `connections=N requests=N ...`.
fn report(text: &str) -> std::collections::BTreeMap<String, u64> {
    text.split(' ')
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap();
            (name.to_owned(), value.parse().unwrap())
        })
        .collect()
}

/// Host and the forwarding fields reach the handler; a spoofed
/// `X-Forwarded-For` is not believed; client connections and upstream
/// connections are both reused; TLS is terminated at the proxy.
fn forwarding_keep_alive_and_tls(kind: Kind) {
    let mut server = Server::start();
    let proxy = Proxy::start(kind, server.port);

    let (code, body) = fetch(&[
        "-H",
        "Host: app.example:8080",
        "-H",
        "X-Forwarded-For: 203.0.113.9",
        &proxy.url("/info"),
    ]);
    assert_eq!(code, "200", "{body}");
    let seen = info(&body);
    assert_eq!(seen["authority"], "app.example:8080", "{body}");
    assert_eq!(seen["Host"], "app.example:8080", "{body}");
    assert_eq!(seen["X-Forwarded-Proto"], "http", "{body}");
    assert_eq!(seen["peer"], "127.0.0.1", "{body}");
    // nginx appends the address it saw to the client's header; Caddy, which
    // trusts no proxy by default, replaces it.
    let forwarded = match proxy.kind {
        Kind::Nginx => "203.0.113.9, 127.0.0.2",
        Kind::Caddy => "127.0.0.2",
    };
    assert_eq!(seen["X-Forwarded-For"], forwarded, "{body}");
    assert_eq!(seen["client"], "127.0.0.2 http", "{body}");

    // Two URLs on one client connection: the second reuses it.
    let reuse = curl(&[
        "-w",
        "|%{http_code} %{num_connects}\n",
        "-o",
        "/dev/null",
        &proxy.url("/info"),
        "-o",
        "/dev/null",
        &proxy.url("/info"),
    ]);
    assert_eq!(String::from_utf8_lossy(&reuse.stdout), "|200 1\n|200 0\n");

    let tls = proxy.tls_args();
    let tls: Vec<&str> = tls.iter().map(String::as_str).collect();
    let mut args = tls.clone();
    let url = proxy.tls_url("/info");
    args.push(&url);
    let (code, body) = fetch(&args);
    assert_eq!(code, "200", "{body}");
    let seen = info(&body);
    assert_eq!(seen["X-Forwarded-Proto"], "https", "{body}");
    assert_eq!(seen["client"], "127.0.0.2 https", "{body}");
    assert_eq!(
        seen["authority"],
        format!("localhost:{}", proxy.tls),
        "{body}"
    );

    // Caddy offers HTTP/2 to clients and still speaks HTTP/1.1 to the server.
    let version = match proxy.kind {
        Kind::Nginx => "1.1",
        Kind::Caddy => "2",
    };
    let mut args = tls.clone();
    args.extend([
        "-w",
        "|%{http_version} %{num_connects}\n",
        "-o",
        "/dev/null",
    ]);
    args.extend([url.as_str(), "-o", "/dev/null", url.as_str()]);
    assert_eq!(
        String::from_utf8_lossy(&curl(&args).stdout),
        format!("|{version} 1\n|{version} 0\n")
    );

    let dir = tempfile::tempdir().unwrap();
    let upload = dir.path().join("upload");
    let sent = payload(700_000);
    std::fs::write(&upload, &sent).unwrap();
    let upload = format!("@{}", upload.display());
    let echo = proxy.tls_url("/echo");
    let mut args = tls.clone();
    args.extend(["--data-binary", &upload, &echo]);
    assert!(curl(&args).stdout == sent, "TLS upload");

    // Twelve requests came through the proxy over far fewer upstream
    // connections.
    for _ in 0..5 {
        assert_eq!(fetch(&[&proxy.url("/info")]).0, "200");
    }
    let counts = report(&server.terminate());
    assert_eq!(counts["requests"], 12, "{counts:?}");
    assert!(counts["connections"] <= 3, "{counts:?}");
    for name in ["handler_errors", "protocol_errors", "timeouts", "rejected"] {
        assert_eq!(counts[name], 0, "{name}: {counts:?}");
    }
}

/// Sized, chunked and `100-continue` uploads and large downloads, through the
/// buffering location and the streaming one (nginx `/stream/`; Caddy streams
/// both); a streamed response reaches the client before it ends.
fn bodies_and_continue(kind: Kind) {
    let mut server = Server::start();
    let proxy = Proxy::start(kind, server.port);
    let dir = tempfile::tempdir().unwrap();
    let upload = dir.path().join("upload");
    let sent = payload(700_000);
    std::fs::write(&upload, &sent).unwrap();
    let upload = format!("@{}", upload.display());

    for prefix in ["", "/stream"] {
        let echo = proxy.url(&format!("{prefix}/echo"));
        for (extra, label) in [
            (vec![], "sized"),
            (vec!["-H", "Transfer-Encoding: chunked"], "chunked"),
            (
                vec!["-H", "Expect: 100-continue", "--expect100-timeout", "30"],
                "continue",
            ),
        ] {
            let mut args = vec!["--data-binary", upload.as_str()];
            args.extend(extra);
            args.push(&echo);
            assert!(curl(&args).stdout == sent, "{prefix} {label} upload");
        }
        let big = curl(&[&proxy.url(&format!("{prefix}/big"))]).stdout;
        assert!(big.len() == 1 << 20 && big.iter().all(|&byte| byte == b'x'));

        // The client waits for `100 Continue` before sending the body.
        let mut stream = connect(proxy.http);
        stream
            .write_all(
                format!(
                    "POST {prefix}/echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 11\r\n\
                     Expect: 100-continue\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let mut received = Vec::new();
        read_until(&mut stream, &mut received, "\r\n\r\n").unwrap();
        let interim = String::from_utf8_lossy(&received).into_owned();
        assert!(
            interim.starts_with("HTTP/1.1 100 Continue\r\n"),
            "{interim}"
        );
        stream.write_all(b"hello world").unwrap();
        stream.read_to_end(&mut received).unwrap();
        let text = String::from_utf8_lossy(&received);
        // Caddy also relays the server's own `100 Continue`, a second interim
        // response, which clients must accept (RFC 9110, section 15.2).
        let mut last = text.as_ref();
        while last.starts_with("HTTP/1.1 100 ") {
            last = last.split_once("\r\n\r\n").unwrap().1;
        }
        assert!(
            last.starts_with("HTTP/1.1 200 OK\r\n") && last.ends_with("\r\n\r\nhello world"),
            "{prefix}: {text}"
        );
    }

    // The first part of a held response arrives before the handler lets it
    // end, which happens only on the request to /release.
    let mut stream = connect(proxy.http);
    stream
        .write_all(b"GET /stream/hold HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut received = Vec::new();
    read_until(&mut stream, &mut received, "first\n").unwrap();
    assert!(!String::from_utf8_lossy(&received).contains("second"));
    assert_eq!(
        fetch(&[&proxy.url("/release")]),
        ("200".into(), "released".into())
    );
    stream.read_to_end(&mut received).unwrap();
    let text = String::from_utf8_lossy(&received);
    assert!(
        text.contains("second\n") && text.ends_with("0\r\n\r\n"),
        "{text}"
    );

    let counts = report(&server.terminate());
    for name in ["handler_errors", "protocol_errors", "timeouts", "rejected"] {
        assert_eq!(counts[name], 0, "{name}: {counts:?}");
    }
}

/// The proxy, not the server, waits out a slow request head; a slow upstream
/// gets the proxy's timeout answer; refusals and early answers to requests
/// whose bodies the server does not read reach the client whole, because
/// the server closes with a lingering close (issue #138), not a reset.
fn slow_clients_timeouts_and_refusals(kind: Kind) {
    let mut server = Server::start();
    let proxy = Proxy::start(kind, server.port);

    let mut slow = connect(proxy.http);
    slow.write_all(b"GET /info HTTP/1.1\r\nHost: x").unwrap();
    let started = Instant::now();
    assert_eq!(fetch(&[&proxy.url("/info")]).0, "200");
    let mut received = Vec::new();
    let _ = slow.read_to_end(&mut received);
    let waited = started.elapsed();
    let text = String::from_utf8_lossy(&received);
    assert!(
        waited >= Duration::from_millis(1500) && waited < Duration::from_secs(15),
        "{waited:?}"
    );
    // Both proxies close without an answer.
    assert!(text.is_empty(), "{text}");

    let start = Instant::now();
    let (code, _) = fetch(&[&proxy.url("/slow")]);
    assert_eq!(code, "504");
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(fetch(&[&proxy.url("/info")]).0, "200");

    let dir = tempfile::tempdir().unwrap();
    // Over `max_body` (1 MiB) but under nginx's `client_max_body_size`.
    let large = dir.path().join("large");
    std::fs::write(&large, payload(2 << 20)).unwrap();
    let large = format!("@{}", large.display());
    // Under `max_body`; /early answers without reading it. Before the
    // lingering budget grew from 64 KiB to 16 MiB, these and the refusals
    // above often became `502`: the proxy was still sending when the server
    // closed.
    let unread = dir.path().join("unread");
    std::fs::write(&unread, payload(1000 << 10)).unwrap();
    let unread = format!("@{}", unread.display());
    for round in 0..5 {
        for prefix in ["", "/stream"] {
            for extra in [vec![], vec!["-H", "Transfer-Encoding: chunked"]] {
                for (path, upload, expected) in [
                    ("echo", &large, ("413", "Content Too Large\n")),
                    ("early", &unread, ("200", "early")),
                ] {
                    let url = proxy.url(&format!("{prefix}/{path}"));
                    let mut args = vec!["--data-binary", upload.as_str()];
                    args.extend(&extra);
                    args.push(&url);
                    if proxy.kind == Kind::Caddy {
                        relayed_upload(
                            proxy.http,
                            &format!("{prefix}/{path}"),
                            std::fs::read(&upload[1..]).unwrap(),
                            !extra.is_empty(),
                            expected,
                        );
                    } else {
                        let (code, body) = fetch(&args);
                        assert_eq!(
                            (code.as_str(), body.as_str()),
                            expected,
                            "round {round}: {prefix}/{path} {extra:?}"
                        );
                    }
                }
            }
        }
    }
    assert_eq!(fetch(&[&proxy.url("/info")]).0, "200");

    let counts = report(&server.terminate());
    // The partial head never reached the server.
    assert_eq!(counts["timeouts"], 0, "{counts:?}");
}

/// `SIGTERM` while a response streams through the proxy: the response ends
/// whole, the server exits with its report, and the proxy then answers
/// `502` itself.
fn shutdown_drains_through_the_proxy(kind: Kind) {
    let mut server = Server::start();
    let proxy = Proxy::start(kind, server.port);
    let (code, response) = fetch(&["--include", &proxy.url("/info")]);
    let failure_report = if code != "200" {
        server.terminate()
    } else {
        String::new()
    };
    assert_eq!(
        code,
        "200",
        "initial proxy response: {response:?}; http={} tls={} upstream={}\nproxy log:\n{}\nupstream stderr:\n{}\nupstream report:\n{failure_report}",
        proxy.http,
        proxy.tls,
        server.port,
        proxy.log(),
        server.diagnostics()
    );
    let mut stream = connect(proxy.http);
    stream
        .write_all(b"GET /stream/tick HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut received = Vec::new();
    read_until(&mut stream, &mut received, "tick\n").unwrap();
    let counts = report(&server.terminate());
    stream.read_to_end(&mut received).unwrap();
    let text = String::from_utf8_lossy(&received);
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
    assert!(
        text.contains("bye\n") && text.ends_with("0\r\n\r\n"),
        "{text}"
    );
    assert_eq!(counts["requests"], 2, "{counts:?}");
    assert_eq!(counts["cancelled"], 0, "{counts:?}");
    assert_eq!(fetch(&[&proxy.url("/info")]).0, "502");
}

macro_rules! behind {
    ($kind:expr_2021, $($test:ident => $scenario:ident),* $(,)?) => {
        $(
            #[test]
            fn $test() {
                if enabled(stringify!($test)) {
                    $scenario($kind);
                }
            }
        )*
    };
}

behind!(
    Kind::Nginx,
    nginx_forwarding_keep_alive_and_tls => forwarding_keep_alive_and_tls,
    nginx_bodies_and_continue => bodies_and_continue,
    nginx_slow_clients_timeouts_and_refusals => slow_clients_timeouts_and_refusals,
    nginx_shutdown_drains => shutdown_drains_through_the_proxy,
);

behind!(
    Kind::Caddy,
    caddy_forwarding_keep_alive_and_tls => forwarding_keep_alive_and_tls,
    caddy_bodies_and_continue => bodies_and_continue,
    caddy_slow_clients_timeouts_and_refusals => slow_clients_timeouts_and_refusals,
    caddy_shutdown_drains => shutdown_drains_through_the_proxy,
);

#[test]
fn marker_read_retries_only_interruptions_and_requires_the_marker() {
    struct ScriptedReads<'a> {
        steps: std::collections::VecDeque<io::Result<&'a [u8]>>,
    }
    impl Read for ScriptedReads<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let data = self
                .steps
                .pop_front()
                .expect("unexpected read past marker")?;
            assert!(data.len() <= buf.len());
            buf[..data.len()].copy_from_slice(data);
            Ok(data.len())
        }
    }
    let mut stream = ScriptedReads {
        steps: [
            Err(io::ErrorKind::Interrupted.into()),
            Ok(b"ti".as_slice()),
            Err(io::ErrorKind::Interrupted.into()),
            Ok(b"ck\n".as_slice()),
        ]
        .into(),
    };
    let mut received = Vec::new();
    read_until(&mut stream, &mut received, "tick").unwrap();
    assert_eq!(received, b"tick\n");
    for kind in [
        io::ErrorKind::TimedOut,
        io::ErrorKind::WouldBlock,
        io::ErrorKind::ConnectionReset,
    ] {
        let mut stream = ScriptedReads {
            steps: [Ok(b"ti".as_slice()), Err(kind.into())].into(),
        };
        let mut received = Vec::new();
        assert_eq!(
            read_until(&mut stream, &mut received, "tick")
                .unwrap_err()
                .kind(),
            kind
        );
        assert_eq!(received, b"ti");
    }
    let mut received = Vec::new();
    assert_eq!(
        read_until(&mut b"ti".as_slice(), &mut received, "tick")
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );
}
