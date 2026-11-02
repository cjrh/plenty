//! Mach-O distribution contracts, checked without invoking platform tools so
//! malformed identities and build-machine paths can be tested on any host.
use object::Object;
use object::read::macho::{LoadCommandVariant, MachOFile64};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn validate(bytes: &[u8], library: Option<&str>, engines: &[&str]) -> Result<()> {
    let image = MachOFile64::<object::Endianness>::parse(bytes)?;
    let expected = if library.is_some() {
        object::ObjectKind::Dynamic
    } else {
        object::ObjectKind::Executable
    };
    if image.architecture() != object::Architecture::Aarch64
        || image.kind() != expected
        || !image.is_little_endian()
    {
        return Err(
            "distribution requires an arm64 Mach-O dylib or executable of the expected kind".into(),
        );
    }
    let endian = image.endian();
    let mut identity = None;
    let mut local_lookup = false;
    let mut signed = false;
    let mut commands = image.macho_load_commands()?;
    while let Some(command) = commands.next()? {
        match command.variant()? {
            LoadCommandVariant::IdDylib(dylib) => {
                let name = command.string(endian, dylib.dylib.name)?;
                if identity.replace(name).is_some() {
                    return Err("duplicate Mach-O dylib identity".into());
                }
            }
            LoadCommandVariant::Dylib(dylib) => {
                let name = command.string(endian, dylib.dylib.name)?;
                if !name.starts_with(b"/usr/lib/")
                    && !name.starts_with(b"/System/Library/")
                    && !engines
                        .iter()
                        .any(|engine| name == format!("@rpath/{engine}").as_bytes())
                {
                    return Err(format!(
                        "nonportable Mach-O dependency: {}",
                        String::from_utf8_lossy(name)
                    )
                    .into());
                }
            }
            LoadCommandVariant::Rpath(rpath) => {
                if library.is_some()
                    || engines.is_empty()
                    || command.string(endian, rpath.path)? != b"@loader_path"
                {
                    return Err(
                        "Mach-O distribution permits only application-local @loader_path lookup"
                            .into(),
                    );
                }
                local_lookup = true;
            }
            LoadCommandVariant::LinkeditData(_)
                if command.cmd() == object::macho::LC_CODE_SIGNATURE =>
            {
                signed = true;
            }
            _ => {}
        }
    }
    if let Some(library) = library
        && identity != Some(format!("@rpath/{library}").as_bytes())
    {
        return Err(format!("engine install name must be @rpath/{library}").into());
    }
    if !engines.is_empty() && !local_lookup {
        return Err("Mach-O distribution omitted @loader_path lookup".into());
    }
    if !signed {
        return Err("arm64 Mach-O distribution needs a code signature".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &str = "libplenty_dataframe_v1.dylib";

    fn command(kind: u32, name: &str, header: usize) -> Vec<u8> {
        let mut bytes = vec![0; header];
        bytes[..4].copy_from_slice(&kind.to_le_bytes());
        bytes[8..12].copy_from_slice(&(header as u32).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        bytes.resize(bytes.len().next_multiple_of(8), 0);
        let size = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        bytes
    }

    fn image(
        library: Option<&str>,
        dependencies: &[&str],
        rpath: Option<&str>,
        signed: bool,
    ) -> Vec<u8> {
        let mut commands: Vec<_> = dependencies
            .iter()
            .map(|name| command(0xc, name, 24))
            .collect();
        if let Some(name) = library {
            commands.push(command(0xd, name, 24));
        }
        if let Some(path) = rpath {
            commands.push(command(0x8000001c, path, 12));
        }
        if signed {
            commands.push(
                [0x1du32, 16, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
            );
        }
        let mut header: Vec<_> = [
            0xfeedfacfu32,
            0x100000c,
            0,
            if library.is_some() { 6 } else { 2 },
            commands.len() as u32,
            commands.iter().map(|c| c.len() as u32).sum(),
            0,
            0,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        header.extend(commands.into_iter().flatten());
        header
    }

    #[test]
    fn versioned_dylib_identity_is_required() {
        let good = image(
            Some(&format!("@rpath/{LIBRARY}")),
            &["/usr/lib/libSystem.B.dylib"],
            None,
            true,
        );
        validate(&good, Some(LIBRARY), &[]).unwrap();
        let bad = image(Some("/build/libplenty_dataframe_v1.dylib"), &[], None, true);
        assert!(
            validate(&bad, Some(LIBRARY), &[])
                .unwrap_err()
                .to_string()
                .contains("install name")
        );
        assert!(validate(&good, None, &[]).is_err());
    }

    #[test]
    fn application_lookup_is_local_and_dependencies_are_bundled() {
        let dependency = format!("@rpath/{LIBRARY}");
        let good = image(
            None,
            &[&dependency, "/usr/lib/libSystem.B.dylib"],
            Some("@loader_path"),
            true,
        );
        validate(&good, None, &[LIBRARY]).unwrap();
        assert!(validate(&good, None, &[]).is_err());
        for path in [None, Some("/build/engine")] {
            assert!(validate(&image(None, &[&dependency], path, true), None, &[LIBRARY]).is_err());
        }
        assert!(
            validate(
                &image(
                    None,
                    &["/opt/homebrew/lib/libz.dylib"],
                    Some("@loader_path"),
                    true
                ),
                None,
                &[LIBRARY]
            )
            .is_err()
        );
    }

    #[test]
    fn architecture_and_signature_are_checked() {
        let mut bytes = image(None, &[], None, true);
        validate(&bytes, None, &[]).unwrap();
        bytes[4..8].copy_from_slice(&0x1000007u32.to_le_bytes());
        assert!(validate(&bytes, None, &[]).is_err());
        assert!(validate(&image(None, &[], None, false), None, &[]).is_err());
        assert!(validate(b"not Mach-O", None, &[]).is_err());
    }
}
