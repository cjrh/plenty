//! Which operations can block until another thread acts, for the
//! live-endpoint join rule (book/src/design/facilities/03-concurrency/channels.md,
//! "Which workers can wait").
//!
//! The rule is an allowlist: every operation waits unless it is shown not to.
//! The matches have no wildcard, so a new operation or a new native primitive
//! must be classified before it compiles, and a native primitive that reaches
//! the operating system counts as a wait unless it is listed in
//! [`never_blocks`].
use crate::channel::{ChannelOp, SelectMode};
use crate::collection::CollectionOp;
use crate::control::ControlOp;
use crate::effects::Effect;
use crate::executor::ExecutorOp;
use crate::filesystem::FsOp;
use crate::mutex::MutexOp;
use crate::net::NetOp;
use crate::op::Op;
use crate::stream::StreamOp;
use crate::threading::ThreadOp;

/// Whether `op` can wait for another thread. Calls to named functions do not:
/// the caller inspects their bodies separately.
pub(super) fn blocks(op: &Op) -> bool {
    match op {
        Op::Channel(op) => match op {
            ChannelOp::Send(_, nowait) | ChannelOp::Recv(_, nowait) => !nowait,
            ChannelOp::Select(_, _, mode) => *mode != SelectMode::Nowait,
            ChannelOp::SendTimeout(_) | ChannelOp::RecvTimeout(_) => true,
            ChannelOp::New(_) => false,
        },
        Op::Mutex(op) => match op {
            MutexOp::Lock(_) => true,
            MutexOp::Unlock(_) | MutexOp::Exclusive(_) | MutexOp::IntoInner(_) => false,
        },
        Op::Executor(op) => match op {
            ExecutorOp::Submit(_, nowait) => !nowait,
            ExecutorOp::Map(..)
            | ExecutorOp::Reduce(..)
            | ExecutorOp::Result(_)
            | ExecutorOp::WaitTimeout(_)
            | ExecutorOp::Shutdown => true,
            ExecutorOp::New
            | ExecutorOp::Done(_)
            | ExecutorOp::Cancel(_)
            | ExecutorOp::CancelPending
            | ExecutorOp::Submitter
            | ExecutorOp::Workers => false,
        },
        Op::Control(op) => match op {
            ControlOp::Wait | ControlOp::WaitTimeout => true,
            ControlOp::New | ControlOp::Cancel | ControlOp::IsCancelled => false,
        },
        // A join waits for the task; the task's own waits are inspected
        // through its worker, but a join counts so that no wait hides
        // behind it.
        Op::Thread(op) => match op {
            ThreadOp::Join(_) | ThreadOp::Finish(..) => true,
            ThreadOp::Start(..) | ThreadOp::Run(_) | ThreadOp::Forget(_) | ThreadOp::Rows(..) => {
                false
            }
        },
        Op::Collection(op) => collection_blocks(op),
        // A foreign function can do anything, and a call through a value has
        // no body to inspect.
        Op::ForeignCall { .. }
        | Op::CallIndirect { .. }
        | Op::TailCallIndirect { .. }
        | Op::BoxedCallableCall { .. }
        | Op::ProtocolBoxCall { .. } => true,
        Op::ClosureCall(closure) => closure.name.is_empty(),
        // Memory, control flow, arithmetic, and calls to named functions.
        Op::Atomic { .. }
        | Op::ClosureNew { .. }
        | Op::BoxedCallableNew { .. }
        | Op::ProtocolBoxNew { .. }
        | Op::FunctionAddress { .. }
        | Op::Try { .. }
        | Op::Class { .. }
        | Op::BorrowLocal { .. }
        | Op::ReadRef { .. }
        | Op::Reborrow { .. }
        | Op::WriteRef { .. }
        | Op::FillRef { .. }
        | Op::ReplaceRef { .. }
        | Op::Loan { .. }
        | Op::UseLoan { .. }
        | Op::EndLoan { .. }
        | Op::Access { .. }
        | Op::Site { .. }
        | Op::CallerSite { .. }
        | Op::Returning { .. }
        | Op::Leaving
        | Op::Building { .. }
        | Op::Passes { .. }
        | Op::Forwards { .. }
        | Op::Tokens { .. }
        | Op::Yield { .. }
        | Op::Next { .. }
        | Op::MoveLocal { .. }
        | Op::DropLocal { .. }
        | Op::EmptyLocal { .. }
        | Op::FieldMove { .. }
        | Op::Enum { .. }
        | Op::Box { .. }
        | Op::Split { .. }
        | Op::SplitClass { .. }
        | Op::Unreachable
        | Op::Numeric { .. }
        | Op::CheckedBlock { .. }
        | Op::CheckedFunction { .. }
        | Op::CheckedArith { .. }
        | Op::Assumed { .. }
        | Op::Loop { .. }
        | Op::If { .. }
        | Op::PushInt { .. }
        | Op::PushFloat { .. }
        | Op::PushUnit
        | Op::PipelineToken { .. }
        | Op::FloatNeg
        | Op::PushStr { .. }
        | Op::PushBytes { .. }
        | Op::PushBool { .. }
        | Op::Add
        | Op::Sub
        | Op::Mul
        | Op::Div
        | Op::FloorDiv { .. }
        | Op::Modulo { .. }
        | Op::Integer { .. }
        | Op::Eq
        | Op::Lt
        | Op::Gt
        | Op::Not
        | Op::Ne
        | Op::Le
        | Op::Ge
        | Op::Drop
        | Op::Dup
        | Op::Swap
        | Op::DefineFn { .. }
        | Op::Call { .. }
        | Op::ForeignNull { .. }
        | Op::TailCall { .. }
        | Op::ForwardsReferences
        | Op::Return
        | Op::Break
        | Op::Continue
        | Op::LoadLocal { .. }
        | Op::StoreLocal { .. }
        | Op::Match { .. }
        | Op::Cast { .. }
        | Op::CheckedCast { .. }
        | Op::TryCast { .. }
        | Op::RangeLength { .. }
        | Op::RangeParts { .. }
        | Op::Contains => false,
    }
}

/// A native primitive waits when it reaches a handle, path, child, socket, or
/// other shared process state, unless [`never_blocks`] lists it. A file can
/// be a socket, pipe, or FIFO whose other end is another thread, and opening
/// a FIFO waits for one.
fn collection_blocks(op: &CollectionOp) -> bool {
    match op.effect() {
        // A sleep ends without another thread.
        Effect::Compute | Effect::Clock | Effect::Entropy | Effect::Environment => false,
        // The other end of a standard stream is outside the program.
        Effect::ProcessStream => false,
        Effect::Path
        | Effect::Handle
        | Effect::Child
        | Effect::Network
        | Effect::ProcessGlobal
        | Effect::SignalDisposition => !never_blocks(op),
    }
}

/// Handle, path, and socket primitives that return without waiting for any
/// other thread or process.
fn never_blocks(op: &CollectionOp) -> bool {
    matches!(
        op,
        // A zero-timeout readiness poll, a cursor move, and state queries.
        CollectionOp::Stream(StreamOp::Poll | StreamOp::FileSeek)
            | CollectionOp::FileClose
            | CollectionOp::FileClosed
            | CollectionOp::FileTell
            | CollectionOp::FileSeek
            | CollectionOp::FileReadable
            | CollectionOp::FileWritable
            // Root reads open with `O_NONBLOCK` and reject non-regular files;
            // exclusive creation cannot open a FIFO; a lock file opens with
            // `O_NONBLOCK`, and a lock attempt does not wait for the holder.
            | CollectionOp::Filesystem(
                FsOp::Close
                    | FsOp::Closed
                    | FsOp::ReadFile
                    | FsOp::CreateFile
                    | FsOp::LockOpen
                    | FsOp::LockTry
            )
            | CollectionOp::Net(
                NetOp::Address
                    | NetOp::ReadyNumber
                    | NetOp::SetOption
                    | NetOp::SelectorOpen
                    | NetOp::SelectorWatch
            )
    )
}
