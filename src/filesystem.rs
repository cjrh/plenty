//! Narrow native primitives; public path/metadata/walking policy lives in std.
use crate::{op::Ty, sum};
use std::sync::Arc;

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
    Root,
    Child,
    ReadFile,
    MakeDir,
    RemoveFile,
    RemoveDir,
    Move,
    RootEntries,
    Anonymous,
    Publish,
    SameDir,
    DuplicateRoot,
    MakeTemporaryDir,
    FileMetadata,
    WatchOpen,
    WatchAdd,
    WatchRemove,
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
            "__fs_root" => Self::Root,
            "__fs_child" => Self::Child,
            "__fs_read_file" => Self::ReadFile,
            "__fs_make_dir" => Self::MakeDir,
            "__fs_remove_file" => Self::RemoveFile,
            "__fs_remove_dir" => Self::RemoveDir,
            "__fs_move" => Self::Move,
            "__fs_root_entries" => Self::RootEntries,
            "__fs_anonymous" => Self::Anonymous,
            "__fs_publish" => Self::Publish,
            "__fs_same_dir" => Self::SameDir,
            "__fs_duplicate_root" => Self::DuplicateRoot,
            "__fs_make_temporary_dir" => Self::MakeTemporaryDir,
            "__fs_file_metadata" => Self::FileMetadata,
            "__fs_watch_open" => Self::WatchOpen,
            "__fs_watch_add" => Self::WatchAdd,
            "__fs_watch_remove" => Self::WatchRemove,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let bytes = || Ty::List(Arc::new(Ty::U8));
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
            Self::Components => (vec![bytes()], Ty::List(Arc::new(bytes()))),
            Self::Display => (vec![bytes()], Ty::Str),
            Self::Root => (vec![bytes()], Ty::File),
            Self::Child | Self::ReadFile | Self::MakeTemporaryDir => {
                (vec![Ty::File, bytes()], Ty::File)
            }
            Self::MakeDir | Self::RemoveFile | Self::RemoveDir => {
                (vec![Ty::File, bytes()], Ty::Unit)
            }
            Self::Move => (vec![Ty::File, bytes(), bytes()], Ty::Unit),
            Self::RootEntries => (vec![Ty::File], Ty::FsReadDir),
            Self::Anonymous | Self::DuplicateRoot => (vec![Ty::File], Ty::File),
            Self::Publish => (vec![Ty::File, Ty::File, bytes()], Ty::Unit),
            Self::SameDir => (vec![Ty::File, Ty::File, bytes()], Ty::Bool),
            Self::FileMetadata => (vec![Ty::File], Ty::FsMetadata),
            Self::WatchOpen => (vec![], Ty::File),
            Self::WatchAdd => (vec![Ty::File, bytes()], Ty::I32),
            Self::WatchRemove => (vec![Ty::File, Ty::I32], Ty::Unit),
        };
        (inputs, sum::result(output, sum::io_error()))
    }

    pub fn mutable(self) -> bool {
        matches!(self, Self::Next | Self::Close)
    }

    pub fn opcode(self) -> i64 {
        match self as i64 {
            index @ 0..=14 => 150 + index,
            index @ 15..=28 => 260 + index - 15,
            index => 512 + index - 29,
        }
    }
}
