# std.jwt

`import std.jwt` signs and verifies JSON Web Tokens (RFC 7519) in the compact
JWS form (RFC 7515), with the RFC 7518 and RFC 8037 algorithms that
[`std.crypto`](crypto.md) provides. Every error is a `JwtError`.

> **Upstream code, used as-is.** Every MAC and signature is computed and
> checked by `std.crypto`, which uses unmodified `ring` and RustCrypto
> primitives; `std.jwt` is checked Plenty code that only parses, validates and
> dispatches. See [`std.crypto`](crypto.md) for this policy.

```plenty
import std.json
import std.jwt
import std.secrets
import std.time
from std.json import Json, JsonError
from std.jwt import Algorithm, JwtError
from std.secrets import SecretsError

def main() -> Result[(), AllocError | IoError | SecretsError | JsonError | JwtError]:
    secret = std.secrets.token_bytes(32)?
    signer = std.jwt.hmac_signing_key(Algorithm.Hs256, &secret)?
    claims = std.json.parse_text("{\"sub\":\"ada\",\"aud\":\"api\",\"exp\":4102444800}", &std.json.limits())?
    token = std.jwt.sign(&signer, &claims)?

    # The verifying key fixes the algorithm; the header only has to agree.
    key = signer.verifying_key()?
    mut rules = std.jwt.validation(std.time.wall_now().seconds)
    rules.leeway = 30
    rules.audience = Some("api")
    verified = std.jwt.verify(&token, &key, &rules)?
    print(verified.header.algorithm.name())?
    for member in &verified.claims:
        match &member.value:
            case Json.Text(text):
                print(member.key, text)?
            case _:
                pass

    # Any change to the signed bytes fails the signature check.
    forged = token.replace("eyJzdWIiOiJhZGEi", "eyJzdWIiOiJib2Ii")?
    match std.jwt.verify(&forged, &key, &rules):
        case Err(JwtError.InvalidSignature):
            print("forged token rejected")?
        case _:
            print("unexpected")?
    Ok(())
```
```output
HS256
sub ada
aud api
forged token rejected
```

See the [signature index](signatures.md#stdjwt) for complete declarations.

## Keys and algorithms

A key is bound to one algorithm when it is made. `sign` uses that algorithm,
and `verify` accepts a token only if its header's `alg` names the same one.
The header never selects the algorithm or the key, so `none`, an HMAC token
checked against an RSA public key, and other algorithm-substitution attacks
fail before any cryptography runs.

| `Algorithm` | Name | Signing key | Verifying key |
|---|---|---|---|
| `Hs256`, `Hs512` | HS256, HS512 | `hmac_signing_key(algorithm, &secret)` | `hmac_verifying_key(algorithm, &secret)` |
| `Rs256`, `Rs384`, `Rs512` | RS256 to RS512 | `rsa_signing_key(algorithm, RsaPrivateKey)` | `rsa_verifying_key(algorithm, RsaPublicKey)` |
| `Ps256`, `Ps384`, `Ps512` | PS256 to PS512 | `rsa_signing_key(algorithm, RsaPrivateKey)` | `rsa_verifying_key(algorithm, RsaPublicKey)` |
| `Es256` | ES256 | `es256_signing_key(P256PrivateKey)` | `es256_verifying_key(P256PublicKey)` |
| `EdDsa` | EdDSA (Ed25519) | `ed25519_signing_key(&seed)` | `ed25519_verifying_key(&public_key)` |

Each constructor returns `Result[_, JwtError]`, except the infallible ES256
ones. `InvalidKey` reports:

- an algorithm the constructor does not handle, such as `rsa_signing_key(Algorithm.Hs256, ...)`;
- an HMAC secret shorter than the hash output: 32 bytes for HS256, 64 for
  HS512 (RFC 7518 section 3.2). Generate one with
  [`std.secrets.token_bytes`](secrets.md);
- an Ed25519 seed or public key that is not 32 bytes, or a small-order public
  key, which could verify forged signatures.

HMAC secrets and Ed25519 keys are copied. RSA and P-256 keys come from
[`std.crypto` key import](crypto-keys.md), which enforces their sizes and
encodings, and are moved in. `SigningKey.verifying_key()` returns the matching
verifying key: a copy of the secret, or the public half. `algorithm()` returns
a key's algorithm and `Algorithm.name()` its JOSE name. Both key types are
[redacted](../language/03-types/classes.md#redacted-types): printing one shows `std.jwt.SigningKey(???)` or
`std.jwt.VerifyingKey(???)`, never the secret.

HS384 and ES384 are not provided: `std.crypto` has no SHA-384 HMAC or P-384
curve. Their names are `UnsupportedAlgorithm`.

## Verifying

`verify(&token, &key, &rules)` returns a `Token` with the decoded `header` and
the claims as a `list[Member]` from [`std.json`](json.md#value-tree). It
checks, in order:

1. `rules` are in range, otherwise `InvalidValidation`.
2. The token is at most `rules.max_size` bytes, otherwise `TooLarge`. Nothing
   is decoded before this check.
3. The token is three segments separated by dots, each strict unpadded
   base64url (RFC 4648 section 5) with zero trailing bits. Padding, the
   standard alphabet, whitespace, and any other byte are `Malformed`.
4. The header is a JSON object (`NotObject`) of strict JSON without duplicate
   members (`Malformed`). `alg`, `kid` and `typ` must be strings when present.
   A missing `alg` is `Malformed`; `none` or an unknown name is
   `UnsupportedAlgorithm`. A `crit` member is `CriticalHeader`, since no
   extensions are understood (RFC 7515 section 4.1.11). Other members,
   including `jwk`, `jku`, `x5u` and `x5c`, are ignored: they never supply a key.
5. `alg` equals the key's algorithm, otherwise `AlgorithmMismatch`.
6. The signature verifies over the first two segments, otherwise
   `InvalidSignature`. HMAC tags are compared in constant time by
   `std.crypto`'s `Mac.verify`.
7. Only now are the claims parsed, with the same strictness as the header:
   they must be a JSON object without duplicate members.
8. The claims satisfy the time, issuer and audience rules below.

`decode_header(&token, max_size)` performs steps 2 to 4 without a key, for
choosing a key by `kid`. Its result is unauthenticated: use it only to pick
among keys the program already trusts, then call `verify`.

### Validation rules

`std.jwt.validation(now)` returns a `Validation` with these public fields:

| Field | Default | Meaning |
|---|---|---|
| `now` | the argument | Current time in Unix seconds, at least 0, such as `std.time.wall_now().seconds` |
| `leeway` | `0` | Allowed clock skew in seconds, `0` to `max_leeway` (86400) |
| `require_expiry` | `True` | Whether a token without `exp` is `MissingClaim(Claim.Expiry)` |
| `issuer` | `Nothing` | When set, `iss` must be present and equal it |
| `audience` | `Nothing` | The audience this program accepts |
| `max_size` | `default_max_size` (8192) | Token limit in bytes, `1` to `max_token_size` (1 MiB) |

The clock is explicit so tests and programs choose its source. The claims are
checked as RFC 7519 section 4.1 describes:

- `exp`: `Expired` once `now - leeway >= exp`. A fractional value is rounded
  down.
- `nbf` and `iat`: `NotYetValid` when more than `leeway` seconds in the
  future. Fractional values are rounded up. An `iat` in the future is
  rejected because the token cannot have been issued yet.
- A time claim that is not a number, or is beyond the `i64` range, is
  `InvalidClaim` with its `Claim`.
- `iss`, when `issuer` is set: missing is `MissingClaim(Claim.Issuer)`, a
  non-string is `InvalidClaim(Claim.Issuer)`, and a different string is
  `WrongIssuer`. The comparison is exact.
- `aud` is a string or an array of strings; anything else is
  `InvalidClaim(Claim.Audience)`. It must contain `audience`, otherwise
  `WrongAudience`. A token with `aud` is `WrongAudience` when `audience` is
  `Nothing`, since the program is not among its recipients (RFC 7519 section
  4.1.3). When `audience` is set, a missing `aud` is
  `MissingClaim(Claim.Audience)`.

Other claims, including `sub` and `jti`, are returned unchecked. `typ` is
returned in `Header.token_type` and not checked.

## Signing

`sign(&key, &claims, key_id)` returns the compact token as `str`. `claims`
must be a `Json.Object`, otherwise `NotObject`; it is encoded compactly in
member order, and a value JSON cannot represent, such as a NaN, is
`Malformed`. The header is `{"alg":"...","typ":"JWT"}`, with `kid` added when
`key_id` is `Some`. HS, RS and EdDSA signatures are deterministic; PS and ES
signatures use operating-system randomness, and its failure is
`RandomFailure`. `sign` adds no claims: set `exp` and `iat` yourself.

## Errors

`JwtError` has a [`std.report`](report.md) `format` method; no message
repeats token contents or key material.

| Variant | Meaning |
|---|---|
| `Malformed` | Not three strict base64url segments of strict JSON, a header without `alg`, or claims that cannot be encoded |
| `NotObject` | The header or claims are not a JSON object |
| `TooLarge` | The token exceeds `max_size` |
| `UnsupportedAlgorithm` | `none` or an algorithm this module does not provide |
| `AlgorithmMismatch` | The header's `alg` is not the key's algorithm |
| `CriticalHeader` | The header has `crit` |
| `InvalidSignature` | The signature or MAC does not verify |
| `Expired`, `NotYetValid` | The time claims reject `now` |
| `MissingClaim(Claim)`, `InvalidClaim(Claim)` | A required claim is absent, or a claim has the wrong type or range |
| `WrongIssuer`, `WrongAudience` | `iss` or `aud` does not match |
| `InvalidKey` | A key does not suit its algorithm |
| `InvalidValidation` | A `Validation` field is out of range |
| `RandomFailure` | Randomness failed while signing |
| `Allocation(AllocError)` | An allocation failed |

`Claim` is `Expiry`, `NotBefore`, `IssuedAt`, `Issuer` or `Audience`.

## Memory

`verify` allocates a copy of the token, the decoded header, claims and
signature, and the claims tree; `sign` allocates the encoded segments and the
result. All of these are bounded by `max_size` when verifying and by the
claims when signing. Every allocation is fallible and reported as
`Allocation`; tests fail each allocation in turn and check that no other
error results and nothing leaks.

Memory-constrained programs should use ES256 or EdDSA, which allocate nothing
inside `std.crypto` beyond the signature they return. HMAC allocates one
368-byte state per operation. RSA reserves
up to 12 KiB per private key while it lives, plus 8 KiB per signature and
12 KiB per verification, and reports a failed reservation as `Allocation`;
see [RSA and ECDSA keys](crypto-keys.md).

## Limits

- Only compact JWS. JWE (encrypted tokens), the JSON serialization, unencoded
  payloads (RFC 7797) and nested tokens are not supported.
- No JWK or JWKS handling: import keys through
  [`std.crypto`](crypto-keys.md), which accepts JWK components.
- No HS384, ES384, ES512 or `ES256K`.
- Header nesting is limited to 4 levels and claims to 32.

## Acknowledgements

- Token format, claims and algorithms follow
  [RFC 7519](https://www.rfc-editor.org/rfc/rfc7519) (JWT),
  [RFC 7515](https://www.rfc-editor.org/rfc/rfc7515) (JWS),
  [RFC 7518](https://www.rfc-editor.org/rfc/rfc7518) (JWA) and
  [RFC 8037](https://www.rfc-editor.org/rfc/rfc8037) (EdDSA in JOSE). Tests
  use the RFC 7515 appendix A.1 to A.3 and RFC 8037 appendix A.4 examples.
- The key-bound API and validation settings were modelled on the Rust
  [`jsonwebtoken`](https://docs.rs/jsonwebtoken/) crate; no code is used.
- Cryptography is `std.crypto`'s; see its
  [acknowledgements](crypto.md#acknowledgements).
