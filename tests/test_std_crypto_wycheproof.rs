//! Wycheproof signature vectors for `std.crypto` (issue #561).
//!
//! `tests/fixtures/crypto/wycheproof/*.jsonl` are compact extracts of
//! C2SP/wycheproof `testvectors_v1/` files at the commit and SHA-256 recorded
//! in each first line, written by `generate.py` there (Apache-2.0; licence in
//! the same directory). Each test group's key is imported through every form
//! the reference page offers for it (SubjectPublicKeyInfo, JWK members, and
//! the SEC1 point or raw key), and every case is verified with each import.
//! Some groups have no JWK; for those the JWK form prints `-` and is skipped.
//!
//! The Plenty program reads framed records from stdin, so one compiled
//! program checks a whole file: `K` and a key's fields, or `T`, a message and
//! a signature. For each `T` it prints one token per import form: `1`
//! verified, `0` rejected, `L` `InvalidLength`, `S` `SmallOrderKey` (from
//! the import or the verification), `E` any other error.
mod support;

use std::collections::BTreeMap;
use std::path::Path;

const HEADER: &str = r#"import std.bytes
import std.crypto
import std.encoding
from std.bytes import Bytes, BytesError
from std.crypto import AdvancedError, RsaScheme
from std.encoding import EncodingError

def size(data: &Bytes, at: i64) -> i64:
    i64(data[at]) * 16777216 + i64(data[at + 1]) * 65536 + i64(data[at + 2]) * 256 + i64(data[at + 3])

def field(data: &Bytes, at: i64) -> Result[Bytes, BytesError]:
    data.slice(at + 4, at + 4 + size(data, at))

def after(data: &Bytes, at: i64) -> i64:
    at + 4 + size(data, at)

def token(result: Result[bool, AdvancedError]) -> str:
    match result:
        case Ok(True):
            "1"
        case Ok(False):
            "0"
        case Err(AdvancedError.InvalidLength):
            "L"
        case Err(AdvancedError.SmallOrderKey):
            "S"
        case Err(_):
            "E"

def stdin() -> Result[Bytes, IoError | BytesError]:
    mut data = std.bytes.empty()?
    mut used = 0
    while True:
        data.resize(used + 65536, 0u8)?
        count = data.read_stdin(used, used + 65536)?
        if count == 0:
            break
        used = used + count
    data.truncate(used)?
    Ok(data)

def jwk(text: &Bytes) -> Result[Bytes, EncodingError]:
    std.encoding.base64url_decode(text, 0, text.len())
"#;

const RSA: &str = r#"
def scheme(code: u8) -> RsaScheme:
    match code:
        case 0u8:
            RsaScheme.Pkcs1Sha256
        case 1u8:
            RsaScheme.Pkcs1Sha384
        case 2u8:
            RsaScheme.Pkcs1Sha512
        case 3u8:
            RsaScheme.PssSha256
        case 4u8:
            RsaScheme.PssSha384
        case _:
            RsaScheme.PssSha512

# One group: its key record, then test records up to the next key.
def group(data: &Bytes, start: i64, code: u8) -> Result[i64, IoError | BytesError | AdvancedError | EncodingError]:
    mut at = start + 1
    spki = field(data, at)?
    at = after(data, at)
    n = jwk(&field(data, at)?)?
    at = after(data, at)
    e = jwk(&field(data, at)?)?
    at = after(data, at)
    from_spki = std.crypto.rsa_public_key_from_spki(&spki)?
    from_jwk = std.crypto.rsa_public_key_from_components(&n, &e)?
    while at < data.len() and data[at] == 84u8:
        message = field(data, at + 1)?
        at = after(data, at + 1)
        signature = field(data, at)?
        at = after(data, at)
        a = token(from_spki.verify(scheme(code), &message, &signature))
        b = token(from_jwk.verify(scheme(code), &message, &signature))
        print(a, b)?
    Ok(at)

def main() -> Result[(), IoError | BytesError | AdvancedError | EncodingError]:
    data = stdin()?
    mut at = 1
    while at < data.len():
        at = group(&data, at, data[0])?
    Ok(())
"#;

const P256: &str = r#"
def group(data: &Bytes, start: i64) -> Result[i64, IoError | BytesError | AdvancedError | EncodingError]:
    mut at = start + 1
    spki = field(data, at)?
    at = after(data, at)
    point = field(data, at)?
    at = after(data, at)
    x_text = field(data, at)?
    at = after(data, at)
    y_text = field(data, at)?
    at = after(data, at)
    from_spki = std.crypto.p256_public_key_from_spki(&spki)?
    from_point = std.crypto.p256_public_key_from_point(&point)?
    # Groups without a JWK send empty members; those cases print `-` for it.
    has_jwk = x_text.len() > 0
    mut x = point.slice(1, 33)?
    mut y = point.slice(33, 65)?
    if has_jwk:
        x = jwk(&x_text)?
        y = jwk(&y_text)?
    from_jwk = std.crypto.p256_public_key_from_coordinates(&x, &y)?
    while at < data.len() and data[at] == 84u8:
        message = field(data, at + 1)?
        at = after(data, at + 1)
        signature = field(data, at)?
        at = after(data, at)
        a = token(from_spki.verify(&message, &signature))
        b = token(from_point.verify(&message, &signature))
        mut c = "-"
        if has_jwk:
            c = token(from_jwk.verify(&message, &signature))
        print(a, b, c)?
    Ok(at)

def main() -> Result[(), IoError | BytesError | AdvancedError | EncodingError]:
    data = stdin()?
    mut at = 1
    while at < data.len():
        at = group(&data, at)?
    Ok(())
"#;

const ED25519: &str = r#"
# A small-order key fails SPKI import; each of its cases then prints `S` for
# that form, so the forms can still be compared.
def verify(key: &Result[Bytes, AdvancedError], message: &Bytes, signature: &Bytes) -> str:
    match key:
        case Ok(public_key):
            token(std.crypto.ed25519_verify(public_key, message, signature))
        case Err(AdvancedError.SmallOrderKey):
            "S"
        case Err(_):
            "E"

def group(data: &Bytes, start: i64) -> Result[i64, IoError | BytesError | AdvancedError | EncodingError]:
    mut at = start + 1
    spki = field(data, at)?
    at = after(data, at)
    raw = field(data, at)?
    at = after(data, at)
    x_text = field(data, at)?
    at = after(data, at)
    from_spki = std.crypto.ed25519_public_key_from_spki(&spki)
    # Groups without a JWK send an empty member; those cases print `-` for it.
    has_jwk = x_text.len() > 0
    mut from_jwk = raw.copy()?
    if has_jwk:
        from_jwk = jwk(&x_text)?
    while at < data.len() and data[at] == 84u8:
        message = field(data, at + 1)?
        at = after(data, at + 1)
        signature = field(data, at)?
        at = after(data, at)
        a = verify(&from_spki, &message, &signature)
        b = token(std.crypto.ed25519_verify(&raw, &message, &signature))
        mut c = "-"
        if has_jwk:
            c = token(std.crypto.ed25519_verify(&from_jwk, &message, &signature))
        print(a, b, c)?
    Ok(at)

def main() -> Result[(), IoError | BytesError | AdvancedError | EncodingError]:
    data = stdin()?
    mut at = 1
    while at < data.len():
        at = group(&data, at)?
    Ok(())
"#;

#[derive(Clone, Copy, PartialEq)]
enum Family {
    Rsa(u8),
    P256,
    Ed25519,
}

struct Case {
    id: u64,
    result: String,
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn frame(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    out.extend_from_slice(data);
}

/// The framed stdin for `name` and its cases in order.
fn vectors(name: &str, family: Family) -> (Vec<u8>, Vec<Case>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/crypto/wycheproof")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines = text.lines();
    let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert!(
        header["source"]
            .as_str()
            .unwrap()
            .contains(name.trim_end_matches(".jsonl"))
    );
    let mut input = vec![match family {
        Family::Rsa(code) => code,
        _ => 0,
    }];
    let mut cases = Vec::new();
    for line in lines {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        if let Some(key) = value.get("key") {
            let member = |name: &str| key.get(name).and_then(|v| v.as_str()).unwrap_or("");
            input.push(b'K');
            frame(&mut input, &unhex(member("spki")));
            match family {
                Family::Rsa(_) => {
                    frame(&mut input, member("n").as_bytes());
                    frame(&mut input, member("e").as_bytes());
                }
                Family::P256 => {
                    frame(&mut input, &unhex(member("point")));
                    frame(&mut input, member("x").as_bytes());
                    frame(&mut input, member("y").as_bytes());
                }
                Family::Ed25519 => {
                    frame(&mut input, &unhex(member("pk")));
                    frame(&mut input, member("x").as_bytes());
                }
            }
        } else {
            let case = value.as_array().unwrap();
            input.push(b'T');
            frame(&mut input, &unhex(case[2].as_str().unwrap()));
            frame(&mut input, &unhex(case[3].as_str().unwrap()));
            cases.push(Case {
                id: case[0].as_u64().unwrap(),
                result: case[1].as_str().unwrap().to_string(),
            });
        }
    }
    (input, cases)
}

/// Each case's token, after checking that every import form agreed.
fn run(name: &str, family: Family) -> Vec<(Case, String)> {
    let source = format!(
        "{HEADER}{}",
        match family {
            Family::Rsa(_) => RSA,
            Family::P256 => P256,
            Family::Ed25519 => ED25519,
        }
    );
    let (input, cases) = vectors(name, family);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, &source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = support::output_within(
        &mut std::process::Command::new(executable),
        &input,
        std::time::Duration::from_secs(120),
    );
    assert!(
        output.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), cases.len(), "{name}: one line per case");
    cases
        .into_iter()
        .zip(lines)
        .map(|(case, line)| {
            let tokens: Vec<&str> = line.split(' ').filter(|token| *token != "-").collect();
            assert!(
                tokens.iter().all(|token| *token == tokens[0]),
                "{name} tcId {}: import forms disagree: {line}",
                case.id
            );
            (case, tokens[0].to_string())
        })
        .collect()
}

/// `valid` must verify, `invalid` must be rejected (`0`) or have the wrong
/// length (`L`), and `acceptable` must give the token `acceptable` assigns.
/// Prints the counts so the PR and reference page can cite them.
fn check(name: &str, family: Family, acceptable: &dyn Fn(u64) -> &'static str) {
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for (case, token) in run(name, family) {
        let pass = match case.result.as_str() {
            "valid" => token == "1",
            "invalid" => token == "0" || token == "L",
            _ => token == acceptable(case.id),
        };
        if !pass {
            failures.push(format!("tcId {} ({}): {token}", case.id, case.result));
        }
        *counts.entry((case.result, token)).or_default() += 1;
    }
    eprintln!("{name}: {counts:?}");
    assert!(failures.is_empty(), "{name}: {failures:#?}");
}

fn no_acceptable(id: u64) -> &'static str {
    panic!("unexpected acceptable case {id}")
}

/// ring requires the NULL parameters in the PKCS #1 v1.5 DigestInfo, so the
/// one `acceptable` case in each PKCS #1 file (`MissingNull`) is rejected.
fn missing_null_rejected(_: u64) -> &'static str {
    "0"
}

#[test]
fn rsa_pkcs1_vectors() {
    for (name, code) in [
        ("rsa_signature_2048_sha256.jsonl", 0),
        ("rsa_signature_2048_sha384.jsonl", 1),
        ("rsa_signature_2048_sha512.jsonl", 2),
        ("rsa_signature_3072_sha256.jsonl", 0),
        ("rsa_signature_4096_sha512.jsonl", 2),
    ] {
        check(name, Family::Rsa(code), &missing_null_rejected);
    }
}

#[test]
fn rsa_pss_vectors() {
    for (name, code) in [
        ("rsa_pss_2048_sha256_mgf1_32.jsonl", 3),
        ("rsa_pss_2048_sha384_mgf1_48.jsonl", 4),
        ("rsa_pss_4096_sha512_mgf1_64.jsonl", 5),
    ] {
        check(name, Family::Rsa(code), &no_acceptable);
    }
}

#[test]
fn ecdsa_p256_fixed_signature_vectors() {
    check(
        "ecdsa_secp256r1_sha256_p1363.jsonl",
        Family::P256,
        &no_acceptable,
    );
}

/// `verify` takes only the fixed 64-byte form, so no DER signature verifies,
/// including the cases Wycheproof marks valid for DER verifiers.
#[test]
fn ecdsa_p256_der_signatures_never_verify() {
    let results = run("ecdsa_secp256r1_sha256.jsonl", Family::P256);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (case, token) in &results {
        assert!(
            token == "0" || token == "L",
            "tcId {} ({}): {token}",
            case.id,
            case.result
        );
        *counts.entry(token.clone()).or_default() += 1;
    }
    eprintln!("ecdsa_secp256r1_sha256.jsonl: {counts:?}");
}

/// The `acceptable` Ed25519 cases (152-180) use small-order (152-159) or
/// mixed-order (160-180) public keys. Small-order keys are rejected with
/// `SmallOrderKey` (issue #569). Mixed-order keys are accepted and their
/// signatures verify; the reference page documents this choice, and a
/// change to it fails here.
fn small_order_keys_rejected(id: u64) -> &'static str {
    match id {
        152..=159 => "S",
        160..=180 => "1",
        _ => panic!("unexpected acceptable case {id}"),
    }
}

#[test]
fn ed25519_vectors() {
    check("ed25519.jsonl", Family::Ed25519, &small_order_keys_rejected);
}
