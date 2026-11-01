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
