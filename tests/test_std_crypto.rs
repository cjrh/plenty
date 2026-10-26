//! `std.crypto`: SHA-256, SHA-512, HMAC and constant-time comparison, checked
//! against published vectors (FIPS 180-2 examples, RFC 4231).
mod support;
use support::check_file_error as rejected;

const HEADER: &str = r#"import std.bytes
import std.crypto
import std.encoding
from std.bytes import Bytes
from std.crypto import Algorithm, CryptoError

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
        "{HEADER}def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.crypto.CryptoError | std.encoding.EncodingError]:\n{body}    Ok(())\n"
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
        .replace("std.crypto.CryptoError", "E")
        .replace("std.bytes.Bytes", "Bytes")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

/// FIPS 180-2 example messages: (input, SHA-256, SHA-512).
const DIGESTS: &[(&str, &str, &str)] = &[
    (
        "",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
    ),
    (
        "abc",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
    ),
    (
        "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        "204a8fc6dda82f0a0ced7beb8e08a41657c16ef468b228a8279be331a703c33596fd15c13b1b07f9aa1d3bea57789ca031ad85c7a71dd70354ec631238ca3445",
    ),
    (
        "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
        "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909",
    ),
];

#[test]
fn digests_match_the_fips_examples() {
    let mut body = String::new();
    let mut expected = Vec::new();
    for (case, (input, sha256, sha512)) in DIGESTS.iter().enumerate() {
        body.push_str(&format!(
            "data{case} = std.bytes.from_text(\"{input}\")?\n\
             print(hex(&std.crypto.sha256(&data{case})?)?)?\n\
             print(hex(&std.crypto.sha512(&data{case})?)?)?\n"
        ));
        expected.extend([*sha256, *sha512]);
    }
    native(&body, &expected.join("\n"));
}

#[test]
fn a_million_bytes_fed_in_pieces_match_the_fips_example() {
    native(
        r#"
block = std.bytes.from_text("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")?
mut short = std.crypto.digest(Algorithm.Sha256)?
mut long = std.crypto.digest(Algorithm.Sha512)?
for _ in range(10000):
    short.update(&block, 0, 100)?
    long.update(&block, 0, 37)?
    long.update(&block, 37, 100)?
print(short.size(), hex(&short.finish()?)?)?
print(long.size(), hex(&long.finish()?)?)?
"#,
        "32 cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0\n\
         64 e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b",
    );
}

/// RFC 4231 test cases 1-4, 6 and 7: (key, data, HMAC-SHA-256, HMAC-SHA-512).
/// Case 5 truncates the tag; `truncated_and_altered_tags_do_not_verify`
/// covers it.
fn rfc4231() -> Vec<(String, String, &'static str, &'static str)> {
    let long_key = "aa".repeat(131);
    vec![
        (
            "0b".repeat(20),
            "4869205468657265".into(),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            "87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cdedaa833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854",
        ),
        (
            "4a656665".into(),
            "7768617420646f2079612077616e7420666f72206e6f7468696e673f".into(),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737",
        ),
        (
            "aa".repeat(20),
            "dd".repeat(50),
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
            "fa73b0089d56a284efb0f0756c890be9b1b5dbdd8ee81a3655f83e33b2279d39bf3e848279a722c806b485a47e67c807b946a337bee8942674278859e13292fb",
        ),
        (
            "0102030405060708090a0b0c0d0e0f10111213141516171819".into(),
            "cd".repeat(50),
            "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b",
            "b0ba465637458c6990e5a8c5f61d4af7e576d97ff94b872de76f8050361ee3dba91ca5c11aa25eb4d679275cc5788063a5f19741120c4f2de2adebeb10a298dd",
        ),
        (
            long_key.clone(),
            "54657374205573696e67204c6172676572205468616e20426c6f636b2d53697a65204b6579202d2048617368204b6579204669727374".into(),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
            "80b24263c7c1a3ebb71493c1dd7be8b49b46d1f41b4aeec1121b013783f8f3526b56d037e05f2598bd0fd2215d6a1e5295e64f73f63f0aec8b915a985d786598",
        ),
        (
            long_key,
            "5468697320697320612074657374207573696e672061206c6172676572207468616e20626c6f636b2d73697a65206b657920616e642061206c6172676572207468616e20626c6f636b2d73697a6520646174612e20546865206b6579206e6565647320746f20626520686173686564206265666f7265206265696e6720757365642062792074686520484d414320616c676f726974686d2e".into(),
            "9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2",
            "e37b6a775dc87dbaa4dfa9f96e5e3ffddebd71f8867289865df5a32d20cdc944b6022cac3c4982b10d5eeb55c3e4de15134676fb6de0446065c97440fa8c6a58",
        ),
    ]
}

#[test]
fn hmacs_match_rfc_4231() {
    let mut body = String::new();
    let mut expected = Vec::new();
    for (case, (key, data, sha256, sha512)) in rfc4231().into_iter().enumerate() {
        body.push_str(&format!(
            "key{case} = unhex(\"{key}\")?\n\
             data{case} = unhex(\"{data}\")?\n\
             print(hex(&std.crypto.hmac_sha256(&key{case}, &data{case})?)?)?\n\
             print(hex(&std.crypto.hmac_sha512(&key{case}, &data{case})?)?)?\n\
             mut mac{case} = std.crypto.hmac(Algorithm.Sha256, &key{case})?\n\
             mac{case}.update(&data{case}, 0, data{case}.len())?\n\
             print(mac{case}.verify(&unhex(\"{sha256}\")?, 0, 32)?)?\n"
        ));
        expected.extend([sha256, sha512, "True"]);
    }
    native(&body, &expected.join("\n"));
}

#[test]
fn incremental_input_equals_whole_input_at_every_split() {
    // Splits cross both block sizes (64 and 128 bytes) and the length field.
    native(
        r#"
mut data = std.bytes.empty()?
for index in range(300):
    data.push(u8((index * 7) % 256))?
key = std.bytes.from_text("split key")?
mut mismatches = 0
for round in range(2):
    mut algorithm = Algorithm.Sha256
    if round == 1:
        algorithm = Algorithm.Sha512
    mut whole_mac = std.crypto.hmac(algorithm, &key)?
    whole_mac.update(&data, 0, data.len())?
    whole = whole_mac.finish()?
    mut plain = std.crypto.digest(algorithm)?
    plain.update(&data, 0, data.len())?
    expected = plain.finish()?
    for split in range(0, 301, 3):
        mut parts = std.crypto.digest(algorithm)?
        parts.update(&data, 0, split)?
        parts.update(&data, split, split)?
        parts.update(&data, split, data.len())?
        if not std.crypto.equal(&parts.finish()?, &expected):
            mismatches += 1
        mut mac = std.crypto.hmac(algorithm, &key)?
        mac.update(&data, 0, split)?
        mac.update(&data, split, data.len())?
        if not mac.verify(&whole, 0, whole.len())?:
            mismatches += 1
print(mismatches)?
mut text = std.crypto.digest(Algorithm.Sha256)?
text.update_text("ab")?
text.update_text("")?
text.update_text("c")?
print(hex(&text.finish()?)?)?
"#,
        "0\nba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
}

#[test]
fn truncated_and_altered_tags_do_not_verify() {
    // RFC 4231 case 5 publishes a truncated tag; HMAC verification compares
    // full-length tags, so a truncated one is a mismatch.
    native(
        r#"
key = unhex("0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c")?
data = std.bytes.from_text("Test With Truncation")?
mut tag = std.crypto.hmac_sha256(&key, &data)?
print(hex(&tag)?)?
mut mac = std.crypto.hmac(Algorithm.Sha256, &key)?
mac.update(&data, 0, data.len())?
print(mac.verify(&tag, 0, 16)?)?
tag.set(31, tag.get(31)? ^ 1u8)?
mut again = std.crypto.hmac(Algorithm.Sha256, &key)?
again.update(&data, 0, data.len())?
print(again.verify(&tag, 0, 32)?)?
"#,
        "a3b6167473100ee06e0c796c2955552bfa6f7c0a6a8aef8b93f860aab0cd20c5\nFalse\nFalse",
    );
}

#[test]
fn equal_compares_contents_and_lengths() {
    native(
        r#"
a = std.bytes.from_text("token")?
b = std.bytes.from_text("token")?
c = std.bytes.from_text("tokens")?
d = std.bytes.from_text("tokan")?
e = std.bytes.empty()?
print(std.crypto.equal(&a, &b), std.crypto.equal(&a, &c), std.crypto.equal(&a, &d))?
print(std.crypto.equal(&e, &e), std.crypto.equal(&e, &a))?
"#,
        "True False False\nTrue False",
    );
}

#[test]
fn invalid_ranges_and_finished_states_are_errors() {
    native(
        r#"
data = std.bytes.from_text("abc")?
mut digest = std.crypto.digest(Algorithm.Sha256)?
print(str.repr(digest.update(&data, 2, 1))?)?
print(str.repr(digest.update(&data, -1, 2))?)?
print(str.repr(digest.update(&data, 0, 4))?)?
mut target = std.bytes.empty()?
target.resize(40, 0u8)?
print(str.repr(digest.finish_into(&mut target, 9))?)?
print(str.repr(digest.finish_into(&mut target, -1))?)?
digest.update(&data, 0, 3)?
digest.finish_into(&mut target, 8)?
print(hex(&target.slice(8, 40)?)?)?
print(str.repr(digest.update(&data, 0, 3))?)?
print(str.repr(digest.update_text("x"))?)?
print(str.repr(digest.finish())?)?
print(str.repr(digest.finish_into(&mut target, 0))?)?
mut mac = std.crypto.hmac(Algorithm.Sha512, &data)?
print(mac.size(), str.repr(mac.verify(&data, 1, 0))?)?
print(mac.verify(&data, 0, 3)?)?
print(str.repr(mac.verify(&data, 0, 3))?)?
"#,
        "Result[(), E].Err(E.OutOfRange)\n\
         Result[(), E].Err(E.OutOfRange)\n\
         Result[(), E].Err(E.OutOfRange)\n\
         Result[(), E].Err(E.OutOfRange)\n\
         Result[(), E].Err(E.OutOfRange)\n\
         ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
         Result[(), E].Err(E.Finished)\n\
         Result[(), E].Err(E.Finished)\n\
         Result[Bytes, E].Err(E.Finished)\n\
         Result[(), E].Err(E.Finished)\n\
         64 Result[bool, E].Err(E.OutOfRange)\n\
         False\n\
         Result[bool, E].Err(E.Finished)",
    );
}

#[test]
fn printing_a_mac_shows_no_key_material() {
    native(
        r#"
key = std.bytes.from_text("secret")?
mac = std.crypto.hmac(Algorithm.Sha256, &key)?
print(mac)?
"#,
        "std.crypto.Mac(_state=Bytes(_owner=b\"\"), _size=32)",
    );
}

#[test]
fn states_cannot_be_forged_or_copied() {
    let forged = rejected(&program(
        "key = std.bytes.from_text(\"k\")?\nmac = std.crypto.Mac(key, 32)\n",
    ));
    assert!(forged.contains("private"), "{forged}");
    let copied = rejected(&program(
        "mac = std.crypto.digest(Algorithm.Sha256)?\nother = copy(mac)?\n",
    ));
    assert!(copied.contains("cannot be copied"), "{copied}");
    // The back-end methods are public on `Bytes`; an ordinary buffer is not a
    // hasher state, and the runtime stops rather than reading it as one.
    let output = run(&program(
        "mut state = std.bytes.from_text(\"not a state\")?\n\
         data = std.bytes.from_text(\"abc\")?\n\
         print(state.crypto_update(&data, 0, 3)?)?\n",
    ));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("operand is not a std.crypto state"),
        "{stderr}"
    );
}

#[test]
fn hashing_works_in_worker_threads() {
    let source = format!(
        "{HEADER}{}",
        r#"
def work(text: str) -> ():
    data = std.bytes.from_text(text).unwrap()
    print(hex(&std.crypto.sha256(&data).unwrap()).unwrap()).unwrap()

def main() -> Result[(), AllocError | IoError | ThreadError]:
    with spawn(work, "abc")?:
        pass
    Ok(())
"#
    );
    assert_eq!(
        output(&source),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// Every program links with `--gc-sections`, and the hashing code sits behind
/// its own runtime entry, so only programs that hash carry it.
#[test]
fn programs_that_never_hash_do_not_link_the_hashing_code() {
    use object::{Object, ObjectSymbol, SymbolKind};
    let symbols = |source: &str| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        let executable = dir.path().join("program");
        std::fs::write(&path, source).unwrap();
        support::compile_unstripped(&path, &executable);
        let data = std::fs::read(&executable).unwrap();
        let file = object::File::parse(&*data).unwrap();
        file.symbols()
            // Functions only: mold also names each GOT entry, `NAME$got`.
            .filter(|symbol| symbol.kind() == SymbolKind::Text)
            .filter_map(|symbol| symbol.name().ok().map(str::to_owned))
            .filter(|name| name == "plenty_crypto" || name.contains("compress256"))
            .count()
    };
    let plain = "def main() -> Result[(), AllocError | IoError]:\n    print(\"hi\")?\n    Ok(())\n";
    assert_eq!(symbols(plain), 0);
    assert_eq!(
        symbols(&program(
            "print(hex(&std.crypto.sha256(&std.bytes.empty()?)?)?)?\n"
        )),
        2
    );
}

/// `build.rs` compiles the runtime's copies under `plenty-runtime/vendor`;
/// Cargo builds the same crate from `Cargo.lock`. Both must name one version.
#[test]
fn vendored_runtime_crates_match_the_lockfile() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let mut vendored: Vec<_> = std::fs::read_dir(root.join("plenty-runtime/vendor"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    vendored.sort();
    assert_eq!(vendored.len(), 21, "{vendored:?}");
    for directory in vendored {
        let (name, version) = directory.rsplit_once('-').unwrap();
        let entry = format!("name = \"{name}\"\nversion = \"{version}\"\n");
        assert!(lock.contains(&entry), "{directory} is not in Cargo.lock");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn finishing_into_an_existing_buffer_does_not_allocate() {
    native(
        r#"
data = std.bytes.from_text("abc")?
key = std.bytes.from_text("key")?
mut target = std.bytes.empty()?
target.resize(96, 0u8)?
mut digest = std.crypto.digest(Algorithm.Sha512)?
mut mac = std.crypto.hmac(Algorithm.Sha256, &key)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
digest.update(&data, 0, 3)?
digest.update_text("")?
digest.finish_into(&mut target, 0)?
mac.update(&data, 0, 3)?
mac.finish_into(&mut target, 64)?
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(hex(&target.slice(0, 8)?)?)?
"#,
        "ddaf35a193617aba",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_and_leaves_the_state_usable() {
    // A state is one allocation; `finish` adds the output's storage and owner.
    // The exit-time balance check verifies that nothing leaks, including the
    // unfinished states dropped at the end.
    native(
        r#"
data = std.bytes.from_text("abc")?
key = std.bytes.from_text("key")?
print("__test_fail_allocations_after_0__")?
plain = std.crypto.digest(Algorithm.Sha256)
keyed = std.crypto.hmac(Algorithm.Sha512, &key)
print("__test_restore_allocations__")?
print(str.repr(plain)?)?
print(str.repr(keyed)?)?
mut digest = std.crypto.digest(Algorithm.Sha256)?
digest.update(&data, 0, 3)?
print("__test_fail_allocations_after_0__")?
first = digest.finish()
print("__test_restore_allocations__")?
print("__test_fail_allocations_after_1__")?
second = digest.finish()
print("__test_restore_allocations__")?
print(str.repr(first)?)?
print(str.repr(second)?)?
print(hex(&digest.finish()?)?)?
mut unfinished = std.crypto.hmac(Algorithm.Sha256, &key)?
unfinished.update(&data, 0, 3)?
"#,
        "Result[std.crypto.Digest, E].Err(E.Allocation(AllocError.OutOfMemory))\n\
         Result[std.crypto.Mac, E].Err(E.Allocation(AllocError.OutOfMemory))\n\
         Result[Bytes, E].Err(E.Allocation(AllocError.OutOfMemory))\n\
         Result[Bytes, E].Err(E.Allocation(AllocError.OutOfMemory))\n\
         ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
}
