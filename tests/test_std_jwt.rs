//! std.jwt (issue #488). Known answers come from RFC 7515 appendices A.1
//! (HS256), A.2 (RS256) and A.3 (ES256) and RFC 8037 appendix A (EdDSA), with
//! the keys shared from `test_std_crypto_keys`. The `INTEROP_*` and
//! `EXPECTED_*` tokens were produced once with Python `cryptography` 50.0.0
//! and the standard `hmac` module over the claims `{"sub":"interop","exp":4102444800}`.
mod support;

use crate::test_std_crypto_keys::{EC_PKCS8, EC_X, EC_Y, ED_PUBLIC, ED_SEED, RSA_PKCS8, RSA_SPKI};

const CLAIMS: &str = "eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ";
const HS256_RFC: &str =
    "eyJ0eXAiOiJKV1QiLA0KICJhbGciOiJIUzI1NiJ9.$CLAIMS.dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const HS256_RFC_KEY: &str =
    "AyM1SysPpbyDfgZld3umj1qzKObwVMkoqQ-EstJQLr_T-1qS0gZH75aKtMN3Yj0iPS4hcgUuTwjAzZr1Z9CAow";
const RS256_RFC: &str = "eyJhbGciOiJSUzI1NiJ9.$CLAIMS.cC4hiUPoj9Eetdgtv3hF80EGrhuB__dzERat0XF9g2VtQgr9PJbu3XOiZj5RZmh7AAuHIm4Bh-0Qc_lF5YKt_O8W2Fp5jujGbds9uJdbF9CUAr7t1dnZcAcQjbKBYNX4BAynRFdiuB--f_nZLgrnbyTyWzO75vRK5h6xBArLIARNPvkSjtQBMHlb1L07Qe7K0GarZRmB_eSN9383LcOLn6_dO--xi12jzDwusC-eOkHWEsqtFZESc6BfI7noOPqvhJ1phCnvWh6IeYI2w9QOYEUipUTI8np6LbgGY9Fs98rqVt5AXLIhWkWywlVmtVrBp0igcN_IoypGlUPQGe77Rw";
const ES256_RFC: &str = "eyJhbGciOiJFUzI1NiJ9.$CLAIMS.DtEhU3ljbEg8L38VWAfUAqOyKAM6-Xx-F4GawxaepmXFCgfTjDxw5djxLa8ISlSApmWQxfKTUJqPP3-Kg6NU1Q";
const EDDSA_RFC: &str = "eyJhbGciOiJFZERTQSJ9.RXhhbXBsZSBvZiBFZDI1NTE5IHNpZ25pbmc.hgyY0il_MGCjP0JzlnLWG1PPOt7-09PGcvMg3AIbQR6dWbhijcNR4ki4iylGjg5BhVsPt9g7sVvpAr_MuM0KAg";

const INTEROP: &str = "eyJzdWIiOiJpbnRlcm9wIiwiZXhwIjo0MTAyNDQ0ODAwfQ";
const INTEROP_PS384: &str = "eyJhbGciOiJQUzM4NCIsInR5cCI6IkpXVCJ9.$INTEROP.Fi-DFZLbFh-w8dGw5xecGGdzUwT9Zy8DIXHcSH4pcmBTC434AkMYLQQaNRNuEZ-k-N9hoUObxJpYbTdJG-1-FTRVafGatMWPBHz0dXGzmQGz0w8x7mG-IUZMD45KB8CKi4IF0SP7rD8OwXPyYbIl7K0bVT5QDp9G4lLnz1NBGfxlq28I9uiZudueRs6ItupQEHMUaCW5RjzzhDiH1I5rcrAuCvO0r8X8XOGZMoPzINFOkfynxGwNmYjXrs-Rp4sXANuFSQYC6l5USqruHvquq7bQv56C3W_SScJDf4WcNd6sU4wPNhPp1z6Jveyzw8FN3f7YVSMtAYBGR4hBG7dp9g";
const INTEROP_ES256: &str = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.$INTEROP.5Rf3jp_EIXXGmIf0wifWL79EXmIDdSVeAbwm0dzYd73JCqlSRU3OeKmDVF7Z8lWP3wi-rtouKWWpwPTrrTwcfg";
/// Signed with the 64 bytes 00..3f.
const INTEROP_HS512: &str = "eyJhbGciOiJIUzUxMiIsInR5cCI6IkpXVCJ9.$INTEROP.lk5E0oQE9e0Vp9XwBS_h7fnv4C_8niu0fp1HStRfCMylCYMrtGmJFmM_R-vEvGG7Nx9jhxWxzDM3FTmmkoCd7g";
const INTEROP_RS512: &str = "eyJhbGciOiJSUzUxMiIsInR5cCI6IkpXVCJ9.$INTEROP.NA8Vudsp9P20Fip_suplW5w2VkNGBrSymTWE31dVC-3x1iVsX93EO2BqESc5S8L45kHSCivEio5SLWBpTITtcDKcZvMZvZSJo-YkFnlGwRSRsmsxVMpWjqyyfo0rqcjY4eNmQxBRPnV9MbX0JhNnSAOm7-JQ91WWd_DuRAwA15VdE1a390YpNcptZh9PdIllDRGCQrRVsINCUI6pWeYZE9GQNT4eTd4jU_jn1rqZ4w_3gTvHZUzgSOME5R61QOuyEpJp9en_ahBfJk3R42Y55iQlEeePUodk0u1mbmaK6rR9t57cYY1zo1wSIB2kIAdZL9G4WcHX7m0MSafU_jk2MQ";
/// Deterministic signatures with `kid` "k1": what `sign` must produce.
const EXPECTED_RS256: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6ImsxIn0.$INTEROP.Vj5EAX64BkQuwt6kXfFulQhlJ2vIV0dZfV6CSU_IZqojBtCCuZIXsDarTQUUSGXBm2m73ETb0oDnWjAaMZ1SmHwilk1J_3AG8IhuyCdXhurNRqfXNHNTbVTb7r3rPnTQgRIkspPnR-SwR6oteJnsrVn_my7MhozCRDJ_ehII6rNdGSal0388eWwusWgLCfiZFpelLf2Xsum16ZJbns6xJibR5oTsSQ0Ezhp_P9pRHPH9uOcsBGG2DLFKpFBgJvIApaHoh0B4xKxrw3aPr_8jXkw6od0PXt60N2nME86lGNTLztJMDYA_kFFQoXsRhc56Czemt0BH7u8c7UvJ948qbQ";
const EXPECTED_EDDSA: &str = "eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCIsImtpZCI6ImsxIn0.$INTEROP.7KcP0DPAN_4XCRqafgTiX2lGuMPgLHo9xlOsMBYME2yscjuGzQb4top3FOPqNCYEkVL130v7Iuj8TbxY8LhfBQ";

const HEADER: &str = r#"import std.bytes
import std.crypto
import std.encoding
import std.json
import std.jwt
import std.fmt
import std.report
from std.bytes import Bytes
from std.json import Json
from std.jwt import Algorithm, JwtError, SigningKey, Token, Validation, VerifyingKey

type Fault = AllocError | IoError | std.bytes.BytesError | std.crypto.AdvancedError | std.crypto.CryptoError | std.encoding.EncodingError | std.json.JsonError | std.fmt.FormatError | std.report.FormatError | JwtError

def unhex(text: &str) -> Result[Bytes, Fault]:
    Ok(std.encoding.hex_from_text(text)?)

def claims(text: &str) -> Result[Json, Fault]:
    Ok(std.json.parse_text(text, &std.json.limits())?)

def show(result: Result[Token, JwtError]) -> Result[(), Fault]:
    match result:
        case Ok(token):
            print("ok", token.header.algorithm.name(), len(token.claims))?
        case Err(error):
            print(str.repr(error)?)?
    Ok(())

def check(token: &str, key: &VerifyingKey, rules: &Validation) -> Result[(), Fault]:
    show(std.jwt.verify(token, key, rules))

def secret() -> Result[Bytes, Fault]:
    Ok(std.bytes.from_text("0123456789abcdef0123456789abcdef")?)

def hs256() -> Result[SigningKey, Fault]:
    Ok(std.jwt.hmac_signing_key(Algorithm.Hs256, &secret()?)?)

def signed(key: &SigningKey, text: &str) -> Result[str, Fault]:
    Ok(std.jwt.sign(key, &claims(text)?)?)

def segment(text: &str) -> Result[Bytes, Fault]:
    data = std.bytes.from_text(text)?
    Ok(std.encoding.base64url_encode(&data, 0, data.len())?)

# A token with an arbitrary header and payload and a valid HS256 tag under
# `secret()`, to reach checks that follow the signature.
def forge(header: &str, payload: &str) -> Result[str, Fault]:
    mut token = segment(header)?
    token.push(46u8)?
    token.extend(&segment(payload)?)?
    tag = std.crypto.hmac_sha256(&secret()?, &token)?
    token.push(46u8)?
    token.extend(&std.encoding.base64url_encode(&tag, 0, tag.len())?)?
    Ok(token.to_text()?)

def rsa_private() -> Result[std.crypto.RsaPrivateKey, Fault]:
    Ok(std.crypto.rsa_private_key_from_pkcs8(&unhex("$RSA_PKCS8")?)?)

def rsa_public(algorithm: Algorithm) -> Result[VerifyingKey, Fault]:
    Ok(std.jwt.rsa_verifying_key(algorithm, std.crypto.rsa_public_key_from_spki(&unhex("$RSA_SPKI")?)?)?)

def es256_public() -> Result[VerifyingKey, Fault]:
    Ok(std.jwt.es256_verifying_key(std.crypto.p256_public_key_from_coordinates(&unhex("$EC_X")?, &unhex("$EC_Y")?)?))

def ed_public() -> Result[VerifyingKey, Fault]:
    Ok(std.jwt.ed25519_verifying_key(&unhex("$ED_PUBLIC")?)?)

"#;

/// `$NAME` stands for the fixture of that name. Tokens contain `$CLAIMS`
/// or `$INTEROP`, so they are substituted before the segments.
const FIXTURES: &[(&str, &str)] = &[
    ("HS256_RFC_KEY", HS256_RFC_KEY),
    ("HS256_RFC", HS256_RFC),
    ("RS256_RFC", RS256_RFC),
    ("ES256_RFC", ES256_RFC),
    ("EDDSA_RFC", EDDSA_RFC),
    ("INTEROP_PS384", INTEROP_PS384),
    ("INTEROP_ES256", INTEROP_ES256),
    ("INTEROP_HS512", INTEROP_HS512),
    ("INTEROP_RS512", INTEROP_RS512),
    ("EXPECTED_RS256", EXPECTED_RS256),
    ("EXPECTED_EDDSA", EXPECTED_EDDSA),
    ("CLAIMS", CLAIMS),
    ("INTEROP", INTEROP),
    ("RSA_PKCS8", RSA_PKCS8),
    ("RSA_SPKI", RSA_SPKI),
    ("EC_PKCS8", EC_PKCS8),
    ("EC_X", EC_X),
    ("EC_Y", EC_Y),
    ("ED_SEED", ED_SEED),
    ("ED_PUBLIC", ED_PUBLIC),
];

fn substitute(mut text: String) -> String {
    for (name, value) in FIXTURES {
        text = text.replace(&format!("${name}"), value);
    }
    assert!(!text.contains('$'), "unknown fixture in {text}");
    text
}

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    substitute(format!(
        "{HEADER}def main() -> Result[(), Fault]:\n{body}    Ok(())\n"
    ))
}

fn native(body: &str, expected: &str) {
    let source = program(body);
    let actual = support::run_file_stdout(&source)
        .replace("std.jwt.JwtError.", "")
        .replace("std.jwt.Claim.", "");
    assert_eq!(actual.trim_end(), expected.trim_matches('\n'), "{source}");
}

#[test]
fn rfc7515_a1_hs256_verifies_until_expiry() {
    native(
        r#"
key = std.jwt.hmac_verifying_key(Algorithm.Hs256, &std.encoding.base64url_from_text("$HS256_RFC_KEY")?)?
mut rules = std.jwt.validation(1300819379)
check("$HS256_RFC", &key, &rules)?
token = std.jwt.verify("$HS256_RFC", &key, &rules)?
print(token.header.token_type)?
match std.json.find(&token.claims, "iss"):
    case Some(index):
        match &token.claims[index].value:
            case Json.Text(issuer):
                print(issuer)?
            case _:
                print("not text")?
    case Nothing:
        print("missing")?
rules.now = 1300819380
check("$HS256_RFC", &key, &rules)?
rules.leeway = 1
check("$HS256_RFC", &key, &rules)?
"#,
        r#"
ok HS256 3
Option[str].Some("JWT")
joe
Expired
ok HS256 3
"#,
    );
}

#[test]
fn rfc7515_a2_rs256_and_deterministic_signing() {
    native(
        r#"
rules = std.jwt.validation(1300819379)
check("$RS256_RFC", &rsa_public(Algorithm.Rs256)?, &rules)?
check("$RS256_RFC", &rsa_public(Algorithm.Ps256)?, &rules)?
check("$RS256_RFC", &rsa_public(Algorithm.Rs512)?, &rules)?
signer = std.jwt.rsa_signing_key(Algorithm.Rs256, rsa_private()?)?
token = std.jwt.sign(&signer, &claims("{\"sub\":\"interop\",\"exp\":4102444800}")?, Some("k1"))?
print(token == "$EXPECTED_RS256")?
check(&token, &signer.verifying_key()?, &rules)?
header = std.jwt.decode_header(&token)?
print(header.algorithm.name(), header.key_id, header.token_type)?
"#,
        r#"
ok RS256 3
AlgorithmMismatch
AlgorithmMismatch
True
ok RS256 2
RS256 Option[str].Some("k1") Option[str].Some("JWT")
"#,
    );
}

#[test]
fn rfc7515_a3_es256_and_random_signing() {
    native(
        r#"
rules = std.jwt.validation(1300819379)
key = es256_public()?
check("$ES256_RFC", &key, &rules)?
check("$INTEROP_ES256", &key, &rules)?
signer = std.jwt.es256_signing_key(std.crypto.p256_private_key_from_pkcs8(&unhex("$EC_PKCS8")?)?)
first = signed(&signer, "{\"n\":1}")?
second = signed(&signer, "{\"n\":1}")?
print(first == second)?
rules2 = Validation(0, 0, False, Nothing, Nothing, 8192)
check(&first, &key, &rules2)?
check(&second, &signer.verifying_key()?, &rules2)?
"#,
        r#"
ok ES256 3
ok ES256 2
False
ok ES256 1
ok ES256 1
"#,
    );
}

/// The RFC 8037 payload is text, not JSON: the signature verifies, and only
/// then are the claims rejected. A bad signature fails first.
#[test]
fn rfc8037_eddsa_checks_the_signature_before_the_claims() {
    native(
        r#"
rules = std.jwt.validation(0)
key = ed_public()?
check("$EDDSA_RFC", &key, &rules)?
check("eyJhbGciOiJFZERTQSJ9.RXhhbXBsZSBvZiBFZDI1NTE5IHNpZ25pbmc.hgyY0il_MGCjP0JzlnLWG1PPOt7-09PGcvMg3AIbQR6dWbhijcNR4ki4iylGjg5BhVsPt9g7sVvpAr_MuM0KAw", &key, &rules)?
signer = std.jwt.ed25519_signing_key(&unhex("$ED_SEED")?)?
token = std.jwt.sign(&signer, &claims("{\"sub\":\"interop\",\"exp\":4102444800}")?, Some("k1"))?
print(token == "$EXPECTED_EDDSA")?
check(&token, &key, &rules)?
"#,
        r#"
Malformed
InvalidSignature
True
ok EdDSA 2
"#,
    );
}

#[test]
fn interoperates_with_python_cryptography() {
    native(
        r#"
rules = std.jwt.validation(0)
check("$INTEROP_PS384", &rsa_public(Algorithm.Ps384)?, &rules)?
check("$INTEROP_RS512", &rsa_public(Algorithm.Rs512)?, &rules)?
mut wide = std.bytes.empty()?
for index in range(64):
    wide.push(u8(index))?
check("$INTEROP_HS512", &std.jwt.hmac_verifying_key(Algorithm.Hs512, &wide)?, &rules)?
for algorithm in [Algorithm.Ps256, Algorithm.Ps512, Algorithm.Rs384]?:
    signer = std.jwt.rsa_signing_key(algorithm, rsa_private()?)?
    check(&signed(&signer, "{\"exp\":1}")?, &signer.verifying_key()?, &rules)?
"#,
        r#"
ok PS384 2
ok RS512 2
ok HS512 2
ok PS256 1
ok PS512 1
ok RS384 1
"#,
    );
}

/// The verifying key fixes the algorithm. `none`, unknown names, a header
/// naming a different algorithm, and critical extensions are rejected before
/// any signature check.
#[test]
fn header_never_chooses_the_algorithm() {
    native(
        r#"
rules = std.jwt.validation(0)
hmac = hs256()?.verifying_key()?
check(&forge("{\"alg\":\"none\"}", "{\"exp\":9}")?, &hmac, &rules)?
none = segment("{\"alg\":\"none\"}")?.to_text()?
payload = segment("{\"exp\":9}")?.to_text()?
check(&f"{none}.{payload}."?, &hmac, &rules)?
check(&forge("{\"alg\":\"None\"}", "{\"exp\":9}")?, &hmac, &rules)?
check(&forge("{\"alg\":\"HS384\"}", "{\"exp\":9}")?, &hmac, &rules)?
check(&forge("{\"typ\":\"JWT\"}", "{\"exp\":9}")?, &hmac, &rules)?
check(&forge("{\"alg\":256}", "{\"exp\":9}")?, &hmac, &rules)?
check(&forge("{\"alg\":\"HS256\",\"crit\":[\"exp\"]}", "{\"exp\":9}")?, &hmac, &rules)?
check(&forge("{\"alg\":\"HS256\",\"kid\":7}", "{\"exp\":9}")?, &hmac, &rules)?
check(&forge("{\"alg\":\"HS256\",\"jwk\":{\"kty\":\"oct\",\"k\":\"AA\"}}", "{\"exp\":9}")?, &hmac, &rules)?
# The classic confusion attack presents an HS256 token to an RSA key, hoping
# the public key is used as the MAC secret. The key refuses HS256 outright.
check(&forge("{\"alg\":\"HS256\"}", "{\"exp\":9}")?, &rsa_public(Algorithm.Rs256)?, &rules)?
check("$EDDSA_RFC", &es256_public()?, &rules)?
"#,
        r#"
UnsupportedAlgorithm
UnsupportedAlgorithm
UnsupportedAlgorithm
UnsupportedAlgorithm
Malformed
Malformed
CriticalHeader
Malformed
ok HS256 1
AlgorithmMismatch
AlgorithmMismatch
"#,
    );
}

#[test]
fn tampering_is_an_invalid_signature() {
    native(
        r#"
rules = std.jwt.validation(0)
signer = hs256()?
key = signer.verifying_key()?
token = signed(&signer, "{\"admin\":false,\"exp\":9}")?
check(&token, &key, &rules)?
parts = token.split(".")?
admin = segment("{\"admin\":true,\"exp\":9}")?.to_text()?
check(&f"{parts[0]}.{admin}.{parts[2]}"?, &key, &rules)?
# One character fewer leaves non-canonical trailing bits: strict base64url.
check(&token.slice(0, len(token) - 1)?, &key, &rules)?
check(&f"{parts[0]}.{parts[1]}."?, &key, &rules)?
other = std.jwt.hmac_verifying_key(Algorithm.Hs256, &std.bytes.from_text("another secret of thirty-two b!!")?)?
check(&token, &other, &rules)?
check(&"$RS256_RFC".replace("cC4hiUPoj9", "cC4hiUPoj8")?, &rsa_public(Algorithm.Rs256)?, &std.jwt.validation(1300819379))?
check(&"$ES256_RFC".replace("DtEhU3ljbE", "DtEhU3ljbF")?, &es256_public()?, &std.jwt.validation(1300819379))?
"#,
        r#"
ok HS256 2
InvalidSignature
Malformed
InvalidSignature
InvalidSignature
InvalidSignature
InvalidSignature
"#,
    );
}

#[test]
fn malformed_tokens_are_rejected() {
    native(
        r#"
rules = std.jwt.validation(0)
key = hs256()?.verifying_key()?
good = signed(&hs256()?, "{\"exp\":9}")?
parts = good.split(".")?
for token in ["", ".", "..", "abc", "a.b", "a.b.c.d", "...."]?:
    check(token, &key, &rules)?
# Padding, the standard alphabet, whitespace and non-ASCII are not base64url.
check(&f"{parts[0]}=.{parts[1]}.{parts[2]}"?, &key, &rules)?
check(&f"{parts[0]}.{parts[1]}.{parts[2]}="?, &key, &rules)?
standard = parts[0].replace("J", "+")?
check(&f"{standard}.{parts[1]}.{parts[2]}"?, &key, &rules)?
check(&f" {good}"?, &key, &rules)?
check(&f"{good}é"?, &key, &rules)?
# Header JSON: not JSON, not an object, duplicate members.
check(&forge("{\"alg\":\"HS256\"", "{\"exp\":9}")?, &key, &rules)?
check(&forge("[\"HS256\"]", "{\"exp\":9}")?, &key, &rules)?
check(&forge("{\"alg\":\"HS256\",\"alg\":\"HS256\"}", "{\"exp\":9}")?, &key, &rules)?
# Claims JSON, read only after the signature verifies.
check(&forge("{\"alg\":\"HS256\"}", "[1]")?, &key, &rules)?
check(&forge("{\"alg\":\"HS256\"}", "")?, &key, &rules)?
check(&forge("{\"alg\":\"HS256\"}", "{\"exp\":9,\"exp\":10}")?, &key, &rules)?
check(&forge("{\"alg\":\"HS256\"}", "{\"exp\":9} x")?, &key, &rules)?
"#,
        r#"
Malformed
Malformed
Malformed
Malformed
Malformed
Malformed
Malformed
Malformed
Malformed
Malformed
Malformed
Malformed
Malformed
NotObject
Malformed
NotObject
Malformed
Malformed
Malformed
"#,
    );
}

#[test]
fn size_limit_and_validation_settings() {
    native(
        r#"
signer = hs256()?
key = signer.verifying_key()?
token = signed(&signer, "{\"exp\":9}")?
mut rules = std.jwt.validation(0)
rules.max_size = token.byte_len()
check(&token, &key, &rules)?
rules.max_size = token.byte_len() - 1
check(&token, &key, &rules)?
print(str.repr(std.jwt.decode_header(&token, token.byte_len() - 1))?)?
huge = "a".repeat(std.jwt.default_max_size + 1)?
check(&huge, &key, &std.jwt.validation(0))?
for setting in [0, std.jwt.max_token_size + 1]?:
    rules.max_size = setting
    check(&token, &key, &rules)?
rules.max_size = std.jwt.default_max_size
rules.leeway = -1
check(&token, &key, &rules)?
rules.leeway = std.jwt.max_leeway + 1
check(&token, &key, &rules)?
rules.leeway = 0
rules.now = -1
check(&token, &key, &rules)?
"#,
        r#"
ok HS256 1
TooLarge
Result[std.jwt.Header, std.jwt.JwtError].Err(TooLarge)
TooLarge
InvalidValidation
InvalidValidation
InvalidValidation
InvalidValidation
InvalidValidation
"#,
    );
}

/// `now` sits at 1000. Expiry is exclusive and rounds down; `nbf` and `iat`
/// round up. Leeway widens each check.
#[test]
fn time_claims_use_the_explicit_clock() {
    native(
        r#"
signer = hs256()?
key = signer.verifying_key()?
mut rules = std.jwt.validation(1000)
for text in ["{\"exp\":1001}", "{\"exp\":1000}", "{\"exp\":1000.9}", "{\"exp\":1001.5}", "{}", "{\"exp\":\"1001\"}", "{\"exp\":1e300}", "{\"exp\":9223372036854775807}", "{\"exp\":-9223372036854775808}", "{\"exp\":2000,\"nbf\":1000}", "{\"exp\":2000,\"nbf\":1001}", "{\"exp\":2000,\"nbf\":1000.1}", "{\"exp\":2000,\"iat\":1001}", "{\"exp\":2000,\"iat\":999}", "{\"exp\":2000,\"nbf\":9223372036854775807}", "{\"exp\":2000,\"nbf\":null}", "{\"exp\":2000,\"iat\":true}"]?:
    check(&signed(&signer, text)?, &key, &rules)?
print("leeway")?
rules.leeway = 5
for text in ["{\"exp\":996}", "{\"exp\":995}", "{\"exp\":2000,\"nbf\":1005}", "{\"exp\":2000,\"nbf\":1006}", "{\"exp\":2000,\"iat\":1005}", "{\"exp\":2000,\"iat\":1006}"]?:
    check(&signed(&signer, text)?, &key, &rules)?
print("optional")?
rules.require_expiry = False
check(&signed(&signer, "{}")?, &key, &rules)?
"#,
        r#"
ok HS256 1
Expired
Expired
ok HS256 1
MissingClaim(Expiry)
InvalidClaim(Expiry)
InvalidClaim(Expiry)
ok HS256 1
Expired
ok HS256 2
NotYetValid
NotYetValid
NotYetValid
ok HS256 2
NotYetValid
InvalidClaim(NotBefore)
InvalidClaim(IssuedAt)
leeway
ok HS256 1
Expired
ok HS256 2
NotYetValid
ok HS256 2
NotYetValid
optional
ok HS256 0
"#,
    );
}

#[test]
fn issuer_and_audience() {
    native(
        r#"
signer = hs256()?
key = signer.verifying_key()?
mut rules = std.jwt.validation(0)
rules.require_expiry = False
check(&signed(&signer, "{\"iss\":\"a\"}")?, &key, &rules)?
rules.issuer = Some("a")
for text in ["{\"iss\":\"a\"}", "{\"iss\":\"b\"}", "{\"iss\":\"A\"}", "{}", "{\"iss\":1}"]?:
    check(&signed(&signer, text)?, &key, &rules)?
print("audience")?
rules.issuer = Nothing
# RFC 7519: a recipient that names no audience rejects any `aud`.
check(&signed(&signer, "{\"aud\":\"api\"}")?, &key, &rules)?
check(&signed(&signer, "{}")?, &key, &rules)?
rules.audience = Some("api")
for text in ["{\"aud\":\"api\"}", "{\"aud\":[\"web\",\"api\"]}", "{\"aud\":\"web\"}", "{\"aud\":[]}", "{\"aud\":[\"api\",1]}", "{\"aud\":{\"api\":true}}", "{}"]?:
    check(&signed(&signer, text)?, &key, &rules)?
"#,
        r#"
ok HS256 1
ok HS256 1
WrongIssuer
WrongIssuer
MissingClaim(Issuer)
InvalidClaim(Issuer)
audience
WrongAudience
ok HS256 0
ok HS256 1
ok HS256 1
WrongAudience
WrongAudience
InvalidClaim(Audience)
InvalidClaim(Audience)
MissingClaim(Audience)
"#,
    );
}

#[test]
fn keys_are_bound_to_their_algorithm() {
    native(
        r#"
short = std.bytes.from_text("0123456789abcdef0123456789abcde")?
print(str.repr(std.jwt.hmac_signing_key(Algorithm.Hs256, &short))?)?
print(str.repr(std.jwt.hmac_verifying_key(Algorithm.Hs512, &secret()?))?)?
print(str.repr(std.jwt.hmac_signing_key(Algorithm.Rs256, &secret()?))?)?
print(str.repr(std.jwt.rsa_signing_key(Algorithm.Hs256, rsa_private()?))?)?
print(str.repr(std.jwt.rsa_verifying_key(Algorithm.Es256, std.crypto.rsa_public_key_from_spki(&unhex("$RSA_SPKI")?)?))?)?
mut zero = std.bytes.empty()?
zero.resize(32, 0u8)?
print(str.repr(std.jwt.ed25519_verifying_key(&zero))?)?
print(str.repr(std.jwt.ed25519_signing_key(&short))?)?
print(hs256()?.algorithm().name(), std.jwt.es256_verifying_key(std.crypto.p256_public_key_from_coordinates(&unhex("$EC_X")?, &unhex("$EC_Y")?)?).algorithm().name())?
print(str.repr(std.jwt.sign(&hs256()?, &claims("[1]")?))?)?
print(str.repr(std.jwt.sign(&hs256()?, &Json.Object([std.json.Member("x", Json.Float(f64.NAN))]?)))?)?
"#,
        r#"
Result[std.jwt.SigningKey, std.jwt.JwtError].Err(InvalidKey)
Result[std.jwt.VerifyingKey, std.jwt.JwtError].Err(InvalidKey)
Result[std.jwt.SigningKey, std.jwt.JwtError].Err(InvalidKey)
Result[std.jwt.SigningKey, std.jwt.JwtError].Err(InvalidKey)
Result[std.jwt.VerifyingKey, std.jwt.JwtError].Err(InvalidKey)
Result[std.jwt.VerifyingKey, std.jwt.JwtError].Err(InvalidKey)
Result[std.jwt.SigningKey, std.jwt.JwtError].Err(InvalidKey)
HS256 ES256
Result[str, std.jwt.JwtError].Err(NotObject)
Result[str, std.jwt.JwtError].Err(Malformed)
"#,
    );
}

#[test]
fn keys_format_redacted() {
    // `SigningKey` and `VerifyingKey` are `@redacted` (issue #586): no
    // automatic formatting reveals an HMAC secret or private key, including
    // inside a Result or list, or through an f-string conversion.
    native(
        r#"
key = hs256()?
print(key)?
print(str.repr(std.jwt.hmac_verifying_key(Algorithm.Hs256, &secret()?))?)?
print([es256_public()?]?)?
print(f"{key!r} {key:>14}"?)?
"#,
        r#"
std.jwt.SigningKey(@redacted)
Result[std.jwt.VerifyingKey, std.jwt.JwtError].Ok(std.jwt.VerifyingKey(@redacted))
[std.jwt.VerifyingKey(@redacted)]
std.jwt.SigningKey(@redacted) std.jwt.SigningKey(@redacted)
"#,
    );
}

#[test]
fn errors_format_through_std_report() {
    native(
        r#"
for error in [JwtError.UnsupportedAlgorithm, JwtError.AlgorithmMismatch, JwtError.MissingClaim(std.jwt.Claim.Audience), JwtError.InvalidClaim(std.jwt.Claim.NotBefore), JwtError.Allocation(AllocError.OutOfMemory)]?:
    print(std.report.to_string(&error, 256, 4)?)?
"#,
        r#"
header names no algorithm, `none`, or an unsupported algorithm
header algorithm differs from the verifying key's algorithm
required claim aud is missing
claim nbf is not a NumericDate
out of memory
"#,
    );
    let output = support::run_file(&substitute(format!(
        "{HEADER}def main() -> Result[(), JwtError]:\n    key = std.jwt.hmac_verifying_key(Algorithm.Hs256, &std.bytes.empty().unwrap())?\n    Ok(())\n"
    )));
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: main returned key is malformed, too short, or for another algorithm\n"
    );
}

/// Every allocation `sign` and `verify` make is fallible: failing the Nth
/// returns Allocation, never another error, and the runtime's exit check
/// sees nothing leaked. Past the last allocation each operation succeeds.
#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_are_errors() {
    const LIMIT: usize = 160;
    let markers = (0..LIMIT)
        .map(|n| format!("\"__test_fail_allocations_after_{n}__\""))
        .collect::<Vec<_>>()
        .join(", ");
    let source = substitute(format!(
        "{HEADER}{}",
        r#"
def report(label: &str, result: Result[(), JwtError]) -> Result[(), Fault]:
    match result:
        case Ok(_):
            print(label, "ok")?
        case Err(JwtError.Allocation(_)):
            print(label, "allocation")?
        case Err(other):
            return Err(other)
    Ok(())

def verified(marker: &str, label: &str, token: &str, key: &VerifyingKey, rules: &Validation) -> Result[(), Fault]:
    print(marker)?
    result = std.jwt.verify(token, key, rules)
    print("__test_restore_allocations__")?
    match result:
        case Ok(_):
            report(label, Ok(()))
        case Err(error):
            report(label, Err(error))

def signs(marker: &str, label: &str, key: &SigningKey, claims: &Json, expected: &str) -> Result[(), Fault]:
    print(marker)?
    result = std.jwt.sign(key, claims, Some("k1"))
    print("__test_restore_allocations__")?
    match result:
        case Ok(token):
            if token != *expected:
                print(label, "differs")?
            report(label, Ok(()))
        case Err(error):
            report(label, Err(error))

def main() -> Result[(), Fault]:
    rules = std.jwt.validation(1300819379)
    hmac = std.jwt.hmac_verifying_key(Algorithm.Hs256, &std.encoding.base64url_from_text("$HS256_RFC_KEY")?)?
    es256 = es256_public()?
    eddsa = ed_public()?
    rs256 = rsa_public(Algorithm.Rs256)?
    rsa_signer = std.jwt.rsa_signing_key(Algorithm.Rs256, rsa_private()?)?
    ed_signer = std.jwt.ed25519_signing_key(&unhex("$ED_SEED")?)?
    payload = claims("{\"sub\":\"interop\",\"exp\":4102444800}")?
    markers = [MARKERS]?
    for i in range(len(markers)):
        verified(markers[i], "hs256", "$HS256_RFC", &hmac, &rules)?
        verified(markers[i], "es256", "$ES256_RFC", &es256, &rules)?
        verified(markers[i], "eddsa", "$EXPECTED_EDDSA", &eddsa, &rules)?
        verified(markers[i], "rs256", "$RS256_RFC", &rs256, &rules)?
        signs(markers[i], "sign-eddsa", &ed_signer, &payload, "$EXPECTED_EDDSA")?
        signs(markers[i], "sign-rs256", &rsa_signer, &payload, "$EXPECTED_RS256")?
    Ok(())
"#
        .replace("MARKERS", &markers)
    ));
    let out = support::run_file_stdout(&source);
    assert!(!out.contains("differs"), "{out}");
    for operation in [
        "hs256",
        "es256",
        "eddsa",
        "rs256",
        "sign-eddsa",
        "sign-rs256",
    ] {
        let results: Vec<&str> = out
            .lines()
            .filter_map(|line| {
                line.strip_prefix(operation)
                    .filter(|rest| rest.starts_with(' '))
            })
            .map(str::trim)
            .collect();
        assert_eq!(results.len(), LIMIT, "{out}");
        let failures = results.iter().take_while(|r| **r == "allocation").count();
        assert!(failures >= 3, "{operation}: {results:?}");
        assert!(
            failures < LIMIT && results[failures..].iter().all(|r| *r == "ok"),
            "{operation}: {results:?}"
        );
    }
}
