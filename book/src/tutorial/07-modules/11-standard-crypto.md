# Hash and sign bytes

`std.crypto` computes SHA-256 and SHA-512 digests and HMAC tags over
[`std.bytes`](../../design/stdlib/bytes.md) buffers, and compares secrets in
constant time.

## Digests

A digest is a fixed-size fingerprint of some bytes. The one-shot functions
take a whole buffer; a `Digest` takes input in pieces, which gives the same
result as hashing the pieces joined together:

```plenty
import std.bytes
import std.crypto
import std.encoding
from std.crypto import Algorithm

type CryptoBufferError = std.encoding.EncodingError | std.crypto.CryptoError | std.bytes.BytesError

def main() -> Result[(), AllocError | IoError | CryptoBufferError]:
    data = std.bytes.from_text("abc")?
    digest = std.crypto.sha256(&data)?
    print(digest.len())?
    print(std.encoding.hex_text(&digest, 0, digest.len())?)?
    mut hasher = std.crypto.digest(Algorithm.Sha256)?
    hasher.update_text("a")?
    hasher.update_text("bc")?
    print(std.crypto.equal(&hasher.finish()?, &digest))?
    print(str.repr(hasher.update_text("more"))?)?
    Ok(())
```
```output
32
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
True
Result[(), std.crypto.CryptoError].Err(std.crypto.CryptoError.Finished)
```

`finish` ends a digest. Feeding it afterwards is the error `Finished`, not a
new hash; make a new `Digest` instead. `update(data, start, stop)` takes a
range of a buffer, so a large input can be hashed while it is read, without
first collecting it in memory.

## Signing with HMAC

A digest proves nothing about who made it: anyone can hash a message. An HMAC
tag mixes in a secret key, so only a holder of the key can produce a tag that
verifies:

```plenty
import std.bytes
import std.crypto
import std.encoding
from std.bytes import Bytes
from std.crypto import Algorithm

def sign(key: &Bytes, message: &str) -> Result[Bytes, AllocError | IoError | std.crypto.CryptoError]:
    mut mac = std.crypto.hmac(Algorithm.Sha256, key)?
    mac.update_text(message)?
    Ok(mac.finish()?)

def check(key: &Bytes, message: &str, tag: &Bytes) -> Result[bool, AllocError | IoError | std.crypto.CryptoError]:
    mut mac = std.crypto.hmac(Algorithm.Sha256, key)?
    mac.update_text(message)?
    Ok(mac.verify(tag, 0, tag.len())?)

type CryptoBufferError = std.encoding.EncodingError | std.crypto.CryptoError | std.bytes.BytesError

def main() -> Result[(), AllocError | IoError | CryptoBufferError]:
    key = std.bytes.from_text("server key")?
    tag = sign(&key, "user=ada")?
    print(std.encoding.hex_text(&tag, 0, 8)?)?
    print(check(&key, "user=ada", &tag)?)?
    print(check(&key, "user=bob", &tag)?)?
    other = std.bytes.from_text("another key")?
    print(check(&other, "user=ada", &tag)?)?
    Ok(())
```
```output
8f60a639ef523af7
True
False
False
```

Check a tag with `verify`, never by finishing and comparing with `==`. `==`
stops at the first differing byte, so how long it takes reveals how much of a
forged tag was right. `verify` and `std.crypto.equal` take the same time
wherever the bytes differ.

A real key should be secret and random: make one with
`std.secrets.token_bytes(32)` from the [previous lessons](09-random-and-secrets.md)
rather than from text.

See the [crypto reference](../../design/stdlib/crypto.md) for the guarantees,
costs, and what the module does not provide.
