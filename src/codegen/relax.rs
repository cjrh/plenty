//! Direct addresses for data defined in the program's own object.
//!
//! With `is_pic`, Cranelift's x86-64 backend loads every symbol address from
//! the GOT (`mov sym@GOTPCREL(%rip), %reg`), even a symbol marked `colocated`.
//! The object emits plain `R_X86_64_GOTPCREL`, which linkers do not relax, so
//! the load and the GOT entry survive into the executable (issue #372). Data
//! the IR marks colocated is a local definition in the same object
//! ([`super::symbols`]), so the instruction is rewritten here to
//! `lea sym(%rip), %reg` with a PC-relative relocation: the rewrite a linker
//! applies to `R_X86_64_REX_GOTPCRELX`.
//!
//! The rewrite depends only on the code and the function's stencil, so the
//! compiled-function cache key, which covers the stencil, still identifies
//! the result.
use cranelift_codegen::MachReloc;
use cranelift_codegen::RelocTarget;
use cranelift_codegen::binemit::Reloc;
use cranelift_codegen::ir::{ExternalName, Function, GlobalValueData};

/// `REX.W`, with any `REX.R` for the destination register.
const REX_W: u8 = 0x48;
const MOV_LOAD: u8 = 0x8B;
const LEA: u8 = 0x8D;

/// Rewrite each GOT load of a colocated data symbol in `bytes`, compiled from
/// `func`, into a PC-relative address computation.
pub(crate) fn relax_local_addresses(func: &Function, bytes: &mut [u8], relocs: &mut [MachReloc]) {
    let local: Vec<&ExternalName> = func
        .global_values
        .values()
        .filter_map(|data| match data {
            GlobalValueData::Symbol {
                name,
                colocated: true,
                tls: false,
                ..
            } => Some(name),
            _ => None,
        })
        .collect();
    if local.is_empty() {
        return;
    }
    for reloc in relocs {
        if reloc.kind != Reloc::X86GOTPCRel4 {
            continue;
        }
        let RelocTarget::ExternalName(name) = &reloc.target else {
            continue;
        };
        if !local.contains(&name) {
            continue;
        }
        // `movq disp32(%rip), %reg`: REX, opcode, ModRM (mod 00, r/m 101),
        // then the displacement the relocation patches.
        let at = reloc.offset as usize;
        let Some([rex, opcode, modrm]) = at.checked_sub(3).and_then(|start| bytes.get(start..at))
        else {
            continue;
        };
        let (rex, opcode, modrm) = (*rex, *opcode, *modrm);
        if rex & !0x04 != REX_W || opcode != MOV_LOAD || modrm & 0xC7 != 0x05 {
            debug_assert!(false, "unexpected GOT load encoding at {at}");
            continue;
        }
        bytes[at - 2] = LEA;
        // Both relocations compute `target + addend - place`; GOTPCREL's
        // target is the GOT entry, PC32's the symbol itself.
        reloc.kind = Reloc::X86PCRel4;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::ir::{UserExternalName, immediates::Imm64};

    fn symbol(func: &mut Function, index: u32, colocated: bool) -> ExternalName {
        let name = ExternalName::user(
            func.declare_imported_user_function(UserExternalName::new(1, index)),
        );
        func.create_global_value(GlobalValueData::Symbol {
            name: name.clone(),
            offset: Imm64::new(0),
            colocated,
            tls: false,
        });
        name
    }

    fn got_load(offset: u32, name: ExternalName) -> MachReloc {
        MachReloc {
            offset,
            kind: Reloc::X86GOTPCRel4,
            target: RelocTarget::ExternalName(name),
            addend: -4,
        }
    }

    #[test]
    fn rewrites_only_got_loads_of_colocated_data() {
        let mut func = Function::new();
        let local = symbol(&mut func, 0, true);
        let shared = symbol(&mut func, 1, false);
        // movq local(%rip), %r8; movq shared(%rip), %rdi
        let mut bytes = vec![
            0x4C, 0x8B, 0x05, 0, 0, 0, 0, //
            0x48, 0x8B, 0x3D, 0, 0, 0, 0,
        ];
        let mut relocs = vec![got_load(3, local), got_load(10, shared)];
        relax_local_addresses(&func, &mut bytes, &mut relocs);
        assert_eq!(&bytes[..3], [0x4C, LEA, 0x05]);
        assert_eq!(relocs[0].kind, Reloc::X86PCRel4);
        assert_eq!(relocs[0].addend, -4);
        assert_eq!(&bytes[7..10], [0x48, MOV_LOAD, 0x3D]);
        assert_eq!(relocs[1].kind, Reloc::X86GOTPCRel4);
    }
    #[test]
    fn leaves_arm_relocations_and_words_unchanged() {
        let mut function = Function::new();
        let local = symbol(&mut function, 0, true);
        let mut bytes = [0x90000000_u32, 0xf9400000, 0x94000000]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let original = bytes.clone();
        let kinds = [
            Reloc::Aarch64AdrGotPage21,
            Reloc::Aarch64Ld64GotLo12Nc,
            Reloc::Arm64Call,
        ];
        let mut relocs = kinds
            .iter()
            .enumerate()
            .map(|(i, kind)| MachReloc {
                offset: (i * 4) as u32,
                kind: *kind,
                target: RelocTarget::ExternalName(local.clone()),
                addend: 0,
            })
            .collect::<Vec<_>>();
        relax_local_addresses(&function, &mut bytes, &mut relocs);
        assert_eq!(bytes, original);
        assert_eq!(relocs.iter().map(|r| r.kind).collect::<Vec<_>>(), kinds);
    }
}
