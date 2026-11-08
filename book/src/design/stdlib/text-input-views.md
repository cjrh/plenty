# Text inputs and substring views

Read-only text parameters use `&strview` when the implementation can consume a
selected UTF-8 range. Pass a whole `str`, an ordinary `&str`, or an existing
view. A substring borrow selects Unicode scalar positions; the callee reads
only the selected bytes. Local view bindings still require an explicit borrow.
Ordinary `&str` remains a reference to an owned string handle, including in
generic APIs. See [text](../facilities/01-builtins/text.md) and
[slices](../language/04-expressions/slices.md) for the language rules.

```plenty
import std.net

def main() -> Result[(), IoError | AllocError | std.net.NetError]:
    line = "address=127.0.0.1:80;"
    address = std.net.parse_address(&line[8:20])?
    print(address.port)?
    Ok(())
```
```output
80
```

## Runtime-backed readers

These inputs accept `&strview`; other parameter types and result types retain
their existing contracts. The [signature index](signatures.md) lists the full
declarations.

| Module | Text inputs |
| --- | --- |
| [`std.actor.net`](actor-net.md) | `lookup` host |
| [`std.actor.tls`](actor-tls.md) | `client` server name |
| [`std.crypto`](crypto.md) | Digest/Mac `update_text`, BLAKE3 derivation context |
| [`std.db.kv`](kv.md) | Table names and `connect` path |
| [`std.db.sqlite`](sqlite.md) | Connection path, SQL, extension path/entrypoint, parameter name, bound text |
| [`std.env`](env.md) | `get` name |
| [`std.io`](io.md) | `open_file` path and mode; `open_path` mode |
| [`std.net`](net.md) | IP/address parsing and resolver host |
| [`std.path`](path.md) | `from_text` input |
| [`std.plot`](plot.md) | `Plot.save` path |
| [`std.process`](process.md) | Command, argument, shell script and environment name/value |
| [`std.tempfile`](tempfile.md) | File prefix/suffix and directory prefix |
| [`std.tls`](tls.md) | PEM inputs, application protocols and server names |
| [`std.bytes`](bytes.md) | `Bytes.matches_text` pattern; its offset remains a **byte** offset |

The native boundary carries the view's address and byte count, with UTF-8 and
scalar boundaries established by the compiler. Arbitrary byte views do not
become text implicitly. Hashing and IPv4 parsing can read the selected range
without an owned text copy. APIs that retain input, such as SQLite text binding,
TLS configuration, command construction and asynchronous name lookup, copy it
before returning. Those copies preserve the existing fallible allocation and
error contracts; a view does not make retained storage free.

Embedded NUL handling belongs to each API. Hashing and bound SQLite text retain
NUL bytes. Native paths, environment names and command arguments keep their
existing validation. NUL bytes outside a selected view are irrelevant. File
open modes retain their compile-time validation rules. View support does not
relax path, SQL, TLS or mode validation.

## Scope

The standard-library text-input inventory separates 49 runtime-backed reader
declarations in 13 modules from 127 Plenty-level readers. The generated
signature index is authoritative for implemented signatures. The deliberately
unchanged cases are two MIME getters returning ordinary `&str`, three serial
protocol helpers, nine JSON protocol callbacks and four TOML protocol callbacks:
returning references and user protocol signatures need their own contracts.

Views remain two words. Scalar `len()` scans a view; `byte_len()` and
[`as_bytes()`](bytes-stringlike.md) give constant-time access to its byte extent.
Cache scalar counts in user code when repeated counts are useful. Indexed
scalar access still traverses UTF-8; caching a count does not add an index.
