//! Socket primitives behind `std.net`. Each is one OS request on a socket
//! descriptor that a `File` owner holds. Address parsing, deadlines,
//! cancellation, retries and error naming live in `std.net`; see
//! book/src/design/stdlib/net.md.
//!
//! The readiness primitives (`__ready_*`) back `std.ready`: a wait that a
//! cancellation token ends directly, and epoll selectors owned by a `File`.
//!
//! An address crosses the boundary as `list[u8]`: the family (4 or 6), the
//! port in network order, the address bytes and, for IPv6, the scope id in
//! network order. That is 7 or 23 bytes.
use crate::effects::Effect;
use crate::{op::Ty, sum};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NetOp {
    Open,
    Accept,
    FinishConnect,
    Address,
    Shutdown,
    SetOption,
    SendTo,
    ReceiveFrom,
    Resolve,
    ReadyWait,
    SelectorOpen,
    SelectorWatch,
    Select,
    ReadyNumber,
    // The operations above reading their `source` operand from a `bytes`
    // literal, which the runtime tells from a buffer by its header.
    SendToLiteral,
}

impl NetOp {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__net_open" => Self::Open,
            "__net_accept" => Self::Accept,
            "__net_finish_connect" => Self::FinishConnect,
            "__net_address" => Self::Address,
            "__net_shutdown" => Self::Shutdown,
            "__net_set_option" => Self::SetOption,
            "__net_send_to" => Self::SendTo,
            "__net_receive_from" => Self::ReceiveFrom,
            "__net_resolve" => Self::Resolve,
            "__ready_wait" => Self::ReadyWait,
            "__ready_selector" => Self::SelectorOpen,
            "__ready_watch" => Self::SelectorWatch,
            "__ready_select" => Self::Select,
            "__ready_number" => Self::ReadyNumber,
            _ => return None,
        })
    }

    /// The operand the primitive only reads bytes from, which a `Bytes`
    /// buffer or a `bytes` literal can supply.
    pub fn source(self) -> Option<usize> {
        match self.buffer_form() {
            Self::SendTo => Some(0),
            _ => None,
        }
    }

    /// The variant reading `source` from a `bytes` literal.
    pub fn literal(self) -> Option<Self> {
        Some(match self {
            Self::SendTo => Self::SendToLiteral,
            _ => return None,
        })
    }

    fn buffer_form(self) -> Self {
        match self {
            Self::SendToLiteral => Self::SendTo,
            other => other,
        }
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
        let form = self.buffer_form();
        if form != self {
            let (mut inputs, output) = form.signature();
            inputs[form.source().unwrap()] = Ty::ByteStr;
            return (inputs, output);
        }
        let address = || Ty::List(Arc::new(Ty::U8));
        let range = || vec![Ty::ByteStorage, Ty::I64, Ty::I64, Ty::File];
        let (inputs, output) = match self {
            // Address, socket kind, listen backlog.
            Self::Open => (vec![address(), Ty::U8, Ty::I64], Ty::File),
            Self::Accept => (vec![Ty::File], Ty::File),
            Self::FinishConnect => (vec![Ty::File], Ty::I64),
            // 0 is the local address, 1 the peer's.
            Self::Address => (vec![Ty::File, Ty::U8], address()),
            Self::Shutdown => (vec![Ty::File, Ty::U8], Ty::I64),
            Self::SetOption => (vec![Ty::File, Ty::U8, Ty::I64], Ty::I64),
            Self::SendTo => ([range(), vec![address()]].concat(), Ty::I64),
            // The sender's address is written into the last buffer.
            Self::ReceiveFrom => ([range(), vec![Ty::ByteStorage]].concat(), Ty::I64),
            // A socket carrying the reply of a lookup on a helper thread.
            Self::Resolve => (vec![Ty::Str], Ty::File),
            // Handle, interest, timeout in nanoseconds; readiness bits.
            Self::ReadyWait => (
                vec![Ty::File, Ty::U8, Ty::U64, Ty::CancellationToken],
                Ty::I64,
            ),
            Self::SelectorOpen => (vec![], Ty::File),
            // Selector, handle, key, interest (0 removes).
            Self::SelectorWatch => (vec![Ty::File, Ty::File, Ty::U64, Ty::U8], Ty::I64),
            // Event buffer, selector, timeout; the number of events written.
            Self::Select => (
                vec![Ty::ByteStorage, Ty::File, Ty::U64, Ty::CancellationToken],
                Ty::I64,
            ),
            // The handle's descriptor number, which selector keys can hold to
            // tell handles apart.
            Self::ReadyNumber => (vec![Ty::File], Ty::I64),
            Self::SendToLiteral => {
                unreachable!("a literal variant defers to its buffer form")
            }
        };
        (inputs, sum::result(output, sum::io_error()))
    }

    /// Operands the primitive writes through or whose socket state it
    /// changes. Accepting and reading addresses share a listener, so several
    /// workers can accept from one borrowed listener.
    pub fn exclusive(self, operand: usize) -> bool {
        let form = self.buffer_form();
        if form != self {
            return form.exclusive(operand);
        }
        match self {
            Self::FinishConnect | Self::Shutdown | Self::SetOption | Self::SelectorWatch => {
                operand == 0
            }
            Self::Select => operand == 0,
            Self::SendTo => operand == 3,
            Self::ReceiveFrom => matches!(operand, 0 | 3 | 4),
            Self::Open
            | Self::Accept
            | Self::Address
            | Self::Resolve
            | Self::ReadyWait
            | Self::SelectorOpen
            | Self::ReadyNumber => false,
            Self::SendToLiteral => {
                unreachable!("a literal variant defers to its buffer form")
            }
        }
    }

    pub fn opcode(self) -> i64 {
        let form = self.buffer_form();
        if form != self {
            return form.opcode();
        }
        410 + self as i64
    }

    pub fn effect(self) -> Effect {
        let form = self.buffer_form();
        if form != self {
            return form.effect();
        }
        match self {
            Self::Open | Self::Resolve => Effect::Network,
            Self::Accept
            | Self::FinishConnect
            | Self::Address
            | Self::Shutdown
            | Self::SetOption
            | Self::SendTo
            | Self::ReceiveFrom
            | Self::ReadyWait
            | Self::SelectorOpen
            | Self::SelectorWatch
            | Self::Select
            | Self::ReadyNumber => Effect::Handle,
            Self::SendToLiteral => {
                unreachable!("a literal variant defers to its buffer form")
            }
        }
    }
}
