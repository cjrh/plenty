# Explicit error composition prototype

Run from the repository root:

```sh
cargo run -- examples/error_composition/main.plenty
cargo test --locked --all-features --test integration -- test_error_composition:: test_typed_stream_prototype::
```

The example prints `1` (bytes delivered) and `104` (the original transport
code). `Wire` is scripted; it performs no network I/O.

`streams.plenty` keeps transport errors generic and distinguishes interruption
from terminal failure. `http_body.plenty` implements only fixed-length HTTP
body framing, after a caller has validated the head and response semantics.
`copy_all` preserves the failing side and bytes delivered to the writer.
There is no stored error to retrieve after a failed call.

Each transport attempt reports either successful progress or an error with no
progress from that attempt. An interrupted attempt is retried; cancellation
must be a terminal `Source(E)` cause. Repeated interruption can retry indefinitely:
these helpers do not impose their own deadline or cancellation token. They do
not flush the writer, and copy progress counts delivered bytes, not consumed
reader bytes that were lost after a write failure.

These modules are experimental examples, not new `std.io` APIs. They do not
implement HTTP header parsing, chunking, TLS, deadlines, or connection reuse.
See the [proposal and findings](../../book/src/proposals/completed/error-composition.md)
for the scope, tradeoffs, and production migration boundary.

Separately, `std.error.map_error` is implemented as an ordinary library helper
and used by representative HTTP/JSON converters. Worker-reachable code uses
concrete callbacks (`def once (...)`) so the existing thread effect checker can
inspect their bodies; passing a thin function value through a generic helper
still has the language's existing worker restriction.
