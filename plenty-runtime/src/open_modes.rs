//! The modes `open` accepts. The compiler includes this file for its check of
//! literal modes, so both sides accept exactly the same set.

/// Listed in diagnostics and in the description of `IoError.InvalidMode`.
pub(crate) const NAMES: &str = "r, w, a, x, r+, w+, a+, x+";

#[derive(Clone, Copy)]
pub(crate) enum OpenMode {
    Read,
    Replace,
    Append,
    CreateNew,
    ReadWrite,
    ReplaceRead,
    AppendRead,
    CreateNewRead,
}

impl OpenMode {
    pub(crate) fn parse(mode: &str) -> Option<Self> {
        Some(match mode {
            "r" => Self::Read,
            "w" => Self::Replace,
            "a" => Self::Append,
            "x" => Self::CreateNew,
            "r+" => Self::ReadWrite,
            "w+" => Self::ReplaceRead,
            "a+" => Self::AppendRead,
            "x+" => Self::CreateNewRead,
            _ => return None,
        })
    }
}
