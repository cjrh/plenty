# `std.http.cookie`

`import std.http.cookie` reads the `Cookie` fields of a request and writes
`Set-Cookie` fields (RFC 6265). Reading is lenient; writing is strict, so no
value can add an attribute or a header field. The module is checked Plenty
in `stdlib/std/http/cookie.plenty` and adds no runtime primitives.

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
valid cookies is `TooManyCookies`.

Values are not decoded. A cookie's value is whatever the client sends; check
it, or sign it, before trusting it.

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
`TooManyCookies`, `Http(HttpError)` from `add`, and `Allocation`.

## Limits

- No `Expires` (use `max_age`), `Partitioned`, or `Priority` attributes.
- No signing or encryption; values are sent as given. A session identifier
  should be a random token from [`std.secrets`](secrets.md), looked up on
  the server.
- `Set-Cookie` fields sent by another server are not parsed.
