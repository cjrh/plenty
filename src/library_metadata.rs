//! Read embedded source contracts as bounded file data, without loading code.
use object::{Object, ObjectSection, ObjectSymbol};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const MAX_BINARY: u64 = 512 * 1024 * 1024;
const MAX_CONTRACT: usize = 4 * 1024 * 1024;
const MAX_CONTRACTS: usize = 256;
const MAX_TOTAL: usize = 16 * 1024 * 1024;

pub(crate) fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryInterface {
    pub name: String,
    pub target: String,
    /// Exact UTF-8 bytes embedded in the binary, including the version header.
    pub source: String,
    /// Absent on older format-1 interfaces that predate compatibility guards.
    pub fingerprint: Option<String>,
}

impl LibraryInterface {
    fn parse(section_name: &str, data: &[u8]) -> Result<Self> {
        if data.len() > MAX_CONTRACT {
            return Err("embedded interface exceeds the 4 MiB limit".into());
        }
        let source = std::str::from_utf8(data)?;
        if source.contains('\0') {
            return Err("embedded interface contains a NUL byte".into());
        }
        let mut lines = source.lines();
        if lines.next() != Some("# plenty-interface-format: 1") {
            return Err("unsupported embedded interface format".into());
        }
        let name = lines
            .next()
            .and_then(|s| s.strip_prefix("# library: "))
            .ok_or("missing embedded library name")?;
        crate::exports::check_library_name(name)?;
        if name != section_name {
            return Err("embedded library name disagrees with its section name".into());
        }
        let target = lines
            .next()
            .and_then(|s| s.strip_prefix("# target: "))
            .ok_or("missing embedded target")?;
        crate::validate_target(Some(target))?;
        if lines.next() != Some("# abi: C") {
            return Err("unsupported embedded interface ABI".into());
        }
        let fingerprint = if let Some((contract, footer)) =
            source.rsplit_once("# interface-sha256: ")
        {
            let mut footer = footer.lines();
            let hash = footer.next().ok_or("missing interface fingerprint")?;
            if !contract.ends_with('\n') || hash != fingerprint(contract.as_bytes()) {
                return Err("embedded interface fingerprint mismatch".into());
            }
            let guard = format!("extern def _plenty_require_contract() -> () = \"{name}_plenty_contract_v1_{hash}\"");
            if footer.next() != Some(guard.as_str()) || footer.next().is_some() {
                return Err("malformed interface compatibility guard".into());
            }
            Some(hash.into())
        } else {
            None
        };
        Ok(Self {
            name: name.into(),
            target: target.into(),
            source: source.into(),
            fingerprint,
        })
    }
}

/// Extract all Plenty interfaces from an ELF object/shared object/executable or
/// a regular static archive. Never executes the input or follows archive paths.
pub fn read_library_interfaces(path: &Path) -> Result<Vec<LibraryInterface>> {
    let data = read_bounded(path, MAX_BINARY)?;
    inspect(&data)
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err(format!(
            "{} exceeds the {limit}-byte inspection limit",
            path.display()
        )
        .into());
    }
    let mut data = Vec::new();
    file.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err(format!(
            "{} exceeds the {limit}-byte inspection limit",
            path.display()
        )
        .into());
    }
    Ok(data)
}

fn inspect(data: &[u8]) -> Result<Vec<LibraryInterface>> {
    let mut interfaces = BTreeMap::new();
    let mut total = 0;
    if data.starts_with(&object::archive::MAGIC) || data.starts_with(&object::archive::THIN_MAGIC) {
        let archive = object::read::archive::ArchiveFile::parse(data)?;
        if archive.is_thin() {
            return Err("thin archives are not supported for interface inspection".into());
        }
        for member in archive.members() {
            let member = member?;
            let bytes = member.data(data)?;
            if bytes.starts_with(b"\x7fELF") {
                read_object(bytes, &mut interfaces, &mut total)?;
            }
        }
    } else {
        read_object(data, &mut interfaces, &mut total)?;
    }
    if interfaces.is_empty() {
        return Err("no embedded Plenty interfaces found".into());
    }
    Ok(interfaces.into_values().collect())
}

/// Verify an exact generated interface and its defined compatibility symbol
/// before invoking a linker. Reads each input once and never executes code.
pub fn verify_library_interface(binary: &Path, interface: &Path) -> Result<()> {
    let source = read_bounded(interface, MAX_CONTRACT as u64)?;
    let name = std::str::from_utf8(&source)?
        .lines()
        .nth(1)
        .and_then(|s| s.strip_prefix("# library: "))
        .ok_or("missing interface library name")?;
    let expected = LibraryInterface::parse(name, &source)?;
    let hash = expected
        .fingerprint
        .as_ref()
        .ok_or("interface has no compatibility fingerprint; regenerate it")?;
    let data = read_bounded(binary, MAX_BINARY)?;
    let interfaces = inspect(&data)?;
    let actual = interfaces
        .iter()
        .find(|i| i.name == name)
        .ok_or_else(|| format!("binary has no interface for library `{name}`"))?;
    if actual.source != expected.source {
        return Err(format!(
            "incompatible interface for `{name}`: expected {hash}, binary has {}",
            actual.fingerprint.as_deref().unwrap_or("no fingerprint")
        )
        .into());
    }
    let symbol = format!("{name}_plenty_contract_v1_{hash}");
    let defined = if data.starts_with(&object::archive::MAGIC) {
        let archive = object::read::archive::ArchiveFile::parse(data.as_slice())?;
        let mut found = false;
        for member in archive.members() {
            let member = member?;
            let bytes = member.data(data.as_slice())?;
            if bytes.starts_with(b"\x7fELF") && defines_symbol(bytes, &symbol)? {
                found = true;
            }
        }
        found
    } else {
        defines_symbol(&data, &symbol)?
    };
    if !defined {
        return Err(format!(
            "binary metadata matches but compatibility symbol `{symbol}` is not defined/exported"
        )
        .into());
    }
    Ok(())
}

fn defines_symbol(data: &[u8], name: &str) -> Result<bool> {
    let file = object::File::parse(data)?;
    let valid = |symbol: object::Symbol<'_, '_>| {
        !symbol.is_undefined()
            && symbol.is_global()
            && symbol.kind() == object::SymbolKind::Text
            && symbol.name().ok() == Some(name)
    };
    Ok(if file.kind() == object::ObjectKind::Dynamic {
        file.dynamic_symbols().any(valid)
    } else {
        file.symbols().any(valid)
    })
}

fn read_object(
    data: &[u8],
    interfaces: &mut BTreeMap<String, LibraryInterface>,
    total: &mut usize,
) -> Result<()> {
    let object = object::File::parse(data)?;
    if object.format() != object::BinaryFormat::Elf
        || object.architecture() != object::Architecture::X86_64
        || !object.is_little_endian()
    {
        return Err("interface inspection requires an x86_64 little-endian ELF object".into());
    }
    for section in object.sections() {
        if let Some(name) = section.name()?.strip_prefix(".plenty.interface.") {
            if interfaces.len() >= MAX_CONTRACTS || section.size() > MAX_CONTRACT as u64 {
                return Err("embedded interface count or size exceeds inspection limits".into());
            }
            if matches!(section.flags(), object::SectionFlags::Elf { sh_flags } if sh_flags & u64::from(object::elf::SHF_COMPRESSED) != 0)
            {
                return Err("compressed interface sections are not supported".into());
            }
            let bytes = section.data()?;
            *total += bytes.len();
            if *total > MAX_TOTAL {
                return Err("embedded interfaces exceed the 16 MiB total limit".into());
            }
            let interface = LibraryInterface::parse(name, bytes)?;
            if interfaces
                .insert(interface.name.clone(), interface)
                .is_some()
            {
                return Err(format!("duplicate embedded library interface `{name}`").into());
            }
        }
    }
    Ok(())
}

/// Write one named source interface only after validating all embedded metadata.
pub fn extract_library_interface(path: &Path, name: &str, output: &Path) -> Result<()> {
    let interfaces = read_library_interfaces(path)?;
    let interface = interfaces
        .iter()
        .find(|i| i.name == name)
        .ok_or_else(|| format!("library has no embedded interface `{name}`"))?;
    crate::library::publication::validate(&[output], &[path.canonicalize()?])?;
    let directory = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let workspace = tempfile::Builder::new()
        .prefix(".plenty-extract-")
        .tempdir_in(directory)?;
    let staged = workspace.path().join("interface");
    std::fs::write(&staged, &interface.source)?;
    crate::library::publication::publish(workspace, &[(staged, output.to_owned())])
}
