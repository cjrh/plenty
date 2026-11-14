//! Exact source identity, independent of declared names and import aliases.
use super::PackageSource;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PackageIdentity {
    Local(PathBuf),
    Script(PathBuf),
    Git {
        url: String,
        subdir: PathBuf,
        commit: String,
    },
}

/// Preserve native path components, including non-Unicode filenames.
fn relative(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for component in &to[common..] {
        path.push(component.as_os_str());
    }
    if path.as_os_str().is_empty() {
        path.push(".");
    }
    path
}
pub(super) fn path_fields(root: &Path, path: &Path) -> (bool, String) {
    let path = relative(root, path);
    if path.to_str().is_some() {
        return (
            false,
            path.components()
                .map(|c| c.as_os_str().to_str().unwrap())
                .collect::<Vec<_>>()
                .join("/"),
        );
    }
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(windows)]
    let bytes = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    #[cfg(not(any(unix, windows)))]
    let bytes = path.as_os_str().as_encoded_bytes().to_vec();
    (true, crate::hash_hex::encode(&bytes))
}
pub(super) fn decode_path(bytes: bool, text: &str) -> Option<PathBuf> {
    if !bytes {
        return Some(PathBuf::from(text));
    }
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = text
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                _ => None,
            };
            Some(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect();
    let bytes = bytes?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(std::ffi::OsString::from_vec(bytes).into())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        if !bytes.len().is_multiple_of(2) {
            return None;
        }
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        Some(std::ffi::OsString::from_wide(&units).into())
    }
    #[cfg(not(any(unix, windows)))]
    {
        String::from_utf8(bytes).ok().map(PathBuf::from)
    }
}
impl PackageIdentity {
    pub(super) fn new(directory: &Path, source: &PackageSource) -> Self {
        match source {
            PackageSource::Local => Self::Local(directory.to_path_buf()),
            PackageSource::Git {
                url,
                subdir,
                commit,
                ..
            } => Self::Git {
                url: url.clone(),
                subdir: subdir.clone().unwrap_or_default(),
                commit: commit.clone(),
            },
        }
    }
    /// Length-delimited Git fields and lossless relative paths are equality keys.
    pub fn serialized(&self, root: &Path) -> String {
        match self {
            Self::Local(path) | Self::Script(path) => {
                let kind = if matches!(self, Self::Local(_)) {
                    "path"
                } else {
                    "script"
                };
                let (bytes, path) = path_fields(root, path);
                format!("{kind}{}:{path}", if bytes { "-bytes" } else { "" })
            }
            Self::Git {
                url,
                subdir,
                commit,
            } => {
                let (bytes, subdir) = if subdir.as_os_str().is_empty() {
                    (false, String::new())
                } else {
                    path_fields(Path::new(""), subdir)
                };
                format!(
                    "git{}:{}:{url}:{}:{subdir}:{commit}",
                    if bytes { "-bytes" } else { "" },
                    url.len(),
                    subdir.len()
                )
            }
        }
    }
    pub(super) fn local_source(&self, root: &Path) -> String {
        let id = self.serialized(root);
        let (kind, path) = id.split_once(':').unwrap();
        format!("{kind}+{path}")
    }
}
