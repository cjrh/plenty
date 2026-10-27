# Hash and sign bytes

`std.crypto` computes BLAKE3, SHA-256 and SHA-512 digests and keyed tags over
[`std.bytes`](../../design/stdlib/bytes.md) buffers, and compares secrets in
constant time. For AEAD encryption and public-key signatures, see
[Encrypt and authenticate bytes](11a-standard-encryption.md).

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

## BLAKE3

For BLAKE3, choose `Algorithm.Blake3` or the one-shot `blake3` function:

```plenty
import std.bytes
import std.crypto
from std.crypto import Algorithm

def main() -> Result[(), IoError | std.bytes.BytesError | std.crypto.CryptoError]:
    data = std.bytes.from_text("hello world")?
    expected = std.crypto.blake3(&data)?
    mut hasher = std.crypto.digest(Algorithm.Blake3)?
    hasher.update_text("hello ")?
    hasher.update_text("world")?
    print(expected.len(), std.crypto.equal(&hasher.finish()?, &expected))?
    Ok(())
```
```output
32 True
```

BLAKE3 also has a native keyed hash with exactly 32 key bytes and a key
derivation mode. Use `blake3_keyed_hasher(key)` for a `Mac` with the same
`update` and `verify` methods used for HMAC below. It is a distinct algorithm:
`hmac(Algorithm.Blake3, key)` reports `UnsupportedAlgorithm`. See the
[BLAKE3 reference](../../design/stdlib/crypto-blake3.md) for key derivation
contexts, allocation costs and limits.

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
`std.secrets.token_bytes(32)` from [Randomness and secrets](09-random-and-secrets.md)
rather than from text.

See the [crypto reference](../../design/stdlib/crypto.md) for the guarantees,
costs, and what the module does not provide.
