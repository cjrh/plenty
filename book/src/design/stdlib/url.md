# std.url

`import std.url` parses and formats absolute URLs, percent-encodes data for a
named URL component, and splits and builds query strings. It has one error
type, `UrlError`.

The module is checked Plenty code over [`std.bytes`](bytes.md), with no native
primitives of its own. It is available on the supported x86-64 Linux GNU
target. A URL is not a filesystem path: [`std.path`](path.md) is a separate
type and nothing converts between them.

## The accepted syntax

`std.url` implements the generic syntax of RFC 3986, with these stated
differences. It is not the WHATWG URL Standard and does not imitate browsers.

| Rule | Behavior |
|---|---|
| Scheme | Required. Relative references such as `/a/b`, `//host/a`, and `?q` are `MissingScheme` |
| Characters | ASCII only, exactly the characters RFC 3986 allows in each component. Spaces, controls, `"<>\^{}` and the backtick, `[`/`]` outside an address literal, and all non-ASCII text are `InvalidByte` |
| Escapes | `%` must be followed by two hex digits of either case, or the URL is `InvalidEscape` |
| Userinfo | Everything before the first `@` of the authority. A second `@` is an error, never a choice of host |
| Host | A registered name, or `[`IPv6 address`]`, or `[v`...`]` (IPvFuture). Empty is allowed. A dotted IPv4 address is an ordinary registered name |
| IPv6 | The RFC 3986 `IPv6address` grammar: one optional `::`, an optional trailing dotted IPv4 address whose octets have no leading zeros. Zone identifiers (`%25eth0`) are rejected |
| Port | Narrower than the RFC: one to five digits, no leading zero, at most `65535`. An empty port (`http://h:/`) is `InvalidPort` |
| Internationalized names | No IDNA processing. A Unicode host must already be Punycode (`xn--...`) or percent-encoded UTF-8 |
| Normalization | None. Case, `.` and `..` segments, escapes, default ports, and empty components are kept exactly as written |

Because nothing is normalized and the port has one spelling, every accepted URL
formats back to the identical text.

## Url

```text
pub class Url:
    pub scheme: str
    pub userinfo: Option[str]
    pub host: Option[str]
    pub port: Option[i64]
    pub path: str
    pub query: Option[str]
    pub fragment: Option[str]
```

Components hold the text between the delimiters, still percent-encoded and
without the delimiters themselves. `Nothing` means the delimiter was absent;
`Some("")` means it was present with nothing after it, so `s:`, `s:?`, and
`s://` are three different values. `host` is `Nothing` exactly when the URL has
no `//` authority. An address literal keeps its brackets: `[::1]`.

| Function | Result | Contract |
|---|---|---|
| `std.url.parse(text: &str, max_bytes)` | `Result[Url, UrlError]` | Split and validate `text`, whose UTF-8 length must not exceed `max_bytes` |
| `url.format()` | `Result[str, UrlError]` | Validate every component and join them |

`parse` checks the length first: `InvalidLimit` for a negative `max_bytes`,
`LimitExceeded` for a longer text. It then reports the first violation from
left to right.

`format` exists so that a `Url` built or edited by a program cannot produce
text that parses differently. It applies the rules of `parse` to each field,
and also returns:

- `MissingHost` when `userinfo` or `port` is present without `host`;
- `InvalidPort` when `port` is outside `0` to `65535`;
- `InvalidByte` when `host` is present and a nonempty `path` does not start
  with `/`, or `host` is absent and `path` starts with `//`.

A successful `format` result always parses back to equal components.

Compare URLs only after deciding what equivalence means for your scheme. The
module does not lowercase schemes or hosts and does not decode escapes, so
`HTTP://A/%7e` and `http://a/~` are different values.

## Percent encoding

| Function | Result | Contract |
|---|---|---|
| `encode(text: &str, component)` | `Result[str, UrlError]` | Escape the UTF-8 bytes of `text` for `component` |
| `encode_bytes(source: &Bytes, start, stop, component)` | `Result[Bytes, UrlError]` | The same for a byte range |
| `decode(text: &str)` | `Result[str, UrlError]` | Replace each `%hh`; the result must be UTF-8 |
| `decode_bytes(source: &Bytes, start, stop)` | `Result[Bytes, UrlError]` | Replace each `%hh` in a byte range |

Ranges follow the `std.bytes` [range rule](bytes.md#the-range-rule).

Letters, digits, and `-._~` are never escaped. `%` is always escaped, so
encoding text that already contains escapes encodes it again. Escapes are
written with uppercase hex digits. Each `Component` additionally keeps:

| Component | Also kept | Use for |
|---|---|---|
| `Userinfo` | `!$&'()*+,;=` | A user name or a password; `:` is escaped, so join them yourself |
| `Segment` | `!$&'()*+,;=:@` | One path segment; `/` is escaped |
| `Path` | `!$&'()*+,;=:@/` | A whole path whose `/` are separators |
| `Query` | `!$&'()*+,;=:@/?` | A whole query string |
| `QueryPart` | `!$'()*,;:@/?` | One key or one value; `&`, `=`, and `+` are escaped |
| `Fragment` | `!$&'()*+,;=:@/?` | A fragment |

`Userinfo` output is also valid in a registered host name.

Decoding is the same for every component: `%hh` becomes one byte and every
other byte is copied. `+` stays `+`; the `application/x-www-form-urlencoded`
reading of `+` as a space is not implemented. `decode` returns `InvalidUtf8`
when the bytes are not UTF-8; use `decode_bytes` for binary data.

Decoded text can contain any character, including `/`, `?`, NUL, and line
breaks. Split a URL into components before decoding, and never parse decoded
text as a URL or a path.

## Query strings

```text
pub class Pair:
    pub key: str
    pub value: str
```

| Function | Result | Contract |
|---|---|---|
| `query_pairs(query: &str)` | `Result[list[Pair], UrlError]` | Split at `&`, split each piece at its first `=`, decode both sides |
| `query_text(pairs: &list[Pair])` | `Result[str, UrlError]` | Join `key=value` with `&`, each side encoded as `QueryPart` |

Every `&`-separated piece is one pair, in order, including empty pieces and
repeated keys. A piece without `=` has an empty value, so `a` and `a=` read the
same; `query_text` always writes the `=`. The empty query has no pairs.
`query_pairs` accepts any byte that is not part of a malformed escape; pass it a
`Url.query` to get the stricter component check.

## Errors

```text
pub enum UrlError:
    InvalidLimit
    LimitExceeded
    OutOfRange
    MissingScheme
    MissingHost
    InvalidByte(i64)
    InvalidEscape(i64)
    InvalidHost(i64)
    InvalidPort(i64)
    InvalidUtf8
    Allocation(AllocError)
```

| Variant | Meaning |
|---|---|
| `InvalidLimit` | `max_bytes` is negative |
| `LimitExceeded` | The text is longer than `max_bytes` |
| `OutOfRange` | A byte range lies outside its buffer |
| `MissingScheme` | No scheme and `:` before the first `/`, `?`, or `#` |
| `MissingHost` | `format` was given userinfo or a port without a host |
| `InvalidByte(index)` | The byte at `index` is not allowed where it appears |
| `InvalidEscape(index)` | The `%` at `index` is not followed by two hex digits |
| `InvalidHost(index)` | The address literal starting at `index` is malformed, or the byte at `index` follows its `]` and is not `:` |
| `InvalidPort(index)` | The port starting at `index` is not an accepted number |
| `InvalidUtf8` | Decoded bytes are not UTF-8 |
| `Allocation(cause)` | Storage could not be allocated |

An index is a UTF-8 byte offset into the input. For `format` it is an offset
into the text that would have been produced; for the byte-range functions it
is a position in the source buffer, not relative to `start`. A failed
operation releases anything it allocated. Wrap a result with
[`std.error.context`](error.md) to name the operation.

## Ownership and threads

`Url` and `Pair` own their strings, so they are resources: assignment moves
them, and the ordinary [loan rules](../language/05-ownership/02-borrowing.md)
apply to their fields. All inputs are shared loans. The module holds no global
state; its functions pass the ordinary worker-effect checks and may run in any
thread.

## Costs

Every operation is linear in its input and allocates in proportion to it.

| Operation | Allocates |
|---|---|
| `parse` | One copy of the text, then one string per present component. A text of more than `max_bytes` UTF-8 bytes is rejected before the copy, so the copy never exceeds `max_bytes` |
| `format` | One growing buffer, a temporary per component, and the result |
| `encode_bytes`, `decode_bytes` | One buffer of exactly the output size, after validation |
| `encode`, `decode` | A `Bytes` copy of the text, the coded buffer, then the `str` |
| `query_pairs` | A copy of the query, the list, and two strings per pair |
| `query_text` | One growing buffer and two temporaries per pair |

Encoded output is at most three times its input.

Every byte goes through checked single-element `Bytes` calls, several times
per byte. Measured with a release compiler on x86-64 Linux, pinned to one
core, parsing one 91-byte URL with all seven components takes about 49 µs:
roughly half a microsecond per input byte.

## Limits

- No relative references and no reference resolution (RFC 3986 section 5).
- No normalization or equivalence: no case folding, dot-segment removal,
  escape normalization, or default-port knowledge.
- No IDNA or Punycode conversion, and no scheme-specific rules such as
  `http` requiring a host.
- No IPv6 zone identifiers. IPv4 addresses are not distinguished from
  registered names.
- No form encoding (`+` as a space) and no `;` pair separator.
- Query pairs are a list. There is no keyed lookup helper; scan the list.
