# std.secrets

`import std.secrets` returns unpredictable bytes from the operating system as
buffers, text tokens, and integers, with one error type, `SecretsError`.

Use it for session tokens, keys, nonces, and password-reset links. For
repeatable simulation and sampling use [`std.random`](random.md); the two
modules share no state and no code path.

Policy is checked Plenty code over one native primitive,
[`Bytes.read_os_entropy`](bytes.md#transport), which makes a single
`getrandom(2)` request. The module is available on the supported x86-64 Linux
GNU target and needs glibc 2.25 or later and Linux 3.17 or later.

```plenty
import std.secrets
from std.secrets import SecretsError

def main() -> Result[(), IoError | SecretsError]:
    session = std.secrets.token_urlsafe(32)?
    reset_code = std.secrets.token_hex(16)?
    roll = std.secrets.below(6u64)? + 1u64
    # The values differ on every run; their shapes do not.
    print(len(session), len(reset_code), roll >= 1u64 and roll <= 6u64)?
    Ok(())
```
```output
43 32 True
```

## Functions

| Function | Result | Contract |
|---|---|---|
| `fill(target: &mut Bytes, start: i64, stop: i64)` | `Result[(), SecretsError]` | Overwrite `target[start:stop]` with secret bytes |
| `token_bytes(count: i64)` | `Result[Bytes, SecretsError]` | A new buffer of exactly `count` secret bytes |
| `token_hex(count: i64)` | `Result[str, SecretsError]` | `count` secret bytes as `2 * count` lowercase hex digits |
| `token_urlsafe(count: i64)` | `Result[str, SecretsError]` | `count` secret bytes as unpadded base64url, `ceil(4 * count / 3)` characters |
| `random_u64()` | `Result[u64, SecretsError]` | A uniform `u64` |
| `below(bound: u64)` | `Result[u64, SecretsError]` | Uniform in `[0, bound)` without modulo bias; zero is `EmptyRange` |

`count` is a number of bytes, not characters, and has no default: choose it for
the secret's purpose. Thirty-two bytes is ample for a bearer token. A negative
count is `OutOfRange`; zero gives an empty result without consulting the
system. `fill` follows the [range rule](bytes.md#the-range-rule). The text
forms use the [`std.encoding`](encoding.md) codecs.

## Failure policy

- **No fallback.** If the system cannot supply entropy, the operation returns
  `SecretsError.Entropy`. It never substitutes the clock, an address, a seeded
  generator, or `/dev/urandom`.
- **Blocking.** A request waits until the kernel generator has been
  initialized, which matters only very early in boot. There is no nonblocking
  form.
- **Short and interrupted requests are retried** until the range is full. A
  request that reports zero bytes for a nonempty range is `UnexpectedEof`; a
  count larger than the request is `InvalidProgress`.
- **No partial secrets.** When `fill` fails, the whole requested range is
  zero. A failed token call releases its buffer and returns only the error.
- **Scrubbing.** `token_hex`, `token_urlsafe`, `random_u64`, and `below` zero
  their intermediate byte buffer before releasing it. The values they return,
  and a `token_bytes` buffer, are the caller's to protect: Plenty does not
  zero memory on drop, and the encoder's own intermediate buffer is not
  scrubbed.

## Sources

The functions above are shorthand for methods on `Source[R]`, which applies
the same policy to any byte reader.

| Item | Contract |
|---|---|
| `os()` | `Source[OsEntropy]`, the operating-system source |
| `source_from[R: Reader](inner: R)` | `Source[R]` over a [`std.io`](io.md) reader |
| `Source[R].fill`, `token_bytes`, `token_hex`, `token_urlsafe`, `next_u64`, `below` | As the functions above, with `self: &mut Source[R]`; `next_u64` is `random_u64` |
| `Source[R].inner` | The reader, public so its owner can inspect or replace it |
| `os_entropy()` | `OsEntropy`, a `std.io` `Reader` whose `read` makes one request and may return a short count |

`source_from` exists to test code against scripted short reads and failures,
and to use a hardware device opened as a stream. The secrecy of its output is
exactly that of the reader supplied; only `os()` is backed by the kernel
generator. `OsEntropy` has a private field and can only be made by
`os_entropy()` and `os()`.

`next_u64` reads eight bytes as a little-endian integer. `below` draws
`next_u64()` until the value is at least `2^64 mod bound` and returns it modulo
`bound`.

## Errors

```text
pub enum SecretsError:
    OutOfRange
    EmptyRange
    Entropy(StreamError)
    Allocation(AllocError)
```

| Variant | Meaning |
|---|---|
| `OutOfRange` | A negative count or a range outside the buffer; the source was not consulted |
| `EmptyRange` | `below(0)` |
| `Entropy(cause)` | The source failed; `cause` is the [`StreamError`](io.md) |
| `Allocation(cause)` | A token or scratch buffer could not be allocated |

From the operating system, `cause` is `StreamError.Io(IoError.System(code))`
with the `getrandom` error number: for example `38` (`ENOSYS`) on a kernel
without the call, or under a sandbox that denies it.

## Ownership and threads

Targets are exclusive loans. `Source[OsEntropy]` and `OsEntropy` hold no
descriptor and no buffered bytes; creating one costs nothing and they need no
closing. The module has no global state, passes the ordinary worker-effect
checks, and may be used from any thread. Each request goes to the kernel, so
threads never share generator state in the process, and a forked child cannot
repeat its parent's values.

## Costs

| Operation | Allocates | System requests |
|---|---|---|
| `fill` | Nothing | One per 33,554,431 bytes at most; one for up to 256 bytes |
| `token_bytes(count)` | One buffer of `count` bytes | As `fill` |
| `token_hex`, `token_urlsafe` | The token buffer, the encoded buffer, and the `str` | As `fill` |
| `random_u64` | One 8-byte scratch buffer | One |
| `below` | As `random_u64`, per attempt | Fewer than two expected |

Nothing is buffered between calls, so every operation is at least one system
call. To draw many small values, `fill` one buffer and decode it, or seed a
[`std.random`](random.md) generator when the values need not be secret.
Throughput has not been measured on an idle machine.

## Limits

- Linux only, through `getrandom(2)`; there is no `/dev/urandom` path for
  kernels older than 3.17.
- No nonblocking or deadline-bounded request.
- No key derivation; see [backlog](../../backlog.md) B67. Digests, HMAC, and
  constant-time comparison are in [`std.crypto`](crypto.md).
- No zero-on-drop buffer type.
- No choice or shuffle helpers; combine `below` with indexing.
