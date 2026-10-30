# `std.mime`

`import std.mime` parses, builds, formats and compares media types such as
`text/html; charset=utf-8`, and maps file extensions to a `Content-Type`.
The module is checked Plenty in `stdlib/std/mime.plenty` and links no native
code. [`std.http.files`](http-files.md) uses its extension table. The
[signature index](signatures.md#stdmime) lists the complete API.

```plenty
import std.mime

def main() -> Result[(), AllocError | IoError | ParseError]:
    value = std.mime.parse("Text/HTML; Charset=\"UTF-8\"; title=\"a \\\"b\\\"\"")?
    print(value.format()?)?
    print(value.type_name(), value.subtype(), value.param("charset"), value.param("TITLE"))?
    print(value.matches(&std.mime.parse("text/*")?), value.matches(&std.mime.parse("image/*")?))?
    mut upload = std.mime.new("multipart", "form-data")?
    upload.set_param("boundary", "x7 y")?
    print(upload.format()?)?
    print(std.mime.from_file_name("logo.SVG"), std.mime.from_extension("woff2"))?
    Ok(())
```
```output
text/html; charset=utf-8; title="a \"b\""
text html Option[str].Some("utf-8") Option[str].Some("a \"b\"")
True False
multipart/form-data; boundary="x7 y"
image/svg+xml Option[str].Some("font/woff2")
```

## Media types

| Item | Meaning |
|---|---|
| `parse(text)` | `Result[MediaType, AllocError \| ParseError]`: the media type in `text` |
| `new(type_name, subtype)` | `Result[MediaType, AllocError \| ParseError]`: a media type without parameters; both names must be tokens |
| `value.type_name()`, `value.subtype()` | `&str`: the lowercase names |
| `value.params()` | `&list[Param]`: parameters in order; each `Param` has `name` and `value` |
| `value.param(name)` | `Option[str]`: the value of the parameter `name`, compared without case |
| `value.essence()` | `Result[str, AllocError]`: `type/subtype` without parameters |
| `value.suffix()` | `Result[Option[str], AllocError]`: the structured syntax suffix after the last `+`, such as `json` in `application/ld+json` |
| `value.is_wildcard()` | `bool`: the type or subtype is `*` |
| `value.matches(&pattern)` | `bool`: `value` falls within the media range `pattern` |
| `value.set_param(name, value)` | `Result[(), AllocError \| ParseError]`: add a parameter, or replace the value of one with the same name |
| `value.remove_param(name)` | `bool`: remove the parameter `name`; `False` when absent |
| `value.format()` | `Result[str, AllocError]`: canonical text |

`parse` follows the `media-type` rule of
[RFC 9110 section 8.3.1](https://www.rfc-editor.org/rfc/rfc9110.html#section-8.3.1):

```text
media-type = type "/" subtype *( OWS ";" OWS [ parameter ] )
parameter  = token "=" ( token / quoted-string )
```

Spaces and tabs are allowed around the whole text and around each `;`, and
empty parameters such as `a/b;;x=1` are skipped. A quoted-string may hold
tab, space, visible ASCII and non-ASCII text, and `\` escapes the next
character; the stored value is unquoted and unescaped. Everything else
returns `ParseError.Invalid`: a missing or empty type or subtype, a
character outside a token, a parameter without `=` or value, an unclosed
quote, text after a closing quote, and a parameter name given twice
(compared without case, as
[RFC 6838 section 4.3](https://www.rfc-editor.org/rfc/rfc6838.html#section-4.3)
requires).

Type, subtype and parameter names are case-insensitive, so they are stored
in lowercase. Parameter values keep their case, except `charset`, whose
values are case-insensitive and are stored in lowercase. `==` therefore
treats `Text/HTML;Charset=UTF-8` and `text/html; charset=utf-8` as equal. It
still compares parameters in order, so `a/b; x=1; y=2` and
`a/b; y=2; x=1` differ.

`format` writes `type/subtype`, then `; name=value` for each parameter in
order. A value that is empty or not a token is written as a quoted-string,
with `"` and `\` escaped. Parsing formatted text gives an equal value.

`set_param` requires a token name and a value without control characters
other than tab, or returns `ParseError.Invalid` and leaves the value
unchanged. It lowercases the name, and the value of `charset`.

## Media ranges

`value.matches(&pattern)` reads `pattern` as a media range, as in an
`Accept` header: `*/*` matches every type, `text/*` every `text` subtype,
and any other type and subtype must be equal. Each parameter of `pattern`
must appear in `value` with the same value; extra parameters of `value` are
ignored. So `text/html; charset=utf-8` matches `text/*` and
`text/html; charset=utf-8`, but not `text/html; level=1`.

`std.mime` does not parse whole `Accept` headers: splitting the list and
reading `q` weights belongs to the HTTP layer.

## File extensions

| Item | Meaning |
|---|---|
| `from_extension(extension)` | `Option[str]`: the `Content-Type` for an extension given without its dot, or `Nothing` |
| `from_file_name(name)` | `str`: the `Content-Type` for the extension after the last `.` of `name`, or `application/octet-stream` |

Both compare without ASCII case and allocate nothing. A name with no `.`,
such as `html`, has no extension; `.html` has the extension `html`. The
table is fixed:

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

The result is `Content-Type` text, ready for a response header; pass it to
`parse` for a `MediaType`.

## Allocation

`parse` copies the text once into a byte buffer and allocates the names,
values and parameter list of the result. `new`, `set_param`, `essence`,
`suffix` and `format` allocate their results. Every allocation failure
returns `AllocError` and leaves existing values unchanged. Accessors,
`param`, `matches`, `is_wildcard`, `remove_param` and the extension lookups
allocate nothing.

## Limits

- No length limit: the caller bounds input, as HTTP header limits do.
- No content sniffing, and no lookup from a media type back to extensions.
- The extension table is the fixed list above, not the full IANA registry.

## Acknowledgements

- API shape modelled on Rust's [`mime`](https://docs.rs/mime/latest/mime/) crate.
- Extension types are registered in the [IANA Media Types registry](https://www.iana.org/assignments/media-types/media-types.xhtml).
