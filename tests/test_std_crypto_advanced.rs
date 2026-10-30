//! Advanced crypto vectors and failure/ownership contracts (issue #464).
mod support;

const HEADER: &str = r#"import std.bytes
import std.crypto
import std.encoding
from std.bytes import Bytes
from std.crypto import AeadAlgorithm, AdvancedError

def hex(data: &Bytes) -> Result[str, AllocError | IoError | std.encoding.EncodingError]:
    Ok(std.encoding.hex_text(data, 0, data.len())?)

def unhex(text: &str) -> Result[Bytes, AllocError | IoError | std.encoding.EncodingError]:
    Ok(std.encoding.hex_from_text(text)?)

"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{HEADER}def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.crypto.AdvancedError | std.encoding.EncodingError]:\n{body}    Ok(())\n"
    )
}

fn run(source: &str) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    std::process::Command::new(executable).output().unwrap()
}

fn output(source: &str) -> String {
    let output = run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.crypto.AdvancedError", "E")
        .replace("std.bytes.Bytes", "Bytes")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

#[test]
fn ed25519_rfc8032_empty_message() {
    native(
        r#"
seed = unhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")?
message = std.bytes.empty()?
key = std.crypto.ed25519_public_key(&seed)?
mut signature = std.crypto.ed25519_sign(&seed, &message)?
print(hex(&key)?)?
print(hex(&signature)?)?
print(std.crypto.ed25519_verify(&key, &message, &signature)?)?
signature.set(0, 0u8)?
print(std.crypto.ed25519_verify(&key, &message, &signature)?)?
"#,
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a\ne5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b\nTrue\nFalse",
    );
}

/// Every encoding of the eight points of order dividing 8: y in {0, 1, p - 1,
/// the two order-8 values} and the non-canonical p and p + 1, each with both
/// sign bits (issue #569). The signature is a forgery that ring accepts for
/// the identity key: R = B and S = 1 satisfy [S]B = R + [k]A for A = 0.
const SMALL_ORDER_KEYS: &str = r#"[
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000080",
    "0100000000000000000000000000000000000000000000000000000000000000",
    "0100000000000000000000000000000000000000000000000000000000000080",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
]"#;

#[test]
fn ed25519_rejects_every_small_order_key_encoding() {
    native(
        &format!(
            r#"
forged = unhex("58666666666666666666666666666666666666666666666666666666666666660100000000000000000000000000000000000000000000000000000000000000")?
message = std.bytes.from_text("anything")?
encodings = {SMALL_ORDER_KEYS}?
for text in encodings:
    key = unhex(text)?
    print(str.repr(std.crypto.ed25519_verify(&key, &message, &forged))?)?
"#
        ),
        &["Result[bool, E].Err(E.SmallOrderKey)"; 14].join("\n"),
    );
}

/// Keys next to the blocklist reach ring: a different y with a matching top
/// byte, y = 2 and its non-canonical p + 2, and a mixed-order key (Wycheproof
/// tcId 160), which is accepted by design. Bad lengths stay `InvalidLength`.
#[test]
fn ed25519_small_order_check_is_exact() {
    native(
        r#"
forged = unhex("58666666666666666666666666666666666666666666666666666666666666660100000000000000000000000000000000000000000000000000000000000000")?
message = std.bytes.from_text("anything")?
for text in ["27e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05", "0200000000000000000000000000000000000000000000000000000000000000", "efffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f", "15deb30729eb3e465c88f11b57d0c8d36eb0f670c3b22d2f80ea6049eeb2c23b"]?:
    key = unhex(text)?
    print(str.repr(std.crypto.ed25519_verify(&key, &message, &forged))?)?
short = unhex("00000000000000000000000000000000000000000000000000000000000000")?
print(str.repr(std.crypto.ed25519_verify(&short, &message, &forged))?)?
identity = unhex("0100000000000000000000000000000000000000000000000000000000000000")?
print(str.repr(std.crypto.ed25519_verify(&identity, &message, &message))?)?
"#,
        "Result[bool, E].Ok(False)\nResult[bool, E].Ok(False)\nResult[bool, E].Ok(False)\nResult[bool, E].Ok(False)\nResult[bool, E].Err(E.InvalidLength)\nResult[bool, E].Err(E.InvalidLength)",
    );
}

/// The entrypoint report of a bare `AdvancedError` uses its std.report
/// `format`, which explains the rejection. Other variants are checked by
/// `to_string`.
#[test]
fn small_order_key_error_explains_why() {
    let output = run(r#"import std.bytes
import std.crypto
import std.report
from std.crypto import AdvancedError

def main() -> Result[(), AdvancedError]:
    match std.report.to_string(&AdvancedError.WeakKey, 256, 4):
        case Ok(text):
            match print(text):
                case _:
                    pass
        case Err(_):
            pass
    mut key = match std.bytes.empty():
        case Ok(bytes):
            bytes
        case Err(_):
            return Ok(())
    mut signature = match std.bytes.empty():
        case Ok(bytes):
            bytes
        case Err(_):
            return Ok(())
    if key.resize(32, 0u8).is_err() or signature.resize(64, 0u8).is_err():
        return Ok(())
    std.crypto.ed25519_verify(&key, &signature, &signature)?
    Ok(())
"#);
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "RSA modulus shorter than 2048 bits\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: main returned Ed25519 public key has small order: signatures can verify under it without any private key, so it is rejected\n"
    );
}

#[test]
fn hkdf_rfc5869_case_one_and_empty_inputs() {
    native(
        r#"
ikm = unhex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b")?
salt = unhex("000102030405060708090a0b0c")?
info = unhex("f0f1f2f3f4f5f6f7f8f9")?
print(hex(&std.crypto.hkdf_sha256(&salt, &ikm, &info, 42)?)?)?
empty = std.bytes.empty()?
print(std.crypto.hkdf_sha256(&empty, &empty, &empty, 0)?.len())?
print(std.crypto.hkdf_sha256(&empty, &empty, &empty, 8160)?.len())?
print(str.repr(std.crypto.hkdf_sha256(&empty, &empty, &empty, 8161))?)?
"#,
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865\n0\n8160\nResult[Bytes, E].Err(E.InvalidLength)",
    );
}

#[test]
fn aes256_gcm_zero_vector_and_authentication_failure() {
    native(
        r#"
key = unhex("0000000000000000000000000000000000000000000000000000000000000000")?
nonce = unhex("000000000000000000000000")?
aad = std.bytes.empty()?
mut data = unhex("00000000000000000000000000000000")?
std.crypto.seal_in_place(AeadAlgorithm.Aes256Gcm, &key, &nonce, &aad, &mut data)?
print(hex(&data)?)?
mut damaged = data.slice(0, data.len())?
damaged.set(31, 0u8)?
print(str.repr(std.crypto.open_in_place(AeadAlgorithm.Aes256Gcm, &key, &nonce, &aad, &mut damaged))?)?
print(hex(&damaged)?)?
std.crypto.open_in_place(AeadAlgorithm.Aes256Gcm, &key, &nonce, &aad, &mut data)?
print(hex(&data)?)?
"#,
        "cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919\nResult[(), E].Err(E.AuthenticationFailed)\n0000000000000000000000000000000000000000000000000000000000000000\n00000000000000000000000000000000",
    );
}

#[test]
fn chacha20_poly1305_rfc8439_vector() {
    native(
        r#"
key = unhex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f")?
nonce = unhex("070000004041424344454647")?
aad = unhex("50515253c0c1c2c3c4c5c6c7")?
mut data = std.bytes.from_text("Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.")?
std.crypto.seal_in_place(AeadAlgorithm.ChaCha20Poly1305, &key, &nonce, &aad, &mut data)?
print(hex(&data)?)?
std.crypto.open_in_place(AeadAlgorithm.ChaCha20Poly1305, &key, &nonce, &aad, &mut data)?
print(data.len())?
"#,
        "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b61161ae10b594f09e26a7e902ecbd0600691\n114",
    );
}

#[test]
fn x25519_shared_keys_and_single_use_including_invalid_peers() {
    native(
        r#"
mut alice = std.crypto.x25519()?
mut bob = std.crypto.x25519()?
a = alice.public_key()?
b = bob.public_key()?
left = alice.agree(&b)?
right = bob.agree(&a)?
print(std.crypto.equal(&left, &right), left.len())?
print(str.repr(alice.agree(&b))?)?
print(str.repr(alice.public_key())?)?
mut bad = std.crypto.x25519()?
short = std.bytes.empty()?
print(str.repr(bad.agree(&short))?)?
print(bad.public_key()?.len())?
zero = unhex("0000000000000000000000000000000000000000000000000000000000000000")?
mut target = zero.slice(0, 32)?
target.fill(0, 32, 255u8)?
print(str.repr(bad.agree_into(&zero, &mut target))?)?
print(hex(&target)?)?
print(str.repr(bad.agree(&b))?)?
"#,
        "True 32\nResult[Bytes, E].Err(E.Finished)\nResult[Bytes, E].Err(E.Finished)\nResult[Bytes, E].Err(E.InvalidLength)\n32\nResult[(), E].Err(E.InvalidPeer)\n0000000000000000000000000000000000000000000000000000000000000000\nResult[Bytes, E].Err(E.Finished)",
    );
}

#[test]
fn invalid_lengths_leave_outputs_unchanged_and_bad_aad_is_rejected() {
    native(
        r#"
empty = std.bytes.empty()?
key = unhex("0000000000000000000000000000000000000000000000000000000000000000")?
nonce = unhex("000000000000000000000000")?
mut data = std.bytes.from_text("unchanged")?
print(str.repr(std.crypto.seal_in_place(AeadAlgorithm.Aes256Gcm, &empty, &nonce, &empty, &mut data))?)?
print(data.to_text()?)?
print(str.repr(std.crypto.open_in_place(AeadAlgorithm.Aes256Gcm, &key, &nonce, &empty, &mut data))?)?
print(data.to_text()?)?
print(str.repr(std.crypto.ed25519_public_key_into(&key, &mut data))?)?
print(str.repr(std.crypto.ed25519_sign_into(&key, &empty, &mut data))?)?
print(str.repr(std.crypto.ed25519_verify(&key, &empty, &data))?)?
print(data.to_text()?)?
mut oversized = std.bytes.empty()?
oversized.resize(8161, 123u8)?
print(str.repr(std.crypto.hkdf_sha256_into(&empty, &key, &empty, &mut oversized))?)?
print(oversized.get(0)?, oversized.get(8160)?)?
mut ciphertext = std.bytes.empty()?
std.crypto.seal_in_place(AeadAlgorithm.ChaCha20Poly1305, &key, &nonce, &empty, &mut ciphertext)?
print(ciphertext.len())?
wrong_aad = std.bytes.from_text("wrong")?
print(str.repr(std.crypto.open_in_place(AeadAlgorithm.ChaCha20Poly1305, &key, &nonce, &wrong_aad, &mut ciphertext))?)?
print(hex(&ciphertext)?)?
"#,
        "Result[(), E].Err(E.InvalidLength)\nunchanged\nResult[(), E].Err(E.InvalidLength)\nunchanged\nResult[(), E].Err(E.InvalidLength)\nResult[(), E].Err(E.InvalidLength)\nResult[bool, E].Err(E.InvalidLength)\nunchanged\nResult[(), E].Err(E.InvalidLength)\n123 123\n16\nResult[(), E].Err(E.AuthenticationFailed)\n00000000000000000000000000000000",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reusable_buffers_do_not_allocate() {
    native(
        r#"
seed = unhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")?
nonce = unhex("000000000000000000000000")?
empty = std.bytes.empty()?
mut message = std.bytes.from_text("message")?
message.reserve(16)?
mut public = std.bytes.empty()?
public.resize(32, 0u8)?
mut signature = std.bytes.empty()?
signature.resize(64, 0u8)?
mut derived = std.bytes.empty()?
derived.resize(32, 0u8)?
mut alice = std.crypto.x25519()?
bob = std.crypto.x25519()?
peer = bob.public_key()?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
std.crypto.ed25519_public_key_into(&seed, &mut public)?
std.crypto.ed25519_sign_into(&seed, &message, &mut signature)?
verified = std.crypto.ed25519_verify(&public, &message, &signature)?
std.crypto.hkdf_sha256_into(&empty, &seed, &empty, &mut derived)?
std.crypto.seal_in_place(AeadAlgorithm.Aes256Gcm, &seed, &nonce, &empty, &mut message)?
std.crypto.open_in_place(AeadAlgorithm.Aes256Gcm, &seed, &nonce, &empty, &mut message)?
std.crypto.seal_in_place(AeadAlgorithm.ChaCha20Poly1305, &seed, &nonce, &empty, &mut message)?
std.crypto.open_in_place(AeadAlgorithm.ChaCha20Poly1305, &seed, &nonce, &empty, &mut message)?
alice.public_key_into(&mut public)?
alice.agree_into(&peer, &mut derived)?
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(verified)?
print(message.to_text()?)?
"#,
        "True\nmessage",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_do_not_consume_keys_or_encrypt_plaintext() {
    native(
        r#"
key = unhex("0000000000000000000000000000000000000000000000000000000000000000")?
nonce = unhex("000000000000000000000000")?
empty = std.bytes.empty()?
mut message = std.bytes.from_text("message")?
mut alice = std.crypto.x25519()?
bob = std.crypto.x25519()?
peer = bob.public_key()?
print("__test_fail_allocations_after_0__")?
seal = std.crypto.seal_in_place(AeadAlgorithm.Aes256Gcm, &key, &nonce, &empty, &mut message)
exchange = alice.agree(&peer)
created = std.crypto.x25519()
signed = std.crypto.ed25519_sign(&key, &message)
derived = std.crypto.hkdf_sha256(&empty, &key, &empty, 32)
print("__test_restore_allocations__")?
print(str.repr(seal)?)?
print(str.repr(exchange)?)?
print(str.repr(created)?)?
print(str.repr(signed)?)?
print(str.repr(derived)?)?
print(message.to_text()?)?
print(alice.agree(&peer)?.len())?
print("__test_fail_allocations_after_1__")?
partial = std.crypto.x25519()
print("__test_restore_allocations__")?
print(str.repr(partial)?)?
print("__test_fail_allocations_after_2__")?
partial2 = std.crypto.x25519()
print("__test_restore_allocations__")?
print(str.repr(partial2)?)?
"#,
        "Result[(), E].Err(E.Allocation(AllocError.OutOfMemory))\nResult[Bytes, E].Err(E.Allocation(AllocError.OutOfMemory))\nResult[std.crypto.X25519Key, E].Err(E.Allocation(AllocError.OutOfMemory))\nResult[Bytes, E].Err(E.Allocation(AllocError.OutOfMemory))\nResult[Bytes, E].Err(E.Allocation(AllocError.OutOfMemory))\nmessage\n32\nResult[std.crypto.X25519Key, E].Err(E.Allocation(AllocError.OutOfMemory))\nResult[std.crypto.X25519Key, E].Err(E.Allocation(AllocError.OutOfMemory))",
    );
}

#[test]
fn mutable_output_cannot_alias_inputs() {
    let error = support::check_file_error(&program(
        r#"
mut key = std.bytes.empty()?
nonce = std.bytes.empty()?
aad = std.bytes.empty()?
std.crypto.seal_in_place(AeadAlgorithm.Aes256Gcm, &key, &nonce, &aad, &mut key)?
"#,
    ));
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn exchange_state_is_private_noncopyable_and_rejects_forged_buffers() {
    let forged = support::check_file_error(&program(
        "state = std.bytes.empty()?\nkey = std.crypto.X25519Key(state)\n",
    ));
    assert!(forged.contains("private"), "{forged}");
    let copied =
        support::check_file_error(&program("key = std.crypto.x25519()?\nother = copy(key)?\n"));
    assert!(copied.contains("cannot be copied"), "{copied}");
    let forged = run(&program(
        "state = std.bytes.empty()?\nmut target = std.bytes.empty()?\ntarget.resize(32, 0u8)?\nprint(std.bytes.crypto_x_public(&state, &mut target))?\n",
    ));
    assert!(!forged.status.success());
    assert!(String::from_utf8_lossy(&forged.stderr).contains("operand is not an X25519 key"));
    native(
        "key = std.crypto.x25519()?\nprint(key)?\n",
        "std.crypto.X25519Key(@redacted)",
    );
}
