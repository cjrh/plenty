//! Statically typed collection operations shared by checking and native lowering.
use crate::collection_opcodes as opcode;
use crate::op::Ty;
use std::sync::Arc;

/// Keep two independently typed set-like owners out of the operation enum's
/// inline payload so adding views does not enlarge every compiler operation.
#[derive(Clone, Debug, PartialEq)]
pub struct KeySetOperation {
    pub left: Ty,
    pub right: Ty,
    pub opcode: i64,
}

/// A key sort the runtime drives. It calls `adapter`, a generated native
/// comparator, with the callback reference and two borrowed rows; the adapter
/// calls `key_of` once per row and compares the keys in the total sort order.
/// Rows move inside the runtime, so no element is retained, released, or
/// copied, and the sort allocates nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct KeySort {
    pub adapter: String,
    /// `key_of(callback: &F, item: &T) -> K`, specialized for this sort.
    pub key_of: String,
    pub list: Ty,
    /// `&F`, the callback reference the adapter receives as its environment.
    pub callback: Ty,
    pub key: Ty,
}

/// A read-only scalar query that a narrow runtime helper answers without
/// changing ownership (issue #364).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Query {
    BytesLen,
    TextLen,
    TextByteLen,
    TextIsAscii,
    /// `text[i]`, which also takes the index and traps when it is out of range.
    TextAt,
    /// `TextOp::AsciiEqualFolded` over two borrowed texts.
    TextAsciiEqualFolded,
    /// Iteration cursors: the scalar at, or the cursor after or before, a
    /// valid UTF-8 boundary.
    TextAtByte,
    TextNextByte,
    TextPrevByte,
    /// `ord(text)`: the code point of a one-scalar text, or a `ParseError`.
    TextOrd,
}

impl Query {
    /// The query an owned-operand operation asks, if it has a borrowed form.
    pub fn of(operation: &CollectionOp) -> Option<Self> {
        Some(match operation {
            CollectionOp::Bytes(crate::bytes::BytesOp::Len) => Self::BytesLen,
            CollectionOp::Len(Ty::Str) => Self::TextLen,
            CollectionOp::TextByteLen => Self::TextByteLen,
            CollectionOp::TextIsAscii => Self::TextIsAscii,
            CollectionOp::TextOrd => Self::TextOrd,
            _ => return None,
        })
    }

    /// The type of the first operand, which the query reads through a
    /// shared reference.
    pub fn operand(self) -> Ty {
        match self {
            Self::BytesLen => Ty::ByteStorage,
            _ => Ty::Str,
        }
    }

    pub fn inputs(self) -> Vec<Ty> {
        let operand = Ty::Ref(Arc::new(self.operand()), false);
        match self {
            Self::TextAt | Self::TextAtByte | Self::TextNextByte | Self::TextPrevByte => {
                vec![operand, Ty::I64]
            }
            Self::TextAsciiEqualFolded => vec![operand.clone(), operand],
            _ => vec![operand],
        }
    }

    pub fn output(self) -> Ty {
        match self {
            Self::TextIsAscii | Self::TextAsciiEqualFolded => Ty::Bool,
            Self::TextAt | Self::TextAtByte => Ty::Str,
            Self::TextOrd => crate::sum::result(Ty::U32, crate::sum::parse_error()),
            Self::BytesLen
            | Self::TextLen
            | Self::TextByteLen
            | Self::TextNextByte
            | Self::TextPrevByte => Ty::I64,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CollectionOp {
    Clist(crate::clist::ClistOp),
    Filesystem(crate::filesystem::FsOp),
    Bytes(crate::bytes::BytesOp),
    Stream(crate::stream::StreamOp),
    Crypto(crate::crypto::CryptoOp),
    Compress(crate::compress::CompressOp),
    Process(crate::process::ProcessOp),
    Time(crate::time::TimeOp),
    Unicode(crate::unicode::UnicodeOp),
    Regex(crate::regex::RegexOp),
    Text(crate::text::TextOp),
    Sqlite(crate::sqlite::SqliteOp),
    Tls(crate::tls::TlsOp),
    Net(crate::net::NetOp),
    OpenFile,
    FileClose,
    FileClosed,
    FileRead,
    FileWrite,
    FileFlush,
    FileSync,
    FileReadLine,
    FileReadSized,
    FileReadLineSized,
    FileReadable,
    FileWritable,
    FileTell,
    FileSeek,
    FileTruncate,
    FileTruncateSized,
    FileReadLines,
    FileWriteLines,
    WriteStdout,
    WriteStderr,
    /// Std only: one SIGPIPE-safe stderr write, resuming at a UTF-8 byte offset.
    ReportWrite,
    /// Std only: whether a process stream is attached to a terminal.
    IsTerminal,
    /// Std only: writes the nearest non-std caller's `file:line:column: `
    /// to standard error; false when there is none.
    CallerLocation,
    /// Std only: `(kind, count, code)` arms or, with a negative count,
    /// disarms failure injection. False outside `plenty test` builds.
    TestFault,
    /// std.actor heap operations: `plenty_actor_heap(op, argument) -> i64`.
    ActorHeap,
    FlushStdout,
    FlushStderr,
    Input,
    Args,
    ArgsBytes,
    InputBytes,
    EncodeUtf8,
    DecodeUtf8,
    WriteStdoutBytes,
    WriteStderrBytes,
    ReadBytes,
    WriteBytes,
    ReadText,
    WriteText,
    AppendText,
    ParseNumber(Ty),
    FormatScalar(Ty),
    FormatValue(Ty),
    TryPrint(Ty),
    TryPrintMany(Vec<Ty>),
    ListUnpack(Ty),
    /// Reports the error `main` returned on stderr, consumes it, and yields
    /// the process status.
    ReportEntryError(Ty),
    ElementRef(Ty, bool),
    TryCopy(Ty),
    Next(Ty),
    TryNew(Ty),        // initial capacity -> Result[collection, AllocError]
    Insert(Ty),        // private builder: collection, element (or key, value) -> collection
    TryReserve(Ty),    // exclusive capacity reservation -> Result[(), AllocError]
    TryInsert(Ty),     // exclusive fallible append/add/insert
    TryExtend(Ty),     // exclusive list + consumed list -> Result[(), AllocError]
    DictTryUpdate(Ty), // exclusive dictionary + consumed dictionary
    SetTryUpdate(Ty),  // exclusive set + consumed set
    SetIsSubset(Ty),
    /// Borrowed set-like operands. Each retains its own owner descriptor,
    /// allowing dictionary keys and sets to share the runtime hash algorithms.
    KeySetRelation(Arc<KeySetOperation>),
    KeySetAlgebra(Arc<KeySetOperation>),
    KeyView(Ty),
    KeyViewToSet(Ty),
    SetIsSuperset(Ty),
    SetIsDisjoint(Ty),
    SetTryUnion(Ty),
    SetTryIntersection(Ty),
    SetTryDifference(Ty),
    SetTrySymmetricDifference(Ty),
    SetIntersectionUpdate(Ty),
    SetDifferenceUpdate(Ty),
    Get(Ty),
    ListGet(Ty), // observed list/index -> Option[non-affine element]
    ListCount(Ty),
    ListFind(Ty),
    ListRFind(Ty),
    DictGet(Ty),      // observed dictionary/key -> Option[non-affine value]
    DictPop(Ty),      // exclusive dictionary/key -> Option[owned value]
    DictGetMut(Ty),   // exclusive dictionary reference/key -> Option[&mut value]
    ListPop(Ty),      // exclusive list/index -> Option[owned element]
    SetDiscard(Ty),   // exclusive set/value -> bool
    ListReverse(Ty),  // exclusive list -> retained internal alias
    ListSwap(Ty),     // exclusive list, two indices -> retained internal alias
    ListSort(Ty),     // exclusive list, descending bool -> retained internal alias
    OrderCompare(Ty), // two ordered keys -> i64
    /// Exclusive list reference, key callback reference, descending bool -> ().
    ListSortBy(Arc<KeySort>),
    Clear(Ty), // exclusive list/dict/set -> retained internal alias
    Len(Ty),
    /// Non-owning list observations; the frontend keeps the receiver's loan live.
    ListLenRef(Ty),
    ListScalarGetRef(Ty),
    /// Stable shared list reference, length, rows, row bytes, index -> scalar.
    /// Metadata is captured before a loop; bounds and negative indices remain checked.
    ListScalarGetView(Ty),
    /// Exclusive scalar row write: list reference, index, value -> ().
    ListScalarSetRef(Ty),
    /// A [`Query`] of the value behind a shared reference, which the
    /// frontend keeps alive under its loan; nothing is retained or released.
    QueryRef(Query),
    /// Borrowed iteration's row cursor: the first row's address and the row
    /// size in bytes, valid while the frontend keeps the list's loan live and
    /// nothing mutates it.
    ListRowsRef(Ty),
    ListRowBytesRef(Ty),
    /// First-row address, row size, in-bounds index -> reference to that row's
    /// element, the address `ElementRef` returns. The index is not checked.
    ListRowRef(Ty, bool),
    /// Internal to `.par`: first-row address, row size, in-bounds index ->
    /// the owned element, leaving an empty row so the list's drop skips it.
    /// The index is not checked.
    ListRowTake(Ty),
    IterGet(Ty),
    // Hash cursors are stable slot IDs plus one; zero terminates traversal.
    HashIterFirst(Ty),
    HashIterNext(Ty),
    HashIterGet(Ty),
    HashIterKeyRef(Ty),
    IterTake(Ty),
    Contains(Ty),
    Range(Ty),
    TryKeys(Ty),
    TryValues(Ty),
    ListTrySlice(Ty),
    TextByteLen,
    TextIndex,
    TextTryConcat,
    /// Separator and any iterable whose elements are `str`: a list, set,
    /// dictionary (keys), string (scalars), or owned generator.
    TextTryJoin(Ty),
    TextTrySplit,
    TextTrySplitLines,
    TextGet,
    TextTrySlice,
    TextTryReplace,
    TextStartsWith,
    TextEndsWith,
    TextFind,
    TextRFind,
    TextCount,
    TextTryStrip,
    TextTryLStrip,
    TextTryRStrip,
    TextTryRepeat,
    TextTryRemovePrefix,
    TextTryRemoveSuffix,
    TextIsAscii,
    TextIsSpace,
    TextIsDigit,
    TextIsAlpha,
    TextTryLower,
    TextTryUpper,
    TextTrySplitWhitespace,
    TextOrd,
    TextChr,
    TextCompare,
}

impl Ty {
    pub fn uses_value_runtime(&self) -> bool {
        self.is_collection()
            || matches!(
                self,
                Self::Enum(_)
                    | Self::Clist(_)
                    | Self::Class(_)
                    | Self::Box(_)
                    | Self::Shared(_)
                    | Self::Generator(_)
                    | Self::File
                    | Self::Channel(..)
                    | Self::CancellationToken
                    | Self::Executor
                    | Self::Future(_)
            )
    }
    pub fn is_collection(&self) -> bool {
        matches!(
            self,
            Self::List(_) | Self::Set(_) | Self::Dict(_, _) | Self::Range(_)
        )
    }
    pub fn element(&self) -> Option<Ty> {
        match self {
            Self::List(t) | Self::Set(t) | Self::KeyView(t) | Self::Dict(t, _) => {
                Some((**t).clone())
            }
            Self::Range(t) => Some((**t).clone()),
            Self::Str => Some(Self::Str),
            Self::ByteStr => Some(Self::U8),
            Self::Generator(t) => Some(t.element.clone()),
            _ => None,
        }
    }
    pub fn hashable(&self) -> bool {
        self.is_int()
            || matches!(self, Self::Bool | Self::Str | Self::ByteStr)
            || matches!(self, Self::Enum(t) if t.tuple() && t.get().variants[0].fields.iter().all(Self::hashable))
    }
}

impl CollectionOp {
    pub fn key_set_relation(left: Ty, right: Ty, opcode: i64) -> Self {
        Self::KeySetRelation(Arc::new(KeySetOperation {
            left,
            right,
            opcode,
        }))
    }
    pub fn key_set_algebra(left: Ty, right: Ty, opcode: i64) -> Self {
        Self::KeySetAlgebra(Arc::new(KeySetOperation {
            left,
            right,
            opcode,
        }))
    }
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        use CollectionOp::*;
        match self {
            Clist(op) => op.signature(),
            KeyView(t) => {
                let Ty::Dict(key, _) = t else { unreachable!() };
                (
                    vec![Ty::Ref(Arc::new(t.clone()), false)],
                    Ty::KeyView(key.clone()),
                )
            }
            KeySetRelation(op) => (vec![op.left.clone(), op.right.clone()], Ty::Bool),
            KeyViewToSet(t) => (
                vec![t.clone()],
                crate::sum::result(
                    Ty::Set(Arc::new(t.element().unwrap())),
                    crate::sum::alloc_error(),
                ),
            ),
            KeySetAlgebra(op) => (
                vec![op.left.clone(), op.right.clone()],
                crate::sum::result(
                    Ty::Set(Arc::new(op.left.element().unwrap())),
                    crate::sum::alloc_error(),
                ),
            ),
            Filesystem(op) => op.signature(),
            Bytes(op) => op.signature(),
            Stream(op) => op.signature(),
            Crypto(op) => op.signature(),
            Compress(op) => op.signature(),
            Process(op) => op.signature(),
            Time(op) => op.signature(),
            Unicode(op) => op.signature(),
            Regex(op) => op.signature(),
            Text(op) => op.signature(),
            Sqlite(op) => op.signature(),
            Tls(op) => op.signature(),
            Net(op) => op.signature(),
            OpenFile => (
                vec![Ty::Str, Ty::Str],
                crate::sum::result(Ty::File, crate::sum::io_error()),
            ),
            FileClose => (
                vec![Ty::File],
                crate::sum::result(Ty::Unit, crate::sum::io_error()),
            ),
            FileClosed => (vec![Ty::File], Ty::Bool),
            FileReadable | FileWritable => (
                vec![Ty::File],
                crate::sum::result(Ty::Bool, crate::sum::io_error()),
            ),
            FileTell => (
                vec![Ty::File],
                crate::sum::result(Ty::U64, crate::sum::io_error()),
            ),
            FileSeek => (
                vec![Ty::File, Ty::U64],
                crate::sum::result(Ty::Unit, crate::sum::io_error()),
            ),
            FileTruncate => (
                vec![Ty::File],
                crate::sum::result(Ty::I64, crate::sum::io_error()),
            ),
            FileReadLines => (
                vec![Ty::File],
                crate::sum::result(
                    Ty::List(std::sync::Arc::new(Ty::Str)),
                    crate::sum::io_error(),
                ),
            ),
            FileWriteLines => (
                vec![Ty::File, Ty::List(std::sync::Arc::new(Ty::Str))],
                crate::sum::result(Ty::Unit, crate::sum::io_error()),
            ),
            FileTruncateSized => (
                vec![Ty::File, Ty::I64],
                crate::sum::result(Ty::I64, crate::sum::io_error()),
            ),
            FileRead | FileReadLine => (
                vec![Ty::File],
                crate::sum::result(Ty::Str, crate::sum::io_error()),
            ),
            FileReadSized | FileReadLineSized => (
                vec![Ty::File, Ty::I64],
                crate::sum::result(Ty::Str, crate::sum::io_error()),
            ),
            FileWrite => (
                vec![Ty::File, Ty::Str],
                crate::sum::result(Ty::I64, crate::sum::io_error()),
            ),
            FileFlush | FileSync => (
                vec![Ty::File],
                crate::sum::result(Ty::Unit, crate::sum::io_error()),
            ),
            WriteText | AppendText => (
                vec![Ty::Str, Ty::Str],
                crate::sum::result(Ty::I64, crate::sum::io_error()),
            ),
            ReadText => (
                vec![Ty::Str],
                crate::sum::result(Ty::Str, crate::sum::io_error()),
            ),
            Args => (
                vec![],
                crate::sum::result(
                    Ty::List(std::sync::Arc::new(Ty::Str)),
                    crate::sum::io_error(),
                ),
            ),
            ArgsBytes => (
                vec![],
                crate::sum::result(
                    Ty::List(Arc::new(Ty::List(Arc::new(Ty::U8)))),
                    crate::sum::io_error(),
                ),
            ),
            InputBytes => (
                vec![Ty::U8],
                crate::sum::result(
                    crate::sum::option(Ty::List(Arc::new(Ty::U8))),
                    crate::sum::io_error(),
                ),
            ),
            EncodeUtf8 => (
                vec![Ty::Str],
                crate::sum::result(Ty::List(Arc::new(Ty::U8)), crate::sum::io_error()),
            ),
            DecodeUtf8 => (
                vec![Ty::List(Arc::new(Ty::U8))],
                crate::sum::result(Ty::Str, crate::sum::io_error()),
            ),
            WriteStdoutBytes | WriteStderrBytes => (
                vec![Ty::List(Arc::new(Ty::U8))],
                crate::sum::result(Ty::I64, crate::sum::io_error()),
            ),
            ReadBytes => (
                vec![Ty::List(Arc::new(Ty::U8))],
                crate::sum::result(Ty::List(Arc::new(Ty::U8)), crate::sum::io_error()),
            ),
            WriteBytes => (
                vec![Ty::List(Arc::new(Ty::U8)), Ty::List(Arc::new(Ty::U8))],
                crate::sum::result(Ty::I64, crate::sum::io_error()),
            ),
            Input => (
                vec![],
                crate::sum::result(crate::sum::option(Ty::Str), crate::sum::io_error()),
            ),
            FlushStdout | FlushStderr => {
                (vec![], crate::sum::result(Ty::Unit, crate::sum::io_error()))
            }
            CallerLocation => (vec![], Ty::Bool),
            IsTerminal => (vec![Ty::U8], Ty::Bool),
            TestFault => (vec![Ty::I64, Ty::I64, Ty::I32], Ty::Bool),
            ActorHeap => (vec![Ty::I64, Ty::I64], Ty::I64),
            WriteStdout | WriteStderr => (
                vec![Ty::Str],
                crate::sum::result(Ty::I64, crate::sum::io_error()),
            ),
            ReportWrite => (
                vec![Ty::Str, Ty::I64],
                crate::sum::result(Ty::I64, crate::sum::io_error()),
            ),
            ElementRef(t, mutable) => {
                let (key, value) = match t {
                    Ty::List(element) => (Ty::I64, (**element).clone()),
                    Ty::Dict(key, value) => ((**key).clone(), (**value).clone()),
                    _ => unreachable!("element reference collection"),
                };
                (
                    vec![Ty::Ref(std::sync::Arc::new(t.clone()), *mutable), key],
                    Ty::Ref(std::sync::Arc::new(value), *mutable),
                )
            }
            TryPrint(t) => (
                vec![t.clone()],
                crate::sum::result(Ty::Unit, crate::sum::io_error()),
            ),
            ReportEntryError(t) => (vec![t.clone()], Ty::I32),
            TryPrintMany(types) => (
                types.clone(),
                crate::sum::result(Ty::Unit, crate::sum::io_error()),
            ),
            ListUnpack(t) => (vec![t.clone(), Ty::I64], t.clone()),
            FormatScalar(t) | FormatValue(t) => (
                vec![t.clone()],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            ParseNumber(t) => (
                vec![Ty::Str],
                crate::sum::result(t.clone(), crate::sum::parse_error()),
            ),
            TryCopy(t) => (
                vec![t.clone()],
                crate::sum::result(t.clone(), crate::sum::alloc_error()),
            ),
            Next(t) => (
                vec![t.clone()],
                crate::sum::option(t.element().expect("generator element")),
            ),
            TextByteLen => (vec![Ty::Str], Ty::I64),
            TextIsAscii | TextIsSpace | TextIsDigit | TextIsAlpha => (vec![Ty::Str], Ty::Bool),
            TextCompare => (vec![Ty::Str, Ty::Str], Ty::I64),
            OrderCompare(t) => (vec![t.clone(), t.clone()], Ty::I64),
            TextOrd => (
                vec![Ty::Str],
                crate::sum::result(Ty::U32, crate::sum::parse_error()),
            ),
            TextChr => (
                vec![Ty::U32],
                crate::sum::result(Ty::Str, crate::sum::parse_error()),
            ),
            TextStartsWith | TextEndsWith => (vec![Ty::Str, Ty::Str], Ty::Bool),
            TextFind | TextRFind => (vec![Ty::Str, Ty::Str], crate::sum::option(Ty::I64)),
            TextCount => (vec![Ty::Str, Ty::Str], Ty::I64),
            TextTryStrip | TextTryLStrip | TextTryRStrip | TextTryLower | TextTryUpper => (
                vec![Ty::Str],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TextIndex => (vec![Ty::Str, Ty::I64], Ty::Str),
            TextTryConcat | TextTryRemovePrefix | TextTryRemoveSuffix => (
                vec![Ty::Str, Ty::Str],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TextTryJoin(t) => (
                vec![Ty::Str, t.clone()],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TextTrySplit => (
                vec![Ty::Str, Ty::Str],
                crate::sum::result(
                    Ty::List(std::sync::Arc::new(Ty::Str)),
                    crate::sum::alloc_error(),
                ),
            ),
            TextTrySplitWhitespace => (
                vec![Ty::Str],
                crate::sum::result(Ty::List(Arc::new(Ty::Str)), crate::sum::alloc_error()),
            ),
            TextTrySplitLines => (
                vec![Ty::Str, Ty::Bool],
                crate::sum::result(
                    Ty::List(std::sync::Arc::new(Ty::Str)),
                    crate::sum::alloc_error(),
                ),
            ),
            TextGet => (vec![Ty::Str, Ty::I64], crate::sum::option(Ty::Str)),
            TextTrySlice => (
                vec![Ty::Str, Ty::I64, Ty::I64],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TextTryRepeat => (
                vec![Ty::Str, Ty::I64],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TextTryReplace => (
                vec![Ty::Str, Ty::Str, Ty::Str],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TryNew(t) => (
                vec![Ty::I64],
                crate::sum::result(t.clone(), crate::sum::alloc_error()),
            ),
            TryReserve(t) => (vec![t.clone(), Ty::I64], crate::sum::allocation_result()),
            SetTryUnion(t)
            | SetTryIntersection(t)
            | SetTryDifference(t)
            | SetTrySymmetricDifference(t) => (
                vec![t.clone(), t.clone()],
                crate::sum::result(t.clone(), crate::sum::alloc_error()),
            ),
            SetIsSubset(t) | SetIsSuperset(t) | SetIsDisjoint(t) => {
                (vec![t.clone(), t.clone()], Ty::Bool)
            }
            TryExtend(t) | DictTryUpdate(t) | SetTryUpdate(t) => {
                (vec![t.clone(), t.clone()], crate::sum::allocation_result())
            }
            TryInsert(t) => {
                let mut args = vec![t.clone(), t.element().expect("collection element")];
                if let Ty::Dict(_, v) = t {
                    args.push((**v).clone());
                }
                (args, crate::sum::allocation_result())
            }
            Insert(t) => {
                let mut args = vec![t.clone(), t.element().expect("collection element")];
                if let Ty::Dict(_, v) = t {
                    args.push((**v).clone());
                }
                (args, t.clone())
            }
            Get(t) => match t {
                Ty::Dict(k, v) => (vec![t.clone(), (**k).clone()], (**v).clone()),
                _ => (vec![t.clone(), Ty::I64], t.element().unwrap()),
            },
            DictGet(t) | DictPop(t) => {
                let Ty::Dict(k, v) = t else { unreachable!() };
                (
                    vec![t.clone(), (**k).clone()],
                    crate::sum::option((**v).clone()),
                )
            }
            DictGetMut(t) => {
                let Ty::Dict(k, v) = t else { unreachable!() };
                (
                    vec![Ty::Ref(Arc::new(t.clone()), true), (**k).clone()],
                    crate::sum::option(Ty::Ref(v.clone(), true)),
                )
            }
            ListGet(t) | ListPop(t) => {
                let Ty::List(element) = t else { unreachable!() };
                (
                    vec![t.clone(), Ty::I64],
                    crate::sum::option((**element).clone()),
                )
            }
            SetDiscard(t) => (vec![t.clone(), t.element().unwrap()], Ty::Bool),
            ListCount(t) => (vec![t.clone(), t.element().unwrap()], Ty::I64),
            ListFind(t) | ListRFind(t) => (
                vec![t.clone(), t.element().unwrap()],
                crate::sum::option(Ty::I64),
            ),
            SetIntersectionUpdate(t) | SetDifferenceUpdate(t) => {
                (vec![t.clone(), t.clone()], t.clone())
            }
            ListReverse(t) | Clear(t) => (vec![t.clone()], t.clone()),
            ListSort(t) => (vec![t.clone(), Ty::Bool], t.clone()),
            ListSwap(t) => (vec![t.clone(), Ty::I64, Ty::I64], t.clone()),
            Len(t) | HashIterFirst(t) => (vec![t.clone()], Ty::I64),
            ListLenRef(t) | ListRowsRef(t) | ListRowBytesRef(t) => {
                (vec![Ty::Ref(Arc::new(t.clone()), false)], Ty::I64)
            }
            ListRowRef(t, mutable) => (
                vec![Ty::I64, Ty::I64, Ty::I64],
                Ty::Ref(Arc::new(t.element().unwrap()), *mutable),
            ),
            ListRowTake(t) => (vec![Ty::I64, Ty::I64, Ty::I64], t.element().unwrap()),
            QueryRef(query) => (query.inputs(), query.output()),
            ListScalarGetRef(t) => (
                vec![Ty::Ref(Arc::new(t.clone()), false), Ty::I64],
                t.element().unwrap(),
            ),
            ListScalarGetView(t) => (
                vec![
                    Ty::Ref(Arc::new(t.clone()), false),
                    Ty::I64,
                    Ty::I64,
                    Ty::I64,
                    Ty::I64,
                ],
                t.element().unwrap(),
            ),
            ListSortBy(sort) => (
                vec![
                    Ty::Ref(Arc::new(sort.list.clone()), true),
                    sort.callback.clone(),
                    Ty::Bool,
                ],
                Ty::Unit,
            ),
            ListScalarSetRef(t) => (
                vec![
                    Ty::Ref(Arc::new(t.clone()), true),
                    Ty::I64,
                    t.element().unwrap(),
                ],
                Ty::Unit,
            ),
            HashIterNext(t) => (vec![t.clone(), Ty::I64], Ty::I64),
            HashIterKeyRef(t) => (
                vec![Ty::Ref(Arc::new(t.clone()), false), Ty::I64],
                Ty::Ref(Arc::new(t.element().unwrap()), false),
            ),
            IterGet(t) | IterTake(t) | HashIterGet(t) => {
                (vec![t.clone(), Ty::I64], t.element().unwrap())
            }
            Contains(t) => (
                vec![
                    if *t == Ty::Str {
                        Ty::Str
                    } else {
                        t.element().unwrap()
                    },
                    t.clone(),
                ],
                Ty::Bool,
            ),
            Range(t) => (
                vec![t.clone(), t.clone(), Ty::I64],
                Ty::Range(Arc::new(t.clone())),
            ),
            TryKeys(t) | TryValues(t) => {
                let Ty::Dict(k, v) = t else { unreachable!() };
                let element = if matches!(self, TryKeys(_)) { k } else { v };
                (
                    vec![t.clone()],
                    crate::sum::result(Ty::List(element.clone()), crate::sum::alloc_error()),
                )
            }
            ListTrySlice(t) => (
                vec![t.clone(), Ty::I64, Ty::I64],
                crate::sum::result(t.clone(), crate::sum::alloc_error()),
            ),
        }
    }
    /// Whether the runtime can trap in this operation, which then takes the
    /// source position to report. Iteration reads stay in bounds by
    /// construction.
    pub fn traps(&self) -> bool {
        matches!(
            self,
            Self::Get(_)
                | Self::ElementRef(..)
                | Self::ListSwap(_)
                | Self::ListUnpack(_)
                | Self::Range(_)
                | Self::TextIndex
                | Self::TextTrySplit
                | Self::QueryRef(Query::TextAt)
        ) || matches!(self, Self::Bytes(op) if op.traps())
            || matches!(self, Self::Clist(op) if op.traps())
    }

    pub fn opcode(&self) -> i64 {
        match self {
            Self::Clist(op) => op.opcode(),
            Self::Filesystem(op) => op.opcode(),
            Self::Bytes(op) => op.opcode(),
            Self::Stream(op) => op.opcode(),
            Self::Crypto(op) => op.opcode(),
            Self::Compress(op) => op.opcode(),
            Self::Process(op) => op.opcode(),
            Self::Time(op) => op.opcode(),
            Self::Unicode(op) => op.opcode(),
            Self::Regex(op) => op.opcode(),
            Self::Text(op) => op.opcode(),
            Self::Sqlite(op) => op.opcode(),
            Self::Tls(op) => op.opcode(),
            Self::Net(op) => op.opcode(),
            Self::WriteStdout => 80,
            Self::FlushStdout => 81,
            Self::CallerLocation | Self::TestFault | Self::ActorHeap => {
                unreachable!("lowered to direct calls, not the collection dispatcher")
            }
            Self::WriteStderr => 82,
            // Report transport primitives have their own range after process I/O.
            Self::ReportWrite => 510,
            Self::IsTerminal => 511,
            Self::FlushStderr => 83,
            Self::Input => 84,
            Self::Args => 85,
            Self::ArgsBytes => 142,
            Self::InputBytes => 143,
            Self::EncodeUtf8 => 144,
            Self::DecodeUtf8 => 145,
            Self::WriteStdoutBytes => 146,
            Self::WriteStderrBytes => 147,
            Self::ReadBytes => 148,
            Self::WriteBytes => 149,
            Self::ReadText => 86,
            Self::WriteText => 87,
            Self::AppendText => 88,
            Self::OpenFile => 89,
            Self::FileClose => 90,
            Self::FileClosed => 91,
            Self::FileRead => 92,
            Self::FileWrite => 93,
            Self::FileFlush => 94,
            Self::FileSync => 95,
            Self::FileReadLine => 96,
            Self::FileReadSized => 97,
            Self::FileReadLineSized => 98,
            Self::FileReadable => 99,
            Self::FileWritable => 100,
            Self::FileTell => 101,
            Self::FileSeek => 102,
            Self::FileTruncate => 103,
            Self::FileTruncateSized => 104,
            Self::FileReadLines => 105,
            Self::FileWriteLines => 106,
            Self::FormatScalar(_) => 79,
            Self::FormatValue(_) => 110,
            Self::TryPrint(_) => 111,
            Self::TryPrintMany(_) => 166,
            Self::ListUnpack(_) => 167,
            Self::ReportEntryError(_) => 122,
            Self::ElementRef(..) => 112,
            Self::ParseNumber(_) => 78,
            Self::TryCopy(_) => 33,
            Self::Next(_) => 24,
            Self::TryNew(_) => 32,
            Self::Insert(_) => 1,
            Self::TryReserve(_) => 28,
            Self::TryInsert(_) => 29,
            Self::TryExtend(_) => 59,
            Self::DictTryUpdate(_) => 60,
            Self::SetTryUpdate(_) => 61,
            Self::SetIsSubset(_) => opcode::SET_IS_SUBSET,
            Self::KeySetRelation(op) | Self::KeySetAlgebra(op) => op.opcode,
            Self::KeyView(_) => unreachable!("key views lower inline"),
            Self::KeyViewToSet(_) => opcode::KEY_VIEW_TO_SET,
            Self::SetIsSuperset(_) => opcode::SET_IS_SUPERSET,
            Self::SetIsDisjoint(_) => opcode::SET_IS_DISJOINT,
            Self::SetTryUnion(_) => opcode::SET_UNION,
            Self::SetTryIntersection(_) => opcode::SET_INTERSECTION,
            Self::SetTryDifference(_) => opcode::SET_DIFFERENCE,
            Self::SetTrySymmetricDifference(_) => opcode::SET_SYMMETRIC_DIFFERENCE,
            Self::SetIntersectionUpdate(_) => 69,
            Self::SetDifferenceUpdate(_) => 70,
            Self::Get(_) => 4,
            Self::DictGet(_) => 38,
            Self::ListGet(_) => 42,
            Self::ListCount(_) => 73,
            Self::ListFind(_) => 74,
            Self::ListRFind(_) => 75,
            Self::DictPop(_) => 39,
            Self::DictGetMut(_) => 113,
            Self::ListPop(_) => 40,
            Self::SetDiscard(_) => 41,
            Self::ListReverse(_) => 57,
            Self::ListSwap(_) => 126,
            Self::ListSort(_) => 127,
            Self::Clear(_) => 58,
            Self::Len(_) => 5,
            Self::ListLenRef(_)
            | Self::ListScalarGetRef(_)
            | Self::ListScalarGetView(_)
            | Self::ListScalarSetRef(_)
            | Self::ListRowsRef(_)
            | Self::ListRowBytesRef(_)
            | Self::ListRowRef(..)
            | Self::ListRowTake(_)
            | Self::ListSortBy(_)
            | Self::OrderCompare(_)
            | Self::QueryRef(_) => {
                unreachable!(
                    "borrowed list access, queries, key sorts, and ordering use narrow runtime helpers"
                )
            }
            Self::IterGet(_) => 6,
            Self::HashIterFirst(_) => 123,
            Self::HashIterNext(_) => 124,
            Self::HashIterGet(_) => 125,
            Self::HashIterKeyRef(_) => 140,
            Self::IterTake(_) => 15,
            Self::Contains(_) => 7,
            Self::Range(_) => 10,
            Self::TryKeys(_) => 43,
            Self::TryValues(_) => 44,
            Self::ListTrySlice(_) => 45,
            Self::TextByteLen => 12,
            Self::TextIndex => 114,
            Self::TextTryRemovePrefix => 71,
            Self::TextTryRemoveSuffix => 72,
            Self::TextIsAscii => 76,
            Self::TextIsSpace => 77,
            Self::TextTryConcat => 34,
            Self::TextTryJoin(_) => 35,
            Self::TextTrySplit => 36,
            Self::TextTrySplitWhitespace => 129,
            Self::TextOrd => 130,
            Self::TextChr => 131,
            Self::TextCompare => 132,
            Self::TextIsDigit => 133,
            Self::TextIsAlpha => 134,
            Self::TextTryLower => 135,
            Self::TextTryUpper => 136,
            Self::TextTrySplitLines => 107,
            Self::TextGet => 37,
            Self::TextTrySlice => 46,
            Self::TextTryReplace => 47,
            Self::TextStartsWith => 48,
            Self::TextEndsWith => 49,
            Self::TextFind => 50,
            Self::TextRFind => 51,
            Self::TextCount => 52,
            Self::TextTryStrip => 53,
            Self::TextTryLStrip => 54,
            Self::TextTryRStrip => 55,
            Self::TextTryRepeat => 56,
        }
    }
}
