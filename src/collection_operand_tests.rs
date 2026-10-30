//! Cross-check shared native scratch arity against independently typed APIs.
//! Exhaustive matches make a newly added primitive require a test decision.
use crate::{
    bytes::BytesOp, compress::CompressOp, crypto::CryptoOp, net::NetOp, sqlite::SqliteOp,
    stream::StreamOp, tls::TlsOp,
};

macro_rules! check_family {
    ($family:ident: $($variant:ident),+ $(,)?) => {{
        for operation in [$($family::$variant),+] {
            match operation { $($family::$variant)|+ => {} }
            let count = operation.signature().0.len();
            assert_eq!(crate::collection_opcodes::scratch_operand_count(operation.opcode()),
                count.saturating_sub(3), "{operation:?}");
        }
    }};
}

#[test]
fn scratch_operand_contract_matches_typed_primitives() {
    check_family!(BytesOp: New, Len, Capacity, Get, Set, Reserve, Resize, Push,
        Append, Overwrite, FromList, ToList, FromText, ToText, WriteFloat,
        WriteFloatSpec, Find, FindIn, FindBytes, Count, Matches, MatchesText,
        Run, Fill, Item, SetItem, ValueItem, FromStatic, CopyWithin, Slice,
        AppendLiteral, OverwriteLiteral, FindInLiteral, FindBytesLiteral,
        MatchesLiteral, SliceLiteral);
    check_family!(CryptoOp: New, NewKeyed, Update, UpdateText, Finish, Verify,
        Equal, NewDeriveKey, AesSeal, AesOpen, ChaChaSeal, ChaChaOpen, EdPublic,
        EdSign, EdVerify, Hkdf, XNew, XPublic, XAgree, KeyNew, RsaLoad, P256Load,
        KeySize, KeyPublic, KeySign, RsaVerify, P256Verify, P256Check);
    check_family!(CompressOp: New, Step, Flush, Ended, Restart, Crc32, StepLiteral);
    check_family!(StreamOp: FileRead, FileWrite, StdinRead, StdWrite, FileSeek,
        EntropyRead, Wait, FileWriteLiteral, StdWriteLiteral);
    check_family!(NetOp: Open, Accept, FinishConnect, Address, Shutdown,
        SetOption, SendTo, ReceiveFrom, Resolve, ReadyWait, SelectorOpen,
        SelectorWatch, Select, ReadyNumber, SendToLiteral);
    check_family!(TlsOp: Roots, RootsCount, ClientConfig, ServerConfig, Client,
        Server, Feed, Take, Read, Write, State, Close, Builder, BuilderRoots,
        BuilderCertificate, BuilderProtocol, BuilderVersion, BuilderSuite,
        BuilderRevocations, Build, Protocol, ServerName, PeerCertificate,
        Version, Suite, WriteLiteral);
    check_family!(SqliteOp: Open, Message, Execute, Prepare, Changes, LastRowid,
        BusyTimeout, Close, Begin, Commit, Rollback, Step, Reset, ClearBindings,
        ParameterCount, ParameterIndex, BindNull, BindInteger, BindReal, BindText,
        BindBlob, ColumnCount, ColumnType, ColumnInteger, ColumnReal, ColumnText,
        ColumnBlob, ColumnName, LoadExtension);
}
