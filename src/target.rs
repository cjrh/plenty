//! Immutable facts for the admitted native targets, shared by parsing,
//! code generation and object inspection. This describes support; recognizing
//! an OS in `if const` does not admit it as a compilation target.
use cranelift_codegen::isa::CallConv;

/// CPU features are never inferred from the build host: x86 uses SSE2;
/// ARM uses generic ARMv8-A. All admitted targets use 64-bit little-endian pointers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NativeTarget {
    pub triple: &'static str,
    pub arch: &'static str,
    pub os: &'static str,
    pub pointer_width: u8,
    pub endian: &'static str,
    pub object_arch: object::Architecture,
    pub object_format: object::BinaryFormat,
    pub baseline: &'static str,
    pub call_conv: CallConv,
}

pub(crate) const NATIVE_TARGETS: [NativeTarget; 4] = [
    NativeTarget {
        triple: "x86_64-unknown-linux-gnu",
        arch: "x86_64",
        os: "linux",
        pointer_width: 64,
        endian: "little",
        object_arch: object::Architecture::X86_64,
        object_format: object::BinaryFormat::Elf,
        baseline: "x86-64/SSE2",
        call_conv: CallConv::SystemV,
    },
    NativeTarget {
        triple: "aarch64-unknown-linux-gnu",
        arch: "aarch64",
        os: "linux",
        pointer_width: 64,
        endian: "little",
        object_arch: object::Architecture::Aarch64,
        object_format: object::BinaryFormat::Elf,
        baseline: "ARMv8-A",
        call_conv: CallConv::SystemV,
    },
    NativeTarget {
        triple: "aarch64-apple-darwin",
        arch: "aarch64",
        os: "macos",
        pointer_width: 64,
        endian: "little",
        object_arch: object::Architecture::Aarch64,
        object_format: object::BinaryFormat::MachO,
        baseline: "ARMv8-A",
        call_conv: CallConv::AppleAarch64,
    },
    NativeTarget {
        triple: "x86_64-pc-windows-msvc",
        arch: "x86_64",
        os: "windows",
        pointer_width: 64,
        endian: "little",
        object_arch: object::Architecture::X86_64,
        object_format: object::BinaryFormat::Coff,
        baseline: "x86-64/SSE2",
        call_conv: CallConv::WindowsFastcall,
    },
];

pub(crate) fn target_description(triple: &str) -> Option<&'static NativeTarget> {
    NATIVE_TARGETS.iter().find(|target| target.triple == triple)
}

/// Where the compiler executable runs: Cargo TARGET, not Cargo HOST (where
/// the build script runs). Building the compiler on another machine is valid.
pub(crate) fn compiler_host() -> &'static str {
    env!("PLENTY_COMPILER_HOST")
}

/// Target of the embedded runtime and generated programs. Code generation is
/// native; choosing a linker cannot select another architecture or OS.
pub fn native_target() -> &'static str {
    env!("PLENTY_RUNTIME_TARGET")
}

/// Reproducible CPU baseline used by this compiler's native code generator.
pub fn native_cpu_baseline() -> &'static str {
    target_description(native_target()).map_or("unknown", |target| target.baseline)
}

/// The CPU level an artifact is compiled for (`--cpu LEVEL`, issue #962).
/// Each level is a fixed feature set; the host CPU is never inspected, so the
/// same sources, level and compiler give the same object on every machine.
/// Programs above the baseline check the running CPU before any Plenty code
/// runs; see `book/src/design/toolchain/01-building/linking.md#cpu-levels`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CpuLevel {
    /// The target's baseline: SSE2 on x86-64, ARMv8-A on AArch64.
    #[default]
    Baseline,
    /// x86-64-v2: adds SSE3, SSSE3, SSE4.1, SSE4.2, POPCNT and CMPXCHG16B.
    X86_64V2,
    /// x86-64-v3: adds AVX, AVX2, FMA, BMI1, BMI2 and LZCNT to v2.
    X86_64V3,
}

impl CpuLevel {
    pub const NAMES: [&'static str; 3] = ["x86-64", "x86-64-v2", "x86-64-v3"];

    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "x86-64" => Ok(Self::Baseline),
            "x86-64-v2" => Ok(Self::X86_64V2),
            "x86-64-v3" => Ok(Self::X86_64V3),
            _ => Err(format!(
                "unknown CPU level `{text}`; valid levels: {}",
                Self::NAMES.join(", ")
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Baseline => "x86-64",
            Self::X86_64V2 => "x86-64-v2",
            Self::X86_64V3 => "x86-64-v3",
        }
    }

    /// The code the runtime's startup check receives; 0 means no check.
    pub(crate) fn code(self) -> i64 {
        match self {
            Self::Baseline => 0,
            Self::X86_64V2 => 2,
            Self::X86_64V3 => 3,
        }
    }

    /// Cranelift x86 ISA flags this level enables. Set explicitly: the fork's
    /// `x86-64-v3` preset omits `has_avx`. Cranelift has no flags for MOVBE,
    /// F16C or LAHF, so it never emits them; the startup check still requires
    /// them, as the level's definition does.
    pub(crate) fn isa_flags(self) -> &'static [&'static str] {
        const V2: [&str; 6] = [
            "has_sse3",
            "has_ssse3",
            "has_sse41",
            "has_sse42",
            "has_popcnt",
            "has_cmpxchg16b",
        ];
        const V3: [&str; 12] = [
            "has_sse3",
            "has_ssse3",
            "has_sse41",
            "has_sse42",
            "has_popcnt",
            "has_cmpxchg16b",
            "has_avx",
            "has_avx2",
            "has_fma",
            "has_bmi1",
            "has_bmi2",
            "has_lzcnt",
        ];
        match self {
            Self::Baseline => &[],
            Self::X86_64V2 => &V2,
            Self::X86_64V3 => &V3,
        }
    }

    /// Reject a level the target cannot use. `x86-64` names the default level
    /// on every target, so a build setting can stay portable.
    pub fn validate_for(self, triple: &str) -> Result<(), String> {
        let arch = target_description(triple).map_or("unknown", |target| target.arch);
        if self != Self::Baseline && arch != "x86_64" {
            return Err(format!(
                "CPU level `{}` needs an x86-64 target, not `{triple}`",
                self.name()
            ));
        }
        Ok(())
    }

    /// The level as `--print-target` shows it: the target's baseline name at
    /// the default level.
    pub fn describe(self, triple: &str) -> &'static str {
        match self {
            Self::Baseline => {
                target_description(triple).map_or("unknown", |target| target.baseline)
            }
            _ => self.name(),
        }
    }
}

/// Convention used only at native C/Rust boundaries. Plenty calls retain Tail.
/// Cranelift calls both Linux C conventions SystemV, including AAPCS64.
pub(crate) fn native_call_conv() -> CallConv {
    target_description(native_target())
        .expect("supported packaged runtime target")
        .call_conv
}

/// Reject unsupported or mismatched targets before native lowering or linking.
pub fn validate_target(requested: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    validate_target_identity(
        requested.unwrap_or(native_target()),
        compiler_host(),
        native_target(),
    )
}

fn validate_target_identity(
    requested: &str,
    compiler: &str,
    packaged: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if target_description(requested).is_none() {
        return Err(format!("unsupported native target `{requested}`; supported targets: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, aarch64-apple-darwin, x86_64-pc-windows-msvc (64-bit little-endian)").into());
    }
    if target_description(compiler).is_none() {
        return Err(format!("unsupported compiler host `{compiler}`").into());
    }
    if target_description(packaged).is_none() {
        return Err(format!("unsupported packaged runtime target `{packaged}`").into());
    }
    if compiler != packaged {
        return Err(format!("compiler host `{compiler}` does not match packaged runtime `{packaged}`; native compilation requires matching targets").into());
    }
    if requested != packaged {
        return Err(format!("target `{requested}` does not match packaged runtime `{packaged}`; cross-compilation is not supported").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admitted_facts_match_target_triples() {
        for target in NATIVE_TARGETS {
            let triple: target_lexicon::Triple = target.triple.parse().unwrap();
            assert_eq!(triple.architecture.to_string(), target.arch);
            assert_eq!(
                triple.operating_system.to_string(),
                if target.os == "macos" {
                    "darwin"
                } else {
                    target.os
                }
            );
            assert_eq!(
                triple.binary_format.to_string(),
                match target.object_format {
                    object::BinaryFormat::Elf => "elf",
                    object::BinaryFormat::MachO => "macho",
                    object::BinaryFormat::Coff => "coff",
                    _ => unreachable!(),
                }
            );
            assert_eq!(triple.pointer_width().unwrap().bits(), target.pointer_width);
            assert_eq!(
                triple.endianness().unwrap(),
                target_lexicon::Endianness::Little
            );
            assert_eq!(target.endian, "little");
            assert_eq!(CallConv::triple_default(&triple), target.call_conv);
            assert_eq!(
                target.object_arch,
                match target.arch {
                    "x86_64" => object::Architecture::X86_64,
                    "aarch64" => object::Architecture::Aarch64,
                    _ => panic!("unexpected admitted architecture"),
                }
            );
            assert!(validate_target_identity(target.triple, target.triple, target.triple).is_ok());
        }
        assert_eq!(NATIVE_TARGETS.len(), 4);
        assert_eq!(compiler_host(), native_target());
        validate_target(None).unwrap();
    }

    #[test]
    fn cpu_levels_parse_name_and_validate() {
        for name in CpuLevel::NAMES {
            assert_eq!(CpuLevel::parse(name).unwrap().name(), name);
        }
        assert_eq!(CpuLevel::default(), CpuLevel::Baseline);
        for bad in ["native", "x86-64-v4", "X86-64-V3", "v3", ""] {
            let error = CpuLevel::parse(bad).unwrap_err();
            assert!(
                error.contains("valid levels: x86-64, x86-64-v2, x86-64-v3"),
                "{error}"
            );
        }
        let x86 = NATIVE_TARGETS[0].triple;
        let arm = NATIVE_TARGETS[1].triple;
        for level in [CpuLevel::X86_64V2, CpuLevel::X86_64V3] {
            level.validate_for(x86).unwrap();
            level.validate_for("x86_64-pc-windows-msvc").unwrap();
            assert!(
                level
                    .validate_for(arm)
                    .unwrap_err()
                    .contains("needs an x86-64 target")
            );
        }
        CpuLevel::Baseline.validate_for(arm).unwrap();
        assert_eq!(CpuLevel::Baseline.describe(x86), "x86-64/SSE2");
        assert_eq!(CpuLevel::Baseline.describe(arm), "ARMv8-A");
        assert_eq!(CpuLevel::X86_64V3.describe(x86), "x86-64-v3");
        let v2 = CpuLevel::X86_64V2.isa_flags();
        assert!(
            v2.iter()
                .all(|flag| CpuLevel::X86_64V3.isa_flags().contains(flag))
        );
    }

    #[test]
    fn rejects_requested_and_packaged_target_mismatches() {
        let x86 = NATIVE_TARGETS[0].triple;
        let arm = NATIVE_TARGETS[1].triple;
        assert!(
            validate_target_identity(arm, x86, x86)
                .unwrap_err()
                .to_string()
                .contains("cross-compilation is not supported")
        );
        // Even if requested output matches the package, the compiler must
        // execute on that target; a matching linker cannot fix this package.
        assert!(
            validate_target_identity(arm, x86, arm)
                .unwrap_err()
                .to_string()
                .contains("compiler host")
        );
        assert!(
            validate_target_identity(x86, arm, x86)
                .unwrap_err()
                .to_string()
                .contains("compiler host")
        );
    }

    #[test]
    fn rejects_unsupported_targets_in_every_identity() {
        let native = NATIVE_TARGETS[0].triple;
        for unsupported in [
            "i686-unknown-linux-gnu",
            "aarch64-pc-windows-msvc",
            "x86_64-apple-darwin",
            "not-a-target",
            "x86_64-unknown-linux-musl",
        ] {
            assert!(target_description(unsupported).is_none());
            assert!(
                validate_target_identity(unsupported, native, native)
                    .unwrap_err()
                    .to_string()
                    .contains("unsupported native target")
            );
            assert!(
                validate_target_identity(native, unsupported, native)
                    .unwrap_err()
                    .to_string()
                    .contains("unsupported compiler host")
            );
            assert!(
                validate_target_identity(native, native, unsupported)
                    .unwrap_err()
                    .to_string()
                    .contains("unsupported packaged runtime target")
            );
        }
    }
}
