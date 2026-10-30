# Encrypt and authenticate bytes

[`std.crypto`](../../design/stdlib/crypto-advanced.md) supplies authenticated
encryption and public-key operations over `Bytes`. Use a defined protocol
for exchanged messages: encryption alone does not manage identities,
replays, message framing, or keys.

## Encrypt one message

AEAD encryption protects both the message and associated data such as a
protocol header. The associated data stays readable, but changing it makes
verification fail. A recipient must have the same secret key, nonce, and
associated data.

This example creates a fresh key for one message. When reusing a key,
the application must ensure it **never repeats a nonce**, including across
restarts. The API does not track that for you.

```plenty
import std.bytes
import std.crypto
import std.secrets
from std.crypto import AeadAlgorithm

type BufferError = std.bytes.BytesError | std.crypto.AdvancedError | std.secrets.SecretsError

def main() -> Result[(), IoError | BufferError]:
    key = std.secrets.token_bytes(32)?
    nonce = std.secrets.token_bytes(12)?
    header = std.bytes.from_text("example-v1")?
    mut message = std.bytes.from_text("meet at noon")?
    std.crypto.seal_in_place(AeadAlgorithm.ChaCha20Poly1305, &key, &nonce, &header, &mut message)?
    # The buffer now holds ciphertext followed by a 16-byte tag.
    print(message.len())?
    std.crypto.open_in_place(AeadAlgorithm.ChaCha20Poly1305, &key, &nonce, &header, &mut message)?
    print(message.to_text()?)?
    Ok(())
```
```output
28
meet at noon
```

Opening returns an error and zeros the buffer if authentication fails; do
not use its contents after an error. The `?` above propagates that error.
Reserve 16 extra bytes before sealing when the buffer needs to be reused
without allocation. Opening never allocates.

## Sign with a private seed

An Ed25519 signature allows someone with the public key to check a message
without knowing the signing seed. The seed remains secret. A verifier also
needs an authenticated source for the public key: accepting any key supplied
alongside the signature proves no identity.

```plenty
import std.bytes
import std.crypto
import std.secrets

type SigningError = std.bytes.BytesError | std.crypto.AdvancedError | std.secrets.SecretsError

def main() -> Result[(), IoError | SigningError]:
    seed = std.secrets.token_bytes(32)?
    public_key = std.crypto.ed25519_public_key(&seed)?
    message = std.bytes.from_text("release manifest")?
    signature = std.crypto.ed25519_sign(&seed, &message)?
    print(std.crypto.ed25519_verify(&public_key, &message, &signature)?)?
    changed = std.bytes.from_text("modified manifest")?
    print(std.crypto.ed25519_verify(&public_key, &changed, &signature)?)?
    Ok(())
```
```output
True
False
```

Signing can reuse its seed. Keys and signatures have fixed lengths; malformed
lengths are errors, while a well-sized invalid signature returns `False`. A
public key that anyone could sign for, one of
[small order](../../design/stdlib/crypto-advanced.md#small-order-public-keys),
is the error `SmallOrderKey`.

## Derive a key after an exchange

Ephemeral X25519 keys can each perform one agreement. Exchange their public
keys first, authenticate the exchange through the application's protocol,
and derive an application key with HKDF. The example runs both peers locally;
it demonstrates agreement and key derivation, without providing a network
authentication protocol.

```plenty
import std.bytes
import std.crypto

def main() -> Result[(), IoError | std.bytes.BytesError | std.crypto.AdvancedError]:
    mut alice = std.crypto.x25519()?
    mut bob = std.crypto.x25519()?
    alice_public = alice.public_key()?
    bob_public = bob.public_key()?
    alice_shared = alice.agree(&bob_public)?
    bob_shared = bob.agree(&alice_public)?
    salt = std.bytes.empty()?
    context = std.bytes.from_text("example-v1 message key")?
    alice_key = std.crypto.hkdf_sha256(&salt, &alice_shared, &context, 32)?
    bob_key = std.crypto.hkdf_sha256(&salt, &bob_shared, &context, 32)?
    print(std.crypto.equal(&alice_key, &bob_key))?
    Ok(())
```
```output
True
```

An agreement attempt consumes the private key even if the peer is rejected;
length and allocation errors leave it usable. HKDF separates key material
by purpose using `context`; it is not a password hashing function. None of
these byte buffers is automatically scrubbed when dropped. See the
[key material and allocation contracts](../../design/stdlib/crypto-advanced.md#errors-allocation-and-key-material)
before storing secrets or designing a protocol.
