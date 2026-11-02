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

/// A new inode avoids stale vnode code-signature caches and leaves existing
/// mappings on the old signed file. Never truncate a previously loaded dylib.
pub(super) fn install_engine(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    let directory = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let staged = tempfile::NamedTempFile::new_in(directory)?;
    std::fs::copy(source, staged.path())?;
    staged.persist(destination).map_err(|error| error.error)?;
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

    #[cfg(unix)]
    #[test]
    fn engine_replacement_preserves_old_file_handles_and_hard_links() {
        use std::io::Read;
        use std::os::unix::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("selected.dylib");
        let destination = root.path().join(LIBRARY);
        std::fs::write(&source, b"first signed engine").unwrap();
        install_engine(&source, &destination).unwrap();
        let mut previous = std::fs::File::open(&destination).unwrap();
        let inode = previous.metadata().unwrap().ino();
        let link = root.path().join("running-engine.dylib");
        std::fs::hard_link(&destination, &link).unwrap();
        std::fs::write(&source, b"replacement signed engine").unwrap();
        install_engine(&source, &destination).unwrap();
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"replacement signed engine"
        );
        assert_ne!(std::fs::metadata(&destination).unwrap().ino(), inode);
        let mut bytes = Vec::new();
        previous.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"first signed engine");
        assert_eq!(std::fs::read(&link).unwrap(), b"first signed engine");
    }

    #[test]
    fn failed_engine_copy_or_publication_preserves_existing_output() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("selected.dylib");
        let destination = root.path().join(LIBRARY);
        std::fs::write(&destination, b"old signed engine").unwrap();
        assert!(install_engine(&source, &destination).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"old signed engine");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        std::fs::write(&source, b"new signed engine").unwrap();
        let directory = root.path().join("occupied");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("keep"), b"existing output").unwrap();
        assert!(install_engine(&source, &directory).is_err());
        assert_eq!(
            std::fs::read(directory.join("keep")).unwrap(),
            b"existing output"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    }
}
