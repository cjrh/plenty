//! A native caller can keep an executable adapter live even if Plenty main never calls it.
use super::CompileOptions;
use crate::hash::HashSet;
use object::{Object, ObjectSection, ObjectSymbol};
use std::path::Path;

#[derive(Default)]
pub(super) struct EngineRoots {
    restrict_globals: bool,
    pub(super) native_imports: HashSet<String>,
}

impl EngineRoots {
    pub(super) fn for_link(options: &CompileOptions, format: object::BinaryFormat) -> Self {
        let mut roots = Self::default();
        if !cfg!(target_os = "linux")
            || format != object::BinaryFormat::Elf
            || options.executable_exports.is_none()
        {
            return roots;
        }
        roots.restrict_globals = options.linker == Path::new("cc");
        for argument in &options.link_args {
            // An archive, script, driver flag, etc. can introduce other roots.
            // Still collect direct engine imports from any ordinary objects.
            if argument.as_encoded_bytes().starts_with(b"-")
                || argument.as_encoded_bytes().starts_with(b"@")
                || !roots.read_native_object(Path::new(argument))
            {
                roots.restrict_globals = false;
            }
        }
        roots
    }

    pub(super) fn includes_global(&self, name: object::Result<&str>) -> bool {
        !self.restrict_globals
            || name.is_ok_and(|name| name == "plenty_main" || self.native_imports.contains(name))
    }

    fn read_native_object(&mut self, path: &Path) -> bool {
        if path.extension().is_none_or(|ext| ext != "o") || !path.is_file() {
            return false;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        let Ok(file) = object::File::parse(bytes.as_slice()) else {
            return false;
        };
        if file.format() != object::BinaryFormat::Elf
            || file.kind() != object::ObjectKind::Relocatable
            || file.symbol_table().is_none()
        {
            return false;
        }
        // Plugin payloads and embedded linker options need not describe all
        // references in the ELF symbol table. Do not infer absent roots there.
        let mut plain = !file.sections().any(|section| {
            section.name().is_ok_and(|name| {
                name.starts_with(".gnu.lto_")
                    || matches!(
                        name,
                        ".llvmbc" | ".llvm.lto" | ".linker-options" | ".deplibs"
                    )
            })
        });
        for symbol in file.symbols().filter(|symbol| symbol.is_undefined()) {
            let Ok(name) = symbol.name() else {
                return false;
            };
            // Versioned references can resolve to a different spelling at link time.
            let name = name.split_once('@').map_or(name, |(base, _)| {
                plain = false;
                base
            });
            self.native_imports.insert(name.to_owned());
        }
        plain
    }
}
