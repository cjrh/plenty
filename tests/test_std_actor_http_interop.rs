//! Independent clients and the worker server's pinned nginx/Caddy setups
//! exercise the actor server's streaming and lingering-close contracts.
use super::test_std_actor_http_corpus::Server;
use super::test_std_http_server_proxy::{Kind, Proxy, enabled};
use std::process::Command;

const SERVER: &str = include_str!("fixtures/actor_http/interop.plenty");

fn curl(args: &[&str], early: bool) -> Vec<u8> {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--http1.1",
            "--max-time",
            "10",
            "--noproxy",
            "*",
        ])
        .args(args)
        .output()
        .unwrap();
    // Caddy may reset its client after relaying an early response while that
    // client is still uploading. The upstream response must still arrive.
    assert!(
        output.status.success() || early && output.status.code() == Some(55),
        "curl {args:?}: {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn exchanges(url: impl Fn(&str) -> String) {
    let count = url("/count");
    let reused = curl(
        &["-w", "|%{http_code} %{num_connects}\n", &count, &count],
        false,
    );
    assert_eq!(
        reused, b"1|200 1\n2|200 0\n",
        "frontend and backend keepalive"
    );
    assert_eq!(curl(&[&url("/")], false), b"home");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upload");
    let payload: Vec<u8> = (0..700_000u32).map(|index| (index % 251) as u8).collect();
    std::fs::write(&path, &payload).unwrap();
    let upload = format!("@{}", path.display());
    for route in ["/echo", "/stream/echo"] {
        for header in [
            "Expect:",
            "Transfer-Encoding: chunked",
            "Expect: 100-continue",
        ] {
            assert_eq!(
                curl(
                    &["--data-binary", &upload, "-H", header, &url(route)],
                    false
                ),
                payload,
                "{route}, {header}"
            );
        }
    }
    assert_eq!(curl(&[&url("/large")], false), vec![b'x'; 262144]);
    assert_eq!(
        curl(
            &["-w", "%{http_code}", "-o", "/dev/null", &url("/error")],
            false
        ),
        b"500"
    );
    // Refused bodies are drained under a bound after the answer is sent, so
    // proxies relay the actual answer rather than replacing it with a 502.
    let refused = dir.path().join("refused");
    std::fs::write(&refused, vec![b'x'; 2 * 1024 * 1024]).unwrap();
    let refused = format!("@{}", refused.display());
    for route in ["/echo", "/stream/echo"] {
        for header in ["Expect:", "Transfer-Encoding: chunked"] {
            for _ in 0..4 {
                assert_eq!(
                    curl(
                        &[
                            "-w",
                            "%{http_code}",
                            "-o",
                            "/dev/null",
                            "--data-binary",
                            &refused,
                            "-H",
                            header,
                            &url(route)
                        ],
                        true
                    ),
                    b"413",
                    "large refused body {route}, {header}"
                );
            }
        }
    }
    // Under the body limit, the handler can respond before reading it.
    for route in ["/early", "/stream/early"] {
        for header in ["Expect:", "Transfer-Encoding: chunked"] {
            assert_eq!(
                curl(
                    &[
                        "-w",
                        "|%{http_code}",
                        "--data-binary",
                        &upload,
                        "-H",
                        header,
                        &url(route)
                    ],
                    true
                ),
                b"early|200"
            );
        }
    }
    assert_eq!(curl(&[&url("/")], false), b"home");
}

#[test]
fn actor_http_curl_streaming_and_refused_uploads() {
    if Command::new("curl").arg("--version").output().is_err() {
        eprintln!("curl unavailable; actor interoperability test skipped");
        return;
    }
    let server = Server::start_with(SERVER);
    exchanges(|path| format!("http://127.0.0.1:{}{path}", server.port));
    server.finish();
}

fn proxy_exchanges(kind: Kind, name: &str) {
    if !enabled(name) {
        return;
    }
    let server = Server::start_with(SERVER);
    let proxy = Proxy::start(kind, server.port);
    exchanges(|path| proxy.url(path));
    let mut tls = proxy.tls_args();
    tls.push(proxy.tls_url("/"));
    assert_eq!(
        curl(&tls.iter().map(String::as_str).collect::<Vec<_>>(), false),
        b"home"
    );
    drop(proxy);
    server.finish();
}

#[test]
fn actor_http_nginx_streaming_refusals_keepalive_and_proxy_tls() {
    proxy_exchanges(Kind::Nginx, "actor_http_nginx");
}

#[test]
fn actor_http_caddy_streaming_refusals_keepalive_and_proxy_tls() {
    proxy_exchanges(Kind::Caddy, "actor_http_caddy");
}

#[test]
fn actor_http_interop_fixture_type_checks_without_clients() {
    // curl/Docker availability must not hide fixture arithmetic diagnostics.
    plenty::check_source(SERVER).unwrap();
}

#[test]
fn actor_http_interop_pump_preserves_short_write_and_error_progress() {
    // Run the actual fixture pump against the actual Response implementation.
    // Same-module composition permits bounded output draining without exposing
    // response internals in the public API or relying on socket timing.
    let application = SERVER
        .split("type Error =")
        .nth(1)
        .unwrap()
        .split("def factory(")
        .next()
        .unwrap();
    let source = format!(
        r#"{}

type Error ={application}

def main() -> Result[(), Error | IoError]:
    mut body = std.bytes.empty()?
    body.resize(1024, 120u8)?
    mut app = App(0, 0, body, 0)
    mut reply = response(512)?
    broken = app.pump(&mut reply)
    before_start = broken.is_err() and app.at == 0 and not reply.is_started()
    mut head = std.http.response(200, "OK")?
    head.add("Content-Length", "1024")?
    reply.start(head)?
    mut empty_app = App(0, 0, std.bytes.empty()?, 0)
    mut empty_reply = response(512)?
    mut empty_head = std.http.response(200, "OK")?
    empty_head.add("Content-Length", "0")?
    empty_reply.start(empty_head)?
    print("__test_begin_no_allocations__")?
    first = app.pump(&mut reply)
    at = app.at
    short = first.is_ok() and at > 0 and at < app.body.len() and not reply.is_done()
    again = app.pump(&mut reply)
    blocked = again.is_ok() and app.at == at and reply.space() == 0
    for _ in range(3):
        if not reply.is_done():
            reply._output.clear()
            app.pump(&mut reply)?
    complete = app.at == 1024 and reply.is_done()
    after_finish = app.pump(&mut reply)
    unchanged = after_finish.is_err() and app.at == 1024
    empty_app.pump(&mut empty_reply)?
    empty = empty_app.at == 0 and empty_reply.is_done()
    print("__test_end_no_allocations__")?
    print(before_start, short, blocked, complete, unchanged, empty)?
    Ok(())
"#,
        include_str!("../stdlib/std/actor/http/server.plenty"),
    );
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.plenty");
        let executable = dir.path().join("program");
        std::fs::write(&input, &source).unwrap();
        let options = plenty::CompileOptions {
            optimize,
            test_hooks: true,
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&input, &executable, None, &options)
            .unwrap_or_else(|error| panic!("{source}\n{error}"));
        let output = Command::new(executable).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let actual = stdout
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(actual, "True True True True True True");
    }
}

#[test]
fn actor_http_interop_handler_overflow_uses_existing_500_contract() {
    if Command::new("curl").arg("--version").output().is_err() {
        eprintln!("curl unavailable; actor handler overflow transport test skipped");
        return;
    }
    // Force the fixture's independently declared arithmetic error at the next
    // Head callback, without sending a lifetime's worth of HTTP requests.
    assert!(SERVER.contains("Ok(App(0, 0,"));
    let source = SERVER.replacen("Ok(App(0, 0,", "Ok(App(0, i64.MAX,", 1);
    let server = Server::start_with(&source);
    let url = format!("http://127.0.0.1:{}/count", server.port);
    assert_eq!(
        curl(&["-w", "%{http_code}", "-o", "/dev/null", &url], false),
        b"500"
    );
    // Handler failure before a response is recoverable: no actor failure or
    // trap is hidden by the 500, and all owned heaps are reclaimed on drain.
    server.finish();
}
