//! The startup CPU check of programs compiled above the baseline (`--cpu`,
//! issue #962). The compiler's `plenty_main` trampoline, which is baseline
//! code, calls [`plenty_rt_require_cpu`] before the program's own entry. This
//! module is compiled at the runtime's baseline, so the check itself uses no
//! instruction it is checking for. See
//! book/src/design/toolchain/01-building/linking.md#cpu-levels.

/// The CPUID and XCR0 words the check reads. Missing leaves read as zero.
#[derive(Clone, Copy, Default)]
pub(crate) struct Features {
    /// Leaf 1, ECX.
    pub leaf1_ecx: u32,
    /// Leaf 7 subleaf 0, EBX.
    pub leaf7_ebx: u32,
    /// Leaf 0x8000_0001, ECX.
    pub extended1_ecx: u32,
    /// XCR0, read only when the OS enabled XSAVE (OSXSAVE).
    pub xcr0: u64,
}

#[derive(Clone, Copy)]
enum Word {
    Leaf1Ecx,
    Leaf7Ebx,
    Extended1Ecx,
    Xcr0,
}

/// `(level, feature, word, bit)`: each level requires its own rows and those
/// of every lower level. These are the psABI's x86-64-v2 and x86-64-v3 sets,
/// plus the OS state AVX needs: OSXSAVE, and XCR0 enabling SSE and AVX state.
const REQUIREMENTS: [(i64, &str, Word, u32); 18] = [
    (2, "sse3", Word::Leaf1Ecx, 0),
    (2, "ssse3", Word::Leaf1Ecx, 9),
    (2, "cmpxchg16b", Word::Leaf1Ecx, 13),
    (2, "sse4.1", Word::Leaf1Ecx, 19),
    (2, "sse4.2", Word::Leaf1Ecx, 20),
    (2, "popcnt", Word::Leaf1Ecx, 23),
    (2, "lahf", Word::Extended1Ecx, 0),
    (3, "fma", Word::Leaf1Ecx, 12),
    (3, "movbe", Word::Leaf1Ecx, 22),
    (3, "osxsave", Word::Leaf1Ecx, 27),
    (3, "avx", Word::Leaf1Ecx, 28),
    (3, "f16c", Word::Leaf1Ecx, 29),
    (3, "bmi1", Word::Leaf7Ebx, 3),
    (3, "avx2", Word::Leaf7Ebx, 5),
    (3, "bmi2", Word::Leaf7Ebx, 8),
    (3, "lzcnt", Word::Extended1Ecx, 5),
    (3, "os sse state", Word::Xcr0, 1),
    (3, "os avx state", Word::Xcr0, 2),
];

/// Level names, indexed by the compiler's level code.
fn level_name(level: i64) -> &'static str {
    match level {
        2 => "x86-64-v2",
        3 => "x86-64-v3",
        _ => "unknown",
    }
}

/// The features `level` requires that `features` lacks, in table order. An
/// unknown level lacks everything, so a mismatched compiler cannot pass.
pub(crate) fn missing(level: i64, features: &Features) -> impl Iterator<Item = &'static str> + '_ {
    let known = matches!(level, 2 | 3);
    REQUIREMENTS
        .iter()
        .filter(move |(at, _, word, bit)| {
            let value = match word {
                Word::Leaf1Ecx => u64::from(features.leaf1_ecx),
                Word::Leaf7Ebx => u64::from(features.leaf7_ebx),
                Word::Extended1Ecx => u64::from(features.extended1_ecx),
                Word::Xcr0 => features.xcr0,
            };
            !known || (*at <= level && value & (1 << bit) == 0)
        })
        .map(|(_, name, ..)| *name)
}

/// The one-line failure message, or `None` when the CPU has the level.
pub(crate) fn failure(level: i64, features: &Features) -> Option<String> {
    let lacking: Vec<&str> = missing(level, features).collect();
    if lacking.is_empty() {
        return None;
    }
    Some(format!(
        "error: this program requires an {} CPU (missing: {})",
        level_name(level),
        lacking.join(", ")
    ))
}

/// Read the running CPU's features.
#[cfg(target_arch = "x86_64")]
fn read() -> Features {
    use core::arch::x86_64::__cpuid_count;
    // SAFETY: CPUID exists on every x86-64 CPU; each leaf is read only when
    // the maximum-leaf query says it exists.
    #[allow(unused_unsafe)]
    let (basic, extended) = unsafe { (__cpuid_count(0, 0).eax, __cpuid_count(0x8000_0000, 0).eax) };
    let mut features = Features::default();
    #[allow(unused_unsafe)]
    unsafe {
        if basic >= 1 {
            features.leaf1_ecx = __cpuid_count(1, 0).ecx;
        }
        if basic >= 7 {
            features.leaf7_ebx = __cpuid_count(7, 0).ebx;
        }
        if extended >= 0x8000_0001 {
            features.extended1_ecx = __cpuid_count(0x8000_0001, 0).ecx;
        }
    }
    if features.leaf1_ecx & (1 << 27) != 0 {
        let (low, high): (u32, u32);
        // SAFETY: OSXSAVE says the OS enabled XGETBV; XCR0 is register 0.
        unsafe {
            core::arch::asm!(
                "xgetbv",
                in("ecx") 0,
                out("eax") low,
                out("edx") high,
                options(nomem, nostack, preserves_flags),
            );
        }
        features.xcr0 = u64::from(high) << 32 | u64::from(low);
    }
    features
}

/// Exit with one line on stderr unless the running CPU has CPU level `level`
/// (the compiler's code: 2 for x86-64-v2, 3 for x86-64-v3). Exits rather than
/// trapping or aborting: the program has not started, so there is nothing to
/// clean up and no core dump to explain.
#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
pub extern "C" fn plenty_rt_require_cpu(level: i64) {
    if let Some(message) = failure(level, &read()) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every v3 bit set, with XCR0 enabling SSE and AVX state.
    fn v3() -> Features {
        let ecx1 = [0, 9, 12, 13, 19, 20, 22, 23, 27, 28, 29]
            .iter()
            .fold(0, |bits, bit| bits | 1 << bit);
        Features {
            leaf1_ecx: ecx1,
            leaf7_ebx: 1 << 3 | 1 << 5 | 1 << 8,
            extended1_ecx: 1 | 1 << 5,
            xcr0: 0b111,
        }
    }

    #[test]
    fn complete_features_pass_every_level() {
        assert!(failure(2, &v3()).is_none());
        assert!(failure(3, &v3()).is_none());
    }

    #[test]
    fn each_missing_feature_fails_its_level() {
        for (level, name, word, bit) in &REQUIREMENTS {
            let mut features = v3();
            match word {
                Word::Leaf1Ecx => features.leaf1_ecx &= !(1 << bit),
                Word::Leaf7Ebx => features.leaf7_ebx &= !(1 << bit),
                Word::Extended1Ecx => features.extended1_ecx &= !(1 << bit),
                Word::Xcr0 => features.xcr0 &= !(1 << bit),
            }
            let lacking: Vec<_> = missing(3, &features).collect();
            assert_eq!(lacking, [*name], "level 3 without {name}");
            assert_eq!(
                missing(2, &features).count(),
                usize::from(*level == 2),
                "{name}"
            );
        }
    }

    #[test]
    fn failure_is_one_line_naming_the_level() {
        let sse2_only = Features::default();
        let message = failure(3, &sse2_only).unwrap();
        assert!(!message.contains('\n'));
        assert!(
            message.starts_with("error: this program requires an x86-64-v3 CPU (missing: sse3, ")
        );
        assert!(message.ends_with("os avx state)"), "{message}");
        let message = failure(2, &sse2_only).unwrap();
        assert_eq!(
            message,
            "error: this program requires an x86-64-v2 CPU (missing: sse3, ssse3, cmpxchg16b, sse4.1, sse4.2, popcnt, lahf)"
        );
        // An AVX2 CPU whose OS does not save AVX state cannot run v3 code.
        let mut no_os_avx = v3();
        no_os_avx.xcr0 = 0b011;
        assert_eq!(missing(3, &no_os_avx).collect::<Vec<_>>(), ["os avx state"]);
        assert!(missing(2, &no_os_avx).next().is_none());
    }

    #[test]
    fn unknown_levels_never_pass() {
        for level in [0, 1, 4, -1] {
            assert!(failure(level, &v3()).is_some(), "{level}");
        }
    }

    /// The machine running the tests passes whatever level its features
    /// give, and the reader agrees with std's detection.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn reader_agrees_with_std_detection() {
        let features = read();
        let v3 = [
            "avx", "avx2", "fma", "bmi1", "bmi2", "lzcnt", "movbe", "f16c",
        ]
        .iter()
        .all(|name| match *name {
            "avx" => std::is_x86_feature_detected!("avx"),
            "avx2" => std::is_x86_feature_detected!("avx2"),
            "fma" => std::is_x86_feature_detected!("fma"),
            "bmi1" => std::is_x86_feature_detected!("bmi1"),
            "bmi2" => std::is_x86_feature_detected!("bmi2"),
            "lzcnt" => std::is_x86_feature_detected!("lzcnt"),
            "movbe" => std::is_x86_feature_detected!("movbe"),
            _ => std::is_x86_feature_detected!("f16c"),
        });
        assert_eq!(failure(3, &features).is_none(), v3);
    }
}
