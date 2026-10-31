//! Match the pinned ring build script's ABI-specific pregenerated assembly.
const RING_X86_SOURCES: &[&str] = &[
    "crypto/curve25519/curve25519.c",
    "crypto/fipsmodule/aes/aes_nohw.c",
    "crypto/fipsmodule/bn/montgomery.c",
    "crypto/fipsmodule/bn/montgomery_inv.c",
    "crypto/fipsmodule/ec/ecp_nistz.c",
    "crypto/fipsmodule/ec/gfp_p256.c",
    "crypto/fipsmodule/ec/gfp_p384.c",
    "crypto/fipsmodule/ec/p256.c",
    "crypto/limbs/limbs.c",
    "crypto/mem.c",
    "crypto/poly1305/poly1305.c",
    "crypto/crypto.c",
    "crypto/cpu_intel.c",
    "crypto/curve25519/curve25519_64_adx.c",
    "crypto/fipsmodule/ec/p256-nistz.c",
    "third_party/fiat/asm/fiat_curve25519_adx_mul.S",
    "third_party/fiat/asm/fiat_curve25519_adx_square.S",
    "pregenerated/chacha-x86_64-elf.S",
    "pregenerated/aes-gcm-avx2-x86_64-elf.S",
    "pregenerated/aesni-gcm-x86_64-elf.S",
    "pregenerated/aesni-x86_64-elf.S",
    "pregenerated/ghash-x86_64-elf.S",
    "pregenerated/vpaes-x86_64-elf.S",
    "pregenerated/x86_64-mont-elf.S",
    "pregenerated/x86_64-mont5-elf.S",
    "pregenerated/p256-x86_64-asm-elf.S",
    "pregenerated/sha512-x86_64-elf.S",
    "pregenerated/sha256-x86_64-elf.S",
    "pregenerated/chacha20_poly1305_x86_64-elf.S",
];

/// Match ring 0.17.14's upstream RING_SRCS and linux64 pregenerated selection.
pub fn ring_sources() -> Vec<String> {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    ring_sources_for(&arch, &os)
}

fn ring_sources_for(arch: &str, os: &str) -> Vec<String> {
    let sources: Vec<&str> = match arch {
        "x86_64" => RING_X86_SOURCES.to_vec(),
        "aarch64" => RING_X86_SOURCES[..11]
            .iter()
            .copied()
            .chain([
                "crypto/fipsmodule/ec/p256-nistz.c",
                "pregenerated/chacha-armv8-linux64.S",
                "pregenerated/chacha20_poly1305_armv8-linux64.S",
                "pregenerated/aesv8-armx-linux64.S",
                "pregenerated/aesv8-gcm-armv8-linux64.S",
                "pregenerated/ghash-neon-armv8-linux64.S",
                "pregenerated/ghashv8-armx-linux64.S",
                "pregenerated/vpaes-armv8-linux64.S",
                "pregenerated/armv8-mont-linux64.S",
                "pregenerated/p256-armv8-asm-linux64.S",
                "pregenerated/sha512-armv8-linux64.S",
                "pregenerated/sha256-armv8-linux64.S",
            ])
            .collect(),
        arch => panic!("embedded runtime does not support architecture {arch}"),
    };
    sources
        .into_iter()
        .filter_map(|source| {
            if os == "windows" && source.starts_with("third_party/fiat/asm/") {
                return None;
            }
            let source = match (arch, os) {
                ("x86_64", "macos") => source.replace("-elf.S", "-macosx.S"),
                ("aarch64", "macos") => source.replace("-linux64.S", "-ios64.S"),
                // Upstream supplies NASM-assembled COFF objects for this ABI.
                ("x86_64", "windows") => source.replace("-elf.S", "-nasm.o"),
                ("aarch64", "windows") => source.replace("-linux64.S", "-win64.S"),
                _ => source.to_owned(),
            };
            Some(source)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_matrix_contains_complete_platform_inputs() {
        let root = std::env::current_dir()
            .unwrap()
            .join("plenty-runtime/vendor/ring-0.17.14");
        for (arch, os, suffix) in [
            ("x86_64", "linux", "-elf.S"),
            ("aarch64", "linux", "-linux64.S"),
            ("x86_64", "macos", "-macosx.S"),
            ("aarch64", "macos", "-ios64.S"),
            ("x86_64", "windows", "-nasm.o"),
            ("aarch64", "windows", "-win64.S"),
        ] {
            let sources = ring_sources_for(arch, os);
            assert!(sources.iter().any(|s| s.ends_with(suffix)));
            for source in sources {
                let path = root.join(&source);
                assert!(path.is_file(), "missing {arch}/{os}: {}", path.display());
                if source.starts_with("pregenerated/") {
                    assert!(source.ends_with(suffix), "wrong ABI: {arch}/{os}: {source}");
                    if os == "windows" && arch == "x86_64" {
                        let bytes = std::fs::read(path).unwrap();
                        assert_eq!(&bytes[..2], &[0x64, 0x86], "expected AMD64 COFF");
                    }
                }
                if os == "windows" {
                    assert!(!source.starts_with("third_party/fiat/asm/"));
                }
            }
        }
    }
}
