#!/usr/bin/env python3
"""Regenerate the compact Wycheproof extracts that tests/test_std_crypto_wycheproof.rs reads.

Usage: generate.py UPSTREAM_DIR

UPSTREAM_DIR holds the files below, copied from testvectors_v1/ of
https://github.com/C2SP/wycheproof at COMMIT; LICENSE comes from the same
commit. Each file's SHA-256 is checked before it is read, so an extract always
corresponds to the recorded upstream bytes. The extract keeps only the key
encodings and test cases the test uses, dropping comments, PEM and ASN text.
"""

import hashlib
import json
import pathlib
import sys

COMMIT = "d17cc89fa5c8f4f4d378b5385ebf75da5d0926c9"

SOURCES = {
    "rsa_signature_2048_sha256_test.json": "94a917b01ff50fb874cfc05bf29b4af44868d944a6558201cf18380da93fb393",
    "rsa_signature_2048_sha384_test.json": "c571c105d261c0ff588a2888a529f152563fb3b77894b7620d4e4f8f934f2c1d",
    "rsa_signature_2048_sha512_test.json": "16ea24b039905d054bdb6004f5fd179374e150b7b6d73a7ba654b6d00eab12ef",
    "rsa_signature_3072_sha256_test.json": "0f5f18cabfaad3e2792e82f7e9882f8999049b456714de924b8a5e202f61ca43",
    "rsa_signature_4096_sha512_test.json": "9d6e67b1eb0d7040db411835238efd20e9302399a258bdff82de3f38e0f2855a",
    "rsa_pss_2048_sha256_mgf1_32_test.json": "7f6efafc160f4816b96cbf1c12188a31051d7e3f001e27505d9edb5f2a0e325c",
    "rsa_pss_2048_sha384_mgf1_48_test.json": "66d464778b0b2f683a1d1a20e94f77e9472b8cc393b45f423fa08a48e677063c",
    "rsa_pss_4096_sha512_mgf1_64_test.json": "c93ceaa56a190c9fd4707441c5c6a75839f108202d88aec891697f9623042547",
    "ecdsa_secp256r1_sha256_p1363_test.json": "c60de693930e386c3a5472d08081623ef8504decc54b38ac01ec6b2a2575c986",
    "ecdsa_secp256r1_sha256_test.json": "182db4f3e230f6f9fa9f800d2a614dede30284b8e8438bbfe1171905402e9332",
    "ed25519_test.json": "ea80ffebac085e21cb9d3e903e7fa2b448d96702da5b31f061d0b83b430b74b3",
}
LICENSE_SHA256 = "58d1e17ffe5109a7ae296caafcadfdbe6a7d176f0bc4ab01e12a689b0499d8bd"


def digest(path: pathlib.Path, expected: str) -> bytes:
    data = path.read_bytes()
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        sys.exit(f"{path}: SHA-256 {actual}, expected {expected}")
    return data


def key(group: dict) -> dict:
    """The encodings each importer receives: SPKI DER, the raw point or key, and
    any JWK members as text."""
    out = {"spki": group["publicKeyDer"]}
    jwk = group.get("publicKeyJwk") or group.get("keyJwk")
    if "uncompressed" in group["publicKey"]:
        out["point"] = group["publicKey"]["uncompressed"]
    if "pk" in group["publicKey"]:
        out["pk"] = group["publicKey"]["pk"]
    if jwk is not None:
        for member in ("n", "e", "x", "y"):
            if member in jwk:
                out[member] = jwk[member]
    return out


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    upstream = pathlib.Path(sys.argv[1])
    here = pathlib.Path(__file__).resolve().parent
    (here / "LICENSE").write_bytes(digest(upstream / "LICENSE", LICENSE_SHA256))
    for name, sha256 in SOURCES.items():
        source = json.loads(digest(upstream / name, sha256))
        lines = [
            json.dumps(
                {
                    "source": f"C2SP/wycheproof {COMMIT} testvectors_v1/{name}",
                    "sha256": sha256,
                    "algorithm": source["algorithm"],
                }
            )
        ]
        for group in source["testGroups"]:
            lines.append(json.dumps({"key": key(group)}))
            for test in group["tests"]:
                lines.append(
                    json.dumps([test["tcId"], test["result"], test["msg"], test["sig"]])
                )
        (here / name.replace("_test.json", ".jsonl")).write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
