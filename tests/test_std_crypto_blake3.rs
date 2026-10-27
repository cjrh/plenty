//! Official BLAKE3 1.8.7 vectors, including every upstream block/tree boundary.
//! Fixture copied unmodified from BLAKE3-team/BLAKE3 tag 1.8.7,
//! test_vectors/test_vectors.json (CC0-1.0 OR Apache-2.0 OR
//! Apache-2.0 WITH LLVM-exception; licenses in the vendored blake3 directory).
use super::test_std_crypto::native;

#[test]
fn official_blake3_vectors_cover_all_three_modes_and_incremental_input() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/crypto/blake3-1.8.7.json")).unwrap();
    let key = vectors["key"].as_str().unwrap();
    let context = vectors["context_string"].as_str().unwrap();
    for (case, vector) in vectors["cases"].as_array().unwrap().iter().enumerate() {
        // Separate programs keep each case below the compiler's local-slot
        // limit and make a failure identify its input length directly.
        let mut source = format!("key = std.bytes.from_text(\"{key}\")?\n");
        let mut expected = Vec::new();
        let len = vector["input_len"].as_u64().unwrap();
        // Pattern from the upstream fixture, generated in Plenty so long
        // tree-boundary inputs don't inflate compiler source literals.
        source.push_str(&format!(
            "mut data{case} = std.bytes.with_capacity({len})?\n\
             for i in range({len}):\n    data{case}.push(u8(i % 251))?\n\
             print(hex(&std.crypto.blake3(&data{case})?)?)?\n\
             print(hex(&std.crypto.blake3_keyed(&key, &data{case})?)?)?\n\
             print(hex(&std.crypto.blake3_derive_key(\"{context}\", &data{case})?)?)?\n\
             mut hash{case} = std.crypto.digest(Algorithm.Blake3)?\n\
             mut mac{case} = std.crypto.blake3_keyed_hasher(&key)?\n\
             mut kdf{case} = std.crypto.blake3_derive_hasher(\"{context}\")?\n\
             hash{case}.update(&data{case}, 0, {half})?\n\
             hash{case}.update(&data{case}, {half}, {len})?\n\
             mac{case}.update(&data{case}, 0, {half})?\n\
             mac{case}.update(&data{case}, {half}, {len})?\n\
             kdf{case}.update(&data{case}, 0, {half})?\n\
             kdf{case}.update(&data{case}, {half}, {len})?\n\
             print(hex(&hash{case}.finish()?)?)?\n\
             print(hex(&mac{case}.finish()?)?)?\n\
             print(hex(&kdf{case}.finish()?)?)?\n",
            half = len / 2,
        ));
        for _ in 0..2 {
            for mode in ["hash", "keyed_hash", "derive_key"] {
                expected.push(vector[mode].as_str().unwrap()[..64].to_owned());
            }
        }
        native(&source, &expected.join("\n"));
    }
}

#[test]
fn blake3_ranges_finished_states_and_key_validation() {
    native(
        r#"
key = std.bytes.from_text("whats the Elvish word for friend")?
empty = std.bytes.from_text("")?
print(str.repr(std.crypto.blake3_keyed_hasher(&empty))?)?
long_key = std.bytes.from_text("whats the Elvish word for friends")?
print(str.repr(std.crypto.blake3_keyed_hasher(&long_key))?)?
print(str.repr(std.crypto.hmac(Algorithm.Blake3, &key))?)?
mut mac = std.crypto.blake3_keyed_hasher(&key)?
print(mac.size())?
print(str.repr(mac.update(&empty, -1, 0))?)?
tag = std.crypto.blake3_keyed(&key, &empty)?
print(mac.verify(&tag, 0, 32)?)?
print(str.repr(mac.verify(&tag, 0, 32))?)?
mut mismatch = std.crypto.blake3_keyed_hasher(&key)?
print(mismatch.verify(&tag, 0, 31)?)?
mut digest = std.crypto.digest(Algorithm.Blake3)?
mut target = std.bytes.with_capacity(34)?
target.resize(34, 7u8)?
print(str.repr(digest.finish_into(&mut target, 3))?)?
digest.finish_into(&mut target, 1)?
print(target[0], target[33])?
print(hex(&target.slice(1, 33)?)?)?
print(str.repr(digest.update_text(""))?)?
print(str.repr(digest.finish())?)?
"#,
        "Result[std.crypto.Mac, E].Err(E.InvalidKeyLength)\n\
         Result[std.crypto.Mac, E].Err(E.InvalidKeyLength)\n\
         Result[std.crypto.Mac, E].Err(E.UnsupportedAlgorithm)\n\
         32\nResult[(), E].Err(E.OutOfRange)\nTrue\n\
         Result[bool, E].Err(E.Finished)\nFalse\n\
         Result[(), E].Err(E.OutOfRange)\n7 7\n\
         af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262\n\
         Result[(), E].Err(E.Finished)\nResult[Bytes, E].Err(E.Finished)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn blake3_updates_output_and_verification_allocate_nothing() {
    native(
        r#"
key = std.bytes.from_text("whats the Elvish word for friend")?
mut data = std.bytes.with_capacity(8193)?
data.resize(8193, 1u8)?
mut target = std.bytes.with_capacity(96)?
target.resize(96, 0u8)?
mut hash = std.crypto.digest(Algorithm.Blake3)?
mut mac = std.crypto.blake3_keyed_hasher(&key)?
mut kdf = std.crypto.blake3_derive_hasher("Plenty tests 2026-10-27 no-allocation test")?
mut verifier = std.crypto.blake3_keyed_hasher(&key)?
tag = std.crypto.blake3_keyed(&key, &data)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
hash.update(&data, 0, data.len())?
hash.update_text("雪")?
hash.finish_into(&mut target, 0)?
mac.update(&data, 0, data.len())?
mac.finish_into(&mut target, 32)?
kdf.update(&data, 0, data.len())?
kdf.finish_into(&mut target, 64)?
verifier.update(&data, 0, data.len())?
verified = verifier.verify(&tag, 0, tag.len())?
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(verified)?
"#,
        "True",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn blake3_allocation_errors_leave_finish_retryable() {
    native(
        r#"
key = std.bytes.from_text("whats the Elvish word for friend")?
print("__test_fail_allocations_after_0__")?
hash = std.crypto.digest(Algorithm.Blake3)
mac = std.crypto.blake3_keyed_hasher(&key)
kdf = std.crypto.blake3_derive_hasher("context")
print("__test_restore_allocations__")?
print(str.repr(hash)?)?
print(str.repr(mac)?)?
print(str.repr(kdf)?)?
mut state = std.crypto.digest(Algorithm.Blake3)?
print("__test_fail_allocations_after_0__")?
first = state.finish()
print("__test_restore_allocations__")?
print("__test_fail_allocations_after_1__")?
second = state.finish()
print("__test_restore_allocations__")?
print(str.repr(first)?)?
print(str.repr(second)?)?
print(hex(&state.finish()?)?)?
mut unfinished = std.crypto.blake3_keyed_hasher(&key)?
unfinished.update_text("secret")?
"#,
        "Result[std.crypto.Digest, E].Err(E.Allocation(AllocError.OutOfMemory))\n\
         Result[std.crypto.Mac, E].Err(E.Allocation(AllocError.OutOfMemory))\n\
         Result[std.crypto.Digest, E].Err(E.Allocation(AllocError.OutOfMemory))\n\
         Result[Bytes, E].Err(E.Allocation(AllocError.OutOfMemory))\n\
         Result[Bytes, E].Err(E.Allocation(AllocError.OutOfMemory))\n\
         af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
    );
}
