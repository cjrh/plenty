# Owned response typestate experiment

This is a deterministic response-lifecycle model using explicit owned receivers.
It is not an HTTP implementation or a replacement for `std.http.server`.
The assessment is in
[`typestate-response-experiment.md`](../../book/src/proposals/typestate-response-experiment.md).

From this worktree, build the compiler into a disk-backed target directory and run:

```sh
CARGO_TARGET_DIR=/path/to/plenty/target cargo build --locked --bin plenty
python3 -B experiments/typestate/check.py \
  --compiler /path/to/plenty/target/debug/plenty \
  --output /path/to/plenty/target/typestate-spike
```

`response.plenty` contains private resource storage and public state types.
`scenarios.plenty` exercises legal transitions, retry ownership, terminal partial
writes, cancellation, abandonment, and error propagation. A destructor prints
connection ID and accepted byte count so the runner can assert exactly-once
cleanup and ordering. Its `unwrap` is test instrumentation, not production
cleanup advice. The wire simulates counts only; it does not encode or transmit
bytes, implement deadlines, or model real socket failures.

`check.py` builds and runs a native program, then checks 12 rejected programs
across the module boundary, including whole use of an owner after a field has
moved out of it.
Temporary files are removed
after each run; compiler/linker failures and timeouts fail the runner.

The accompanying compiler change adds consuming methods; `std.text.Builder.finish`
is its first stdlib adoption. The model itself is not production HTTP code.
