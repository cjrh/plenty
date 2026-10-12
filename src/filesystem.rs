//! Narrow native primitives; public path/metadata/walking policy lives in std.
use crate::{op::Ty, sum};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FsOp {
    Open,
    Next,
    Close,
    Closed,
    Name,
    Path,
    FileType,
    EntryMetadata,
    Metadata,
    MetadataField,
    ReadLink,
    Join,
    Lexical,
    Components,
    Display,
}

impl FsOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__fs_open" => Self::Open,
            "__fs_next" => Self::Next,
            "__fs_close" => Self::Close,
            "__fs_closed" => Self::Closed,
            "__fs_name" => Self::Name,
            "__fs_path" => Self::Path,
            "__fs_type" => Self::FileType,
            "__fs_entry_metadata" => Self::EntryMetadata,
            "__fs_metadata" => Self::Metadata,
            "__fs_metadata_field" => Self::MetadataField,
            "__fs_readlink" => Self::ReadLink,
            "__path_join" => Self::Join,
            "__path_lexical" => Self::Lexical,
            "__path_components" => Self::Components,
            "__path_display" => Self::Display,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let bytes = || Ty::List(Rc::new(Ty::U8));
        let (inputs, output) = match self {
            Self::Open => (vec![bytes()], Ty::FsReadDir),
            Self::Next => (vec![Ty::FsReadDir], sum::option(Ty::FsDirEntry)),
            Self::Close => (vec![Ty::FsReadDir], Ty::Unit),
            Self::Closed => return (vec![Ty::FsReadDir], Ty::Bool),
            Self::Name | Self::Path => (vec![Ty::FsDirEntry], bytes()),
            Self::FileType => (vec![Ty::FsDirEntry], Ty::U8),
            Self::EntryMetadata => (vec![Ty::FsDirEntry], Ty::FsMetadata),
            Self::Metadata => (vec![bytes(), Ty::Bool], Ty::FsMetadata),
            Self::MetadataField => return (vec![Ty::FsMetadata, Ty::U8], Ty::U64),
            Self::ReadLink => (vec![bytes()], bytes()),
            Self::Join => (vec![bytes(), bytes()], bytes()),
            Self::Lexical => (vec![bytes(), Ty::U8], sum::option(bytes())),
            Self::Components => (vec![bytes()], Ty::List(Rc::new(bytes()))),
            Self::Display => (vec![bytes()], Ty::Str),
        };
        (inputs, sum::result(output, sum::io_error()))
    }

    pub fn mutable(self) -> bool {
        matches!(self, Self::Next | Self::Close)
    }

    pub fn opcode(self) -> i64 {
        150 + self as i64
    }
}
