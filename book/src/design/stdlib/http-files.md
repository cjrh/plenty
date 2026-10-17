# `std.http.files`

`import std.http.files` answers [`std.http.server`](http-server.md) requests
with files below one directory. The module is checked Plenty in
`stdlib/std/http/files.plenty`; it uses [`std.fs`](fs.md) `Root` handles and
`file_metadata`, and adds no runtime primitives of its own.

| Item | Meaning |
|---|---|
| `serve(exchange, &root, path)` | `Result[i64, FilesError]`: answer the request with the file at `path` below `root`; the status sent |
| `open_file(&root, path)` | `Result[Option[FileStream], FilesError]`: the regular file at `path`, or `Nothing` when it is refused or missing |
| `media_type(name)` | `str`: the `Content-Type` for `name`'s extension |
| `http_date(seconds)` | `Result[Option[str], FilesError]`: an IMF-fixdate such as `Sun, 06 Nov 1994 08:49:37 GMT`; `Nothing` before 1970 or after 9999 |
| `parse_http_date(text)` | `Result[Option[i64], FilesError]`: seconds since the epoch, or `Nothing` unless `text` is an exact IMF-fixdate |
| `max_path` | `4096`: longest accepted `path`, in bytes |

```text
routes.add("GET", "/static/{path...}", 0)?
...
path = exchange.params[0].concat("")?
std.http.files.serve(exchange, &self.public, &path)?
```

## Confinement

`path` is relative and already percent-decoded, as a `{name...}` capture
is. `open_file` splits it on `/` and opens each segment through the previous
directory's `Root`, so every component is opened with `O_NOFOLLOW` relative
to its parent. A path is refused when:

- it is empty or longer than `max_path`,
- a segment is empty, which covers an absolute path and `//`,
- a segment starts with `.`, which covers `.`, `..` and hidden files such as
  `.env` or `.git`,
- it contains a NUL byte,
- any component is a symbolic link, the last is not a regular file (a
  directory, FIFO or device), or it does not exist or cannot be opened.

Cleaning the path as a string would not be enough: a link inside the
directory can lead out of it, and a path checked before opening can be
replaced before it is opened. Here the open itself refuses links, and the
file is opened non-blocking so a FIFO cannot stall the worker. Every
refusal is the same `404 Not Found`, so a client learns nothing about what
lies outside the root or which entries are hidden.

## Responses

| Request | Response |
|---|---|
| Not `GET` or `HEAD` | `405` with `Allow: GET, HEAD` |
| Refused or missing path | `404`, plain text |
| `If-None-Match` lists the file's tag, or is `*` | `304` with `ETag` and `Last-Modified`, no body |
| No `If-None-Match`, and `If-Modified-Since` is not before the file's time | `304` |
| Otherwise | `200` with `Content-Type`, `Content-Length`, `X-Content-Type-Options: nosniff`, `ETag`, `Last-Modified`; `HEAD` sends no body |

The tag is weak, `W/"size-seconds.nanoseconds"` from the open file's size
and modification time, compared weakly as RFC 9110 requires for
`If-None-Match`. `If-None-Match` takes precedence over `If-Modified-Since`,
which is ignored unless it is an exact IMF-fixdate (the obsolete RFC 850 and
asctime forms are refused). Size and time come from `std.fs.file_metadata`
on the open file, so they describe what is sent even if the path is replaced
meanwhile.

The body is read in 16 KiB pieces into one buffer and written as it is
read, so memory does not depend on the file's size. Exactly the size seen at
open is sent. A file that shrinks while it is sent is `Truncated` after the
head has gone out; the server then closes the connection.

`media_type` compares the extension without case:

| Extension | Type |
|---|---|
| `html`, `htm` | `text/html; charset=utf-8` |
| `css` | `text/css; charset=utf-8` |
| `js`, `mjs` | `text/javascript; charset=utf-8` |
| `json` | `application/json` |
| `txt` | `text/plain; charset=utf-8` |
| `csv` | `text/csv; charset=utf-8` |
| `xml` | `application/xml` |
| `svg` | `image/svg+xml` |
| `png`, `jpg`, `jpeg`, `gif`, `webp`, `ico` | `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/vnd.microsoft.icon` |
| `wasm` | `application/wasm` |
| `pdf` | `application/pdf` |
| `woff2`, `woff` | `font/woff2`, `font/woff` |
| Anything else | `application/octet-stream` |

## Errors

`FilesError` is `Io(IoError)` from reading the open file's metadata,
`Stream(StreamError)` from reading it, `Http(HttpError)` from writing the
response, `Truncated`, and `Allocation`. A refused or missing path is not an
error.

## Limits

- No `Range` requests (`206`), no `If-Range`, and no strong tags.
- No directory index: a path naming a directory is `404`, so `index.html`
  must be named in the route or the handler.
- No compression, precompressed variants, or `Cache-Control`; a handler
  that needs them builds the response itself from `open_file`, or a
  reverse proxy adds them.
- A file is read with ordinary blocking reads on the worker; a slow
  filesystem holds the worker.
- Linux only, like `std.fs`.
