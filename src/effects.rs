//! The worker-thread effect contract of every native primitive.
//!
//! Worker eligibility is closed-world: `threading::check` walks each reachable
//! body and asks every native operation for its effect. The classification is
//! a match without a wildcard over the operations that reach the operating
//! system, so a new primitive cannot compile until it states its effect.
//! Imports grant nothing: a standard-library helper is certified by the same
//! walk as user code. See book/src/design/facilities/03-concurrency/effects.md.
use crate::collection::CollectionOp;
use crate::filesystem::FsOp;
use crate::process::ProcessOp;
use crate::stream::StreamOp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Memory and computation only.
    Compute,
    /// Reads a clock or blocks the calling thread for a bounded time.
    Clock,
    /// Reads the operating system's entropy source.
    Entropy,
    /// Standard streams, printing and arguments: shared, internally
    /// synchronized, with no ordering between threads.
    ProcessStream,
    /// Resolves a path in the shared filesystem namespace. A relative path
    /// resolves against the process working directory.
    Path,
    /// Acts through a handle the caller owns or borrows under the ordinary
    /// loan rules: files, directory roots and streams, child pipes.
    Handle,
    /// Reads environment variables or the working directory. Nothing in the
    /// language writes the environment.
    Environment,
    /// Spawns, waits for, or signals a child through the runtime's
    /// synchronized child table.
    Child,
    /// Changes state that every thread observes.
    ProcessGlobal,
}

impl Effect {
    /// Why a worker may not perform this effect, or `None` when it may.
    pub fn worker_denial(self) -> Option<&'static str> {
        match self {
            Self::ProcessGlobal => Some(
                "changing the working directory affects every thread's relative paths; change it on the main thread before starting workers",
            ),
            Self::Compute
            | Self::Clock
            | Self::Entropy
            | Self::ProcessStream
            | Self::Path
            | Self::Handle
            | Self::Environment
            | Self::Child => None,
        }
    }
}

impl FsOp {
    pub fn effect(self) -> Effect {
        match self {
            Self::Open | Self::EntryMetadata | Self::Metadata | Self::ReadLink | Self::Root => {
                Effect::Path
            }
            Self::Next
            | Self::Close
            | Self::Closed
            | Self::Child
            | Self::ReadFile
            | Self::MakeDir
            | Self::RemoveFile
            | Self::RemoveDir
            | Self::Move
            | Self::RootEntries
            | Self::Anonymous
            | Self::Publish
            | Self::SameDir
            | Self::DuplicateRoot
            | Self::MakeTemporaryDir => Effect::Handle,
            Self::Name
            | Self::Path
            | Self::FileType
            | Self::MetadataField
            | Self::Join
            | Self::Lexical
            | Self::Components
            | Self::Display => Effect::Compute,
        }
    }
}

impl StreamOp {
    pub fn effect(self) -> Effect {
        match self {
            Self::FileRead | Self::FileWrite | Self::FileSeek | Self::Wait => Effect::Handle,
            Self::StdinRead | Self::StdWrite => Effect::ProcessStream,
            Self::EntropyRead => Effect::Entropy,
        }
    }
}

impl ProcessOp {
    pub fn effect(self) -> Effect {
        match self {
            Self::EnvGet | Self::EnvVars | Self::CurrentDir => Effect::Environment,
            Self::ChangeDir => Effect::ProcessGlobal,
            Self::Spawn | Self::Pipe | Self::Wait | Self::Signal | Self::Poll => Effect::Child,
        }
    }
}

impl CollectionOp {
    pub fn effect(&self) -> Effect {
        use CollectionOp::*;
        match self {
            Filesystem(op) => op.effect(),
            Stream(op) => op.effect(),
            Process(op) => op.effect(),
            Time(_) => Effect::Clock,
            // These primitive families transform caller-owned memory only.
            Bytes(_) | Crypto(_) | Compress(_) | Unicode(_) => Effect::Compute,
            OpenFile | ReadBytes | WriteBytes | ReadText | WriteText | AppendText => Effect::Path,
            FileClose | FileClosed | FileRead | FileWrite | FileFlush | FileSync | FileReadLine
            | FileReadSized | FileReadLineSized | FileReadable | FileWritable | FileTell
            | FileSeek | FileTruncate | FileTruncateSized | FileReadLines | FileWriteLines => {
                Effect::Handle
            }
            WriteStdout | WriteStderr | FlushStdout | FlushStderr | Input | Args | ArgsBytes
            | InputBytes | WriteStdoutBytes | WriteStderrBytes | TryPrint(_) | TryPrintMany(_)
            | ReportEntryError(_) => Effect::ProcessStream,
            EncodeUtf8
            | DecodeUtf8
            | ParseNumber(_)
            | FormatScalar(_)
            | FormatValue(_)
            | ListUnpack(_)
            | ElementRef(..)
            | TryCopy(_)
            | Next(_)
            | TryNew(_)
            | Insert(_)
            | TryReserve(_)
            | TryInsert(_)
            | TryExtend(_)
            | DictTryUpdate(_)
            | SetTryUpdate(_)
            | SetIsSubset(_)
            | SetIsSuperset(_)
            | SetIsDisjoint(_)
            | SetTryUnion(_)
            | SetTryIntersection(_)
            | SetTryDifference(_)
            | SetTrySymmetricDifference(_)
            | SetIntersectionUpdate(_)
            | SetDifferenceUpdate(_)
            | Get(_)
            | ListGet(_)
            | ListCount(_)
            | ListFind(_)
            | ListRFind(_)
            | DictGet(_)
            | DictPop(_)
            | ListPop(_)
            | SetDiscard(_)
            | ListReverse(_)
            | ListSwap(_)
            | ListSort(_)
            | OrderCompare(_)
            | Clear(_)
            | Len(_)
            | ListLenRef(_)
            | ListScalarGetRef(_)
            | IterGet(_)
            | HashIterFirst(_)
            | HashIterNext(_)
            | HashIterGet(_)
            | HashIterKeyRef(_)
            | IterTake(_)
            | Contains(_)
            | Range(_)
            | TryKeys(_)
            | TryValues(_)
            | ListTrySlice(_)
            | TextByteLen
            | TextAtByte
            | TextNextByte
            | TextPrevByte
            | TextIndex
            | TextTryConcat
            | TextTryJoin
            | TextTrySplit
            | TextTrySplitLines
            | TextGet
            | TextTrySlice
            | TextTryReplace
            | TextStartsWith
            | TextEndsWith
            | TextFind
            | TextRFind
            | TextCount
            | TextTryStrip
            | TextTryLStrip
            | TextTryRStrip
            | TextTryRepeat
            | TextTryRemovePrefix
            | TextTryRemoveSuffix
            | TextIsAscii
            | TextIsSpace
            | TextIsDigit
            | TextIsAlpha
            | TextTryLower
            | TextTryUpper
            | TextTrySplitWhitespace
            | TextOrd
            | TextChr
            | TextCompare => Effect::Compute,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_process_global_effects_are_denied_to_workers() {
        assert!(ProcessOp::ChangeDir.effect().worker_denial().is_some());
        for op in [
            ProcessOp::EnvGet,
            ProcessOp::EnvVars,
            ProcessOp::CurrentDir,
            ProcessOp::Spawn,
            ProcessOp::Pipe,
            ProcessOp::Wait,
            ProcessOp::Signal,
            ProcessOp::Poll,
        ] {
            assert!(op.effect().worker_denial().is_none(), "{op:?}");
        }
        assert_eq!(StreamOp::Wait.effect(), Effect::Handle);
        assert_eq!(CollectionOp::OpenFile.effect(), Effect::Path);
        assert_eq!(CollectionOp::TextFind.effect(), Effect::Compute);
    }
}
