# `std.http.body`

`import std.http.body` reads request bodies inside a
[`std.http.server`](http-server.md) handler: URL-encoded forms, JSON
documents and multipart uploads. Every reader checks the request's
`Content-Type`, works within caller limits, and fails with a `BodyError`
that maps to a response status. The module is checked Plenty in
`stdlib/std/http/body.plenty` and adds no runtime primitives. The lesson
[Read forms, cookies and forwarded clients](../../tutorial/11-concurrency/http-forms.md)
uses it.

## Errors

| Variant | Meaning | `status` |
|---|---|---|
| `UnsupportedMediaType` | `Content-Type` missing, malformed or not the expected type | 415 |
| `TooLarge` | A byte limit was passed, or `Content-Length` declares more than it | 413 |
| `TooManyFields` | More fields or parts than allowed | 413 |
| `Malformed` | Bad percent-escape, multipart structure, boundary or part header | 400 |
| `InvalidUtf8` | A decoded name or value, or a multipart field, is not UTF-8 | 400 |
| `Http(HttpError)` | Reading the body failed; `BodyTooLarge` is 413, `Stream` (timeout, cancellation, reset) 408 | 413, 408, 400 or 500 |
| `Json(JsonError)` | The document is invalid (400) or over `max_input` (413) | 400 or 413 |
| `Storage(StreamError)` | Writing an upload to its temporary file failed | 500 |
| `InvalidLimit` | A negative limit | 500 |
| `Allocation(AllocError)` | Memory ran out | 500 |

`status(&error) -> i64` gives the status, and
`reject(exchange, &error) -> Result[(), HttpError]` answers with it and its
reason as plain text. The server's own fallback only maps a failed body
read, so a handler that wants `415` or `400` for a bad form calls `reject`
instead of returning the error.

## Forms

| Item | Meaning |
|---|---|
| `FormLimits(max_bytes, max_fields)` | Public fields; `form_limits()` is 64 KiB and 256 fields |
| `read_form(exchange, &limits)` | `Result[list[Pair], BodyError]`: read an `application/x-www-form-urlencoded` body |
| `parse_form(&bytes, &limits)` | Decode a form already in memory |
| `parse_form_text(&text, &limits)` | Decode text, such as `exchange.query` |
| `find(&pairs, key)` | `Option[i64]`: index of the first pair named `key` |

`Pair(key, value)` is [`std.url`](url.md)'s. Pairs keep their order and
duplicates. `+` decodes to a space and `%hh` to a byte; a `%` without two
hex digits is `Malformed`, and the decoded bytes must be UTF-8. Empty pieces
between `&` are skipped, and a piece without `=` has an empty value.
Parameters such as `charset` on the media type are accepted and ignored:
forms are UTF-8.

`read_form` refuses a declared `Content-Length` over `max_bytes` before
reading, and stops a chunked body as soon as it passes the limit.

## JSON

| Item | Meaning |
|---|---|
| `read_json(exchange, &limits)` | `Result[Json, BodyError]`: read and parse a JSON body |
| `send_json(exchange, status, &value, &limits)` | `Result[(), BodyError]`: encode and send as `application/json` |

`limits` is a [`std.json.Limits`](json.md); `max_input` bounds the body,
and depth, duplicate keys and output size follow it. The request must be
`application/json` or another `application/...+json` type.

## Multipart uploads

| Item | Meaning |
|---|---|
| `MultipartLimits(max_parts, max_head, max_field, max_file, max_body)` | Public fields; `multipart_limits()` is 64 parts, 4 KiB heads, 64 KiB fields, 8 MiB files, 16 MiB in all |
| `read_multipart(exchange, &limits, storage: &Root)` | `Result[list[Part], BodyError]`: read a `multipart/form-data` body |
| `Part` | `name`, `filename: Option[str]`, `content_type`, `size`, `content: Content` |
| `Content` | `Text(str)` for a field, `File(TempFile)` for an upload |

`max_head` bounds one part's header section, `max_field` a field held in
memory, `max_file` one upload, and `max_body` every byte read, including the
preamble, delimiters and epilogue. A declared `Content-Length` over
`max_body` is refused before reading.

A part with a `filename` parameter is an upload. Its content is streamed into
a new [unnamed temporary file](fs/temporary.md) in `storage` as it arrives,
never held whole in memory, and the file is rewound before it is returned:
read it through `content.stream`, or keep it with `publish(&name)`. Because
the file has no name until published, a request that fails, a connection
that is cancelled, and a part the handler drops all leave nothing on disk.
The descriptors are closed when the `TempFile`s drop. Each upload holds two
descriptors until then, so `max_parts` also bounds descriptor use.

`filename` is what the client sent. It is not a path and is not cleaned; do
not use it to name a file without checking it, and prefer a name the program
chooses.

Other parts are fields and must be UTF-8. A part's `Content-Type` defaults
to `text/plain` and is reported, not checked. Each part needs a
`Content-Disposition: form-data` field with a `name`. The boundary must be
1 to 70 characters from RFC 2046's set; per that RFC, the content of a part
cannot contain the delimiter line, so a part whose content holds
`CRLF--boundary` ends there. `Content-Transfer-Encoding` is ignored, as
RFC 7578 deprecates it.

## Limits

- Bodies are read whole for forms and JSON; use `exchange.read_body`
  directly for streamed processing.
- Multipart fields are not streamed; their size is bounded by `max_field`.
- No `multipart/mixed` nesting, and no character sets other than UTF-8.
