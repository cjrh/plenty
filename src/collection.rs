//! Statically typed collection operations shared by checking and native lowering.
use crate::op::Ty;

#[derive(Clone, Debug, PartialEq)]
pub enum CollectionOp {
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
    FlushStdout,
    FlushStderr,
    Input,
    Args,
    ReadText,
    WriteText,
    AppendText,
    ParseNumber(Ty),
    FormatScalar(Ty),
    FormatValue(Ty),
    TryPrint(Ty),
    Copy(Ty),
    TryCopy(Ty),
    Next(Ty),
    New(Ty),
    TryNew(Ty),        // initial capacity -> Result[collection, AllocError]
    Insert(Ty),        // private builder: collection, element (or key, value) -> collection
    Append(Ty),        // exclusive in-place update of a list or set
    TryReserve(Ty),    // exclusive capacity reservation -> Result[(), AllocError]
    TryInsert(Ty),     // exclusive fallible append/add/insert
    TryExtend(Ty),     // exclusive list + consumed list -> Result[(), AllocError]
    DictTryUpdate(Ty), // exclusive dictionary + consumed dictionary
    SetTryUpdate(Ty),  // exclusive set + consumed set
    SetIsSubset(Ty),
    SetIsSuperset(Ty),
    SetIsDisjoint(Ty),
    SetTryUnion(Ty),
    SetTryIntersection(Ty),
    SetTryDifference(Ty),
    SetTrySymmetricDifference(Ty),
    SetIntersectionUpdate(Ty),
    SetDifferenceUpdate(Ty),
    Put(Ty), // exclusive in-place indexed update
    Get(Ty),
    ListGet(Ty), // observed list/index -> Option[non-affine element]
    ListCount(Ty),
    ListFind(Ty),
    ListRFind(Ty),
    DictGet(Ty),     // observed dictionary/key -> Option[non-affine value]
    DictPop(Ty),     // exclusive dictionary/key -> Option[owned value]
    ListPop(Ty),     // exclusive list/index -> Option[owned element]
    SetDiscard(Ty),  // exclusive set/value -> bool
    ListReverse(Ty), // exclusive list -> retained internal alias
    Clear(Ty),       // exclusive list/dict/set -> retained internal alias
    Len(Ty),
    IterGet(Ty),
    IterTake(Ty),
    Contains(Ty),
    Range,
    Values(Ty),
    TryKeys(Ty),
    TryValues(Ty),
    ListTrySlice(Ty),
    TextByteLen,
    TextAtByte,
    TextTryConcat,
    TextTryJoin,
    TextTrySplit,
    TextTrySplitLines,
    TextTryGet,
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
}

impl Ty {
    pub fn uses_value_runtime(&self) -> bool {
        self.is_collection()
            || matches!(
                self,
                Self::Enum(_) | Self::Class(_) | Self::Generator(_) | Self::File
            )
    }
    pub fn is_collection(&self) -> bool {
        matches!(
            self,
            Self::List(_) | Self::Set(_) | Self::Dict(_, _) | Self::Range
        )
    }
    pub fn element(&self) -> Option<Ty> {
        match self {
            Self::List(t) | Self::Set(t) | Self::Dict(t, _) => Some((**t).clone()),
            Self::Range => Some(Self::I64),
            Self::Str => Some(Self::Str),
            Self::Generator(t) => Some((**t).clone()),
            _ => None,
        }
    }
    pub fn hashable(&self) -> bool {
        self.is_int() || matches!(self, Self::Bool | Self::Str)
    }
}

impl CollectionOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        use CollectionOp::*;
        match self {
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
                crate::sum::result(Ty::List(std::rc::Rc::new(Ty::Str)), crate::sum::io_error()),
            ),
            FileWriteLines => (
                vec![Ty::File, Ty::List(std::rc::Rc::new(Ty::Str))],
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
                crate::sum::result(Ty::List(std::rc::Rc::new(Ty::Str)), crate::sum::io_error()),
            ),
            Input => (
                vec![],
                crate::sum::result(crate::sum::option(Ty::Str), crate::sum::io_error()),
            ),
            FlushStdout | FlushStderr => {
                (vec![], crate::sum::result(Ty::Unit, crate::sum::io_error()))
            }
            WriteStdout | WriteStderr => (
                vec![Ty::Str],
                crate::sum::result(Ty::I64, crate::sum::io_error()),
            ),
            TryPrint(t) => (
                vec![t.clone()],
                crate::sum::result(Ty::Unit, crate::sum::io_error()),
            ),
            FormatScalar(t) | FormatValue(t) => (
                vec![t.clone()],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            ParseNumber(t) => (
                vec![Ty::Str],
                crate::sum::result(t.clone(), crate::sum::parse_error()),
            ),
            Copy(t) => (vec![t.clone()], t.clone()),
            TryCopy(t) => (
                vec![t.clone()],
                crate::sum::result(t.clone(), crate::sum::alloc_error()),
            ),
            Next(t) => (
                vec![t.clone()],
                crate::sum::option(t.element().expect("generator element")),
            ),
            TextByteLen => (vec![Ty::Str], Ty::I64),
            TextIsAscii | TextIsSpace => (vec![Ty::Str], Ty::Bool),
            TextStartsWith | TextEndsWith => (vec![Ty::Str, Ty::Str], Ty::Bool),
            TextFind | TextRFind => (vec![Ty::Str, Ty::Str], crate::sum::option(Ty::I64)),
            TextCount => (vec![Ty::Str, Ty::Str], Ty::I64),
            TextTryStrip | TextTryLStrip | TextTryRStrip => (
                vec![Ty::Str],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TextAtByte => (vec![Ty::Str, Ty::I64], Ty::Str),
            TextTryConcat | TextTryRemovePrefix | TextTryRemoveSuffix => (
                vec![Ty::Str, Ty::Str],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TextTryJoin => (
                vec![Ty::Str, Ty::List(std::rc::Rc::new(Ty::Str))],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TextTrySplit => (
                vec![Ty::Str, Ty::Str],
                crate::sum::result(
                    Ty::List(std::rc::Rc::new(Ty::Str)),
                    crate::sum::alloc_error(),
                ),
            ),
            TextTrySplitLines => (
                vec![Ty::Str, Ty::Bool],
                crate::sum::result(
                    Ty::List(std::rc::Rc::new(Ty::Str)),
                    crate::sum::alloc_error(),
                ),
            ),
            TextTryGet => (
                vec![Ty::Str, Ty::I64],
                crate::sum::result(crate::sum::option(Ty::Str), crate::sum::alloc_error()),
            ),
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
            New(t) => (vec![], t.clone()),
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
            Insert(t) | Append(t) => {
                let mut args = vec![t.clone(), t.element().expect("collection element")];
                if let Ty::Dict(_, v) = t {
                    args.push((**v).clone());
                }
                (args, t.clone())
            }
            Put(t) => match t {
                Ty::List(v) => (vec![t.clone(), Ty::I64, (**v).clone()], t.clone()),
                Ty::Dict(k, v) => (vec![t.clone(), (**k).clone(), (**v).clone()], t.clone()),
                _ => unreachable!(),
            },
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
            Len(t) => (vec![t.clone()], Ty::I64),
            IterGet(t) | IterTake(t) => (vec![t.clone(), Ty::I64], t.element().unwrap()),
            Contains(t) => (vec![t.element().unwrap(), t.clone()], Ty::Bool),
            Range => (vec![Ty::I64, Ty::I64, Ty::I64], Ty::Range),
            Values(t) => {
                let Ty::Dict(_, v) = t else { unreachable!() };
                (vec![t.clone()], Ty::List(v.clone()))
            }
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
    pub fn opcode(&self) -> i64 {
        match self {
            Self::WriteStdout => 80,
            Self::FlushStdout => 81,
            Self::WriteStderr => 82,
            Self::FlushStderr => 83,
            Self::Input => 84,
            Self::Args => 85,
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
            Self::ParseNumber(_) => 78,
            Self::Copy(_) => 14,
            Self::TryCopy(_) => 33,
            Self::Next(_) => 24,
            Self::New(_) => 0,
            Self::TryNew(_) => 32,
            Self::Insert(_) => 1,
            Self::Append(_) => 2,
            Self::TryReserve(_) => 28,
            Self::TryInsert(_) => 29,
            Self::TryExtend(_) => 59,
            Self::DictTryUpdate(_) => 60,
            Self::SetTryUpdate(_) => 61,
            Self::SetIsSubset(_) => 62,
            Self::SetIsSuperset(_) => 63,
            Self::SetIsDisjoint(_) => 64,
            Self::SetTryUnion(_) => 65,
            Self::SetTryIntersection(_) => 66,
            Self::SetTryDifference(_) => 67,
            Self::SetTrySymmetricDifference(_) => 68,
            Self::SetIntersectionUpdate(_) => 69,
            Self::SetDifferenceUpdate(_) => 70,
            Self::Put(_) => 3,
            Self::Get(_) => 4,
            Self::DictGet(_) => 38,
            Self::ListGet(_) => 42,
            Self::ListCount(_) => 73,
            Self::ListFind(_) => 74,
            Self::ListRFind(_) => 75,
            Self::DictPop(_) => 39,
            Self::ListPop(_) => 40,
            Self::SetDiscard(_) => 41,
            Self::ListReverse(_) => 57,
            Self::Clear(_) => 58,
            Self::Len(_) => 5,
            Self::IterGet(_) => 6,
            Self::IterTake(_) => 15,
            Self::Contains(_) => 7,
            Self::Range => 10,
            Self::Values(_) => 11,
            Self::TryKeys(_) => 43,
            Self::TryValues(_) => 44,
            Self::ListTrySlice(_) => 45,
            Self::TextByteLen => 12,
            Self::TextAtByte => 13,
            Self::TextTryRemovePrefix => 71,
            Self::TextTryRemoveSuffix => 72,
            Self::TextIsAscii => 76,
            Self::TextIsSpace => 77,
            Self::TextTryConcat => 34,
            Self::TextTryJoin => 35,
            Self::TextTrySplit => 36,
            Self::TextTrySplitLines => 107,
            Self::TextTryGet => 37,
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
