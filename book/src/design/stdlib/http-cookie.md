# `std.http.cookie`

`import std.http.cookie` reads the `Cookie` fields of a request and writes
`Set-Cookie` fields (RFC 6265). Reading is lenient; writing is strict, so no
value can add an attribute or a header field. The module is checked Plenty
in `stdlib/std/http/cookie.plenty` and adds no runtime primitives; signing
uses [`std.crypto`](crypto.md).

## Reading

| Item | Meaning |
|---|---|
| `Cookie(name, value)` | One cookie a client sent |
| `cookies(&headers, limit)` | `Result[list[Cookie], CookieError]`: every valid cookie in the `Cookie` fields, in order |
| `find(&cookies, name)` | `Option[i64]`: index of the first cookie named `name` |

A pair whose name is not a token or whose value has a byte outside RFC
6265's cookie-octets (control bytes, space, `"`, `,`, `;`, `\`) is skipped,
as is a piece without `=`. The field carries cookies set by every
application on the host, and one malformed cookie must not make every
request fail. A value wrapped in double quotes loses them. More than `limit`
valid cookies is `TooManyCookies`. Each field is copied once into a byte
buffer and checked with bulk scans; only valid cookies are copied out as
text, so a skipped pair allocates nothing.

Values are not decoded. A cookie's value is whatever the client sends; check
it, or [sign](#signing) it, before trusting it.

## Writing

| Item | Meaning |
|---|---|
| `set_cookie(name, value)` | `Result[SetCookie, CookieError]`: a cookie with the defaults below |
| `removal(name)` | A cookie that deletes `name`: empty value, `Max-Age=0` |
| `SetCookie` | Public fields `name`, `value`, `path`, `domain`, `max_age: Option[i64]`, `secure`, `http_only`, `same_site: SameSite` |
| `cookie.format()` | `Result[str, CookieError]`: the field value, after checking every part |
| `add(&mut head, &cookie)` | Append a `Set-Cookie` field to a `ResponseHead` |
| `SameSite` | `Strict`, `Lax`, or `Unrestricted` (`SameSite=None`) |

`set_cookie` gives `Path=/`, `Secure`, `HttpOnly`, `SameSite=Lax` and no
`Max-Age`, which ends the cookie with the browser session. Change the
fields to relax them; `format` checks the result each time:

| Rule | Error |
|---|---|
| Name is a non-empty token | `InvalidName` |
| Value has only cookie-octets | `InvalidValue` |
| `path` is empty or starts with `/` and has only visible ASCII and space, no `;` | `InvalidAttribute` |
| `domain` has only letters, digits, `-` and `.`, and does not start or end with `.` | `InvalidAttribute` |
| `max_age` is not negative | `InvalidAttribute` |
| `SameSite=None` needs `secure` | `InvalidAttribute` |
| A `__Secure-` name needs `secure` | `InvalidAttribute` |
| A `__Host-` name needs `secure`, path `/` and no domain | `InvalidAttribute` |

Empty `path` and `domain` are left out. `CookieError` also has
`TooManyCookies`, `Http(HttpError)` from `add`, `Crypto(CryptoError)` from
the HMAC in the signing functions, and `Allocation`.

## Signing

| Item | Meaning |
|---|---|
| `sign(&keys, name, value)` | `Result[str, CookieError]`: `value.<tag>` |
| `verify(&keys, name, signed)` | `Result[Option[str], CookieError]`: the value when the tag matches under any key, else `Nothing` |
| `signed_cookie(&keys, name, value)` | `Result[SetCookie, CookieError]`: `set_cookie(name, sign(...))` |
| `find_signed(&cookies, &keys, name)` | `Result[Option[str], CookieError]`: `verify` on each cookie named `name`, in order; the first whose tag verifies; `Nothing` when none does |
| `min_key` | `32`: shortest key, in bytes |

The tag is HMAC-SHA256 over `name=value` under a key, in unpadded base64url
(43 characters) after a `.`. The name is covered, so a value signed for one
cookie is refused under another. `keys` is a `list[Bytes]`: `sign` uses the
first, and `verify` tries every key without stopping at a match, each with
the constant-time comparison of `Mac.verify`, so the time taken depends only
on the number of keys. To rotate, put the new key first and keep the old
one listed until cookies signed with it have expired.

An empty list or a key shorter than `min_key` is `InvalidKey`; `sign` also
checks the name and value as `set_cookie` does. A malformed or mismatched
tag is `Nothing`, not an error. Signing allocates the result text and a
fixed-size HMAC state; verification allocates the decoded tag and the
returned value, and nothing per key beyond the HMAC state.

Signing stops changes, not reading: the value travels in clear. It does
not expire or bind the cookie to a client; put an expiry time in the value,
or keep sessions on the server, when that matters.

## Limits

- No `Expires` (use `max_age`), `Partitioned`, or `Priority` attributes.
- No encryption; a signed value is still readable. A session identifier
  can instead be a random token from [`std.secrets`](secrets.md), looked up
  on the server.
- `Set-Cookie` fields sent by another server are not parsed.
