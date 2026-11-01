# Reverse-proxy tests

`tests/test_std_http_server_proxy.rs` runs `std.http.server` behind nginx and
Caddy (see [reverse proxies](../design/stdlib/http-qualification.md#reverse-proxies)).
They need Docker and curl, so they run only when asked:

```sh
PLENTY_DOCKER_TESTS=1 cargo test --locked --test integration test_std_http_server_proxy::
```

Without the variable each test prints that it is skipped and passes, so the
ordinary suite needs no Docker and does not slow down. With it, a missing
Docker daemon or curl fails the tests. The first run pulls the two pinned images
(about 70 MB each). Each test starts its own containers with `--network host`
on free loopback ports, names them `plenty-proxy-test-<pid>-<n>` with the label
`plenty-test=proxy`, and removes them when it ends, also on failure; nothing
else is stopped or pruned. A test process killed outright leaves its
containers running; list them with
`docker ps --filter label=plenty-test=proxy`.

CI runs them in the `proxy_tests` job of `.github/workflows/ci.yml` on
GitHub's `ubuntu-latest`, which has Docker and curl. The job pulls
`nginx:1.30.6-alpine3.24` and `caddy:2.11.7-alpine` in an earlier step, so
pulls are not timed with the tests; when the tests change an image tag,
change the job's pull step to match.
