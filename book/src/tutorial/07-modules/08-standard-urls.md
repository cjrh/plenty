# Take a URL apart and build one

A URL is several components joined by delimiters, and each component has its
own rules about which characters may appear. `std.url` splits a URL into those
components and puts data into one of them safely.

```plenty
import std.url

def main() -> Result[(), Failure]:
    url = std.url.parse("https://example.com:8443/docs/a%20b?tag=x%26y&page=2#top", 2048)?
    print(url.scheme, url.path)?
    print(str.repr(&url.host)?, str.repr(&url.port)?)?
    print(str.repr(&url.userinfo)?, str.repr(&url.fragment)?)?
    match &url.query:
        case Some(query):
            for pair in &std.url.query_pairs(query)?:
                print(pair.key, "=", pair.value)?
        case Nothing:
            pass
    print(std.url.decode(&url.path)?)?
    print(url.format()?)?
    Ok(())
```
```output
https /docs/a%20b
Option[str].Some("example.com") Option[i64].Some(8443)
Option[str].Nothing Option[str].Some("top")
tag = x&y
page = 2
/docs/a b
https://example.com:8443/docs/a%20b?tag=x%26y&page=2#top
```

`parse` takes the text and the longest length in bytes you are willing to
accept. Components come back exactly as written, still percent-encoded:
nothing is lowercased, decoded, or tidied, so `format` returns the original
text. An optional component is `Nothing` when its delimiter is absent.

Decode a component only after the URL has been split. `%26` in the query is
data, and `query_pairs` splits at the real `&` before it decodes, which is why
`tag` holds `x&y`.

## Put data into a URL

To build a URL, encode each piece for the component it is going into, then fill
in a `Url`:

```plenty
import std.url
from std.url import Component, Pair, Url

def main() -> Result[(), Failure]:
    path = "/files/".concat(&std.url.encode("a/b report.txt", Component.Segment)?)?
    mut pairs = list[Pair]()?
    pairs.append(Pair("q", "fish & chips"))?
    pairs.append(Pair("lang", "en"))?
    query = std.url.query_text(&pairs)?
    url = Url("https", Nothing, Some("example.com"), Nothing, path, Some(query), Nothing)
    print(url.format()?)?
    print(std.url.encode("a/b", Component.Path)?, std.url.encode("a/b", Component.Segment)?)?
    Ok(())
```
```output
https://example.com/files/a%2Fb%20report.txt?q=fish%20%26%20chips&lang=en
a/b a%2Fb
```

The `Component` says where the data is going. As a `Segment`, the `/` in a file
name is data and is escaped; as a `Path`, it is a separator and is kept.
`query_text` encodes every key and value as a `QueryPart`, which escapes `&` and
`=`.

## What is rejected

`std.url` follows RFC 3986 strictly and never repairs input:

```plenty
import std.url
from std.url import Url

def main() -> Result[(), Failure]:
    print(str.repr(std.url.parse("/docs/page", 2048))?)?
    print(str.repr(std.url.parse("http://example.com/a b", 2048))?)?
    print(str.repr(std.url.parse("http://example.com/100%", 2048))?)?
    print(str.repr(std.url.parse("http://good.example@evil.example@host/", 2048))?)?
    print(str.repr(std.url.parse("http://example.com:99999/", 2048))?)?
    print(str.repr(std.url.parse("http://example.com/", 10))?)?
    forged = Url("https", Nothing, Some("example.com/x"), Nothing, "", Nothing, Nothing)
    print(str.repr(forged.format())?)?
    Ok(())
```
```output
Result[std.url.Url, std.url.UrlError].Err(std.url.UrlError.MissingScheme)
Result[std.url.Url, std.url.UrlError].Err(std.url.UrlError.InvalidByte(20))
Result[std.url.Url, std.url.UrlError].Err(std.url.UrlError.InvalidEscape(22))
Result[std.url.Url, std.url.UrlError].Err(std.url.UrlError.InvalidByte(32))
Result[std.url.Url, std.url.UrlError].Err(std.url.UrlError.InvalidPort(19))
Result[std.url.Url, std.url.UrlError].Err(std.url.UrlError.LimitExceeded)
Result[str, std.url.UrlError].Err(std.url.UrlError.InvalidByte(19))
```

The number is the byte offset of the problem. A URL must start with a scheme;
relative references are not parsed. A second `@` is an error rather than a
guess about which host was meant. `format` checks a `Url` you built yourself
with the same rules, so a host containing `/` cannot turn into a different URL.

Non-ASCII text must be percent-encoded, which `encode` does for you. A Unicode
host name must already be in its `xn--` form.

See the [URL reference](../../design/stdlib/url.md) for the exact syntax, every
operation, its errors, and its costs.
