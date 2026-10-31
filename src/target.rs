//! Immutable facts for the admitted native targets, shared by parsing,
//! code generation and object inspection. This describes support; recognizing
//! an OS in `if const` does not admit it as a compilation target.
use cranelift_codegen::isa::CallConv;

/// CPU features are never inferred from the build host: x86 uses SSE2;
/// ARM uses generic ARMv8-A. Only 64-bit little-endian GNU/Linux is admitted.
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

pub(crate) const NATIVE_TARGETS: [NativeTarget; 2] = [
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
        return Err(format!("unsupported native target `{requested}`; supported targets: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu (64-bit little-endian GNU/Linux)").into());
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
            assert_eq!(triple.operating_system.to_string(), target.os);
            assert_eq!(triple.binary_format.to_string(), "elf");
            assert_eq!(target.object_format, object::BinaryFormat::Elf);
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
        assert_eq!(NATIVE_TARGETS.len(), 2);
        assert_eq!(compiler_host(), native_target());
        validate_target(None).unwrap();
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
            "x86_64-pc-windows-msvc",
            "aarch64-apple-darwin",
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
