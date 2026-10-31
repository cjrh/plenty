# Incremental HTTP decoder fuzzing

The decoder target uses the HTTP server's existing libFuzzer runner, wire
corpus and dictionary. From the repository root:

```sh
python3 fuzz/http_server/run.py --target decoder --plenty target/release/plenty --time 60 --seed 592601
```

The compiler's `--fuzz` command links libFuzzer and instruments the generated
code for coverage; this is not an AddressSanitizer or ThreadSanitizer run.
`--runs N` replaces the time budget; `--jobs N` runs independent workers
sharing the corpus. Build requirements and the worker-server target are in
[HTTP qualification](../../book/src/design/stdlib/http-qualification.md#fuzzing).

The first input byte selects feed sizes from 1 to 64 bytes, small limits,
and alternating single-byte feeds. Each input is also decoded with a
4096-byte feed. The results must agree on completed requests, body length,
body checksum and framing errors. Checks cover bounded progress, explicit
EOF, completion without consuming a pipelined request, and reset state.
Inputs are capped at 65536 bytes; the runner normally mutates up to 4096.
The shared corpus also seeds longer header-limit cases.

Expected malformed input returns `HttpError`. Decoder or harness traps occur
outside an actor and therefore crash the fuzz process, producing an artifact;
actor trap containment cannot hide them. Actor resource and listener recovery
are exercised separately by the real-socket corpus and mutation tests.

The default work directory is `target/fuzz-http-decoder`: `seeds/` holds the
shared corpus, `corpus/` retains coverage-increasing inputs, and `findings/`
holds crash or timeout artifacts. Existing findings are preserved. The runner
returns nonzero for a failing fuzzer or a new finding. Reproduce one artifact:

```sh
target/fuzz-http-decoder/http_decoder_fuzzer target/fuzz-http-decoder/findings/crash-HASH
```

Keep run statistics with the qualification report: compiler revision,
command and seed, duration, executions, coverage/corpus sizes and findings.
A finite fuzz run supplements the contract tests; it does not prove that all
malformed inputs are safe.
