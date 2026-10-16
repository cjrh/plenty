//! Socket primitives behind `std.net`. Each is one OS request on a socket
//! descriptor that a `File` owner holds. Address parsing, deadlines,
//! cancellation, retries and error naming live in `std.net`; see
//! book/src/design/stdlib/net.md.
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
            _ => return None,
        })
    }

    pub fn signature(self) -> (Vec<Ty>, Ty) {
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
            // Concatenated addresses with port zero; empty when the name has none.
            Self::Resolve => (vec![Ty::Str], address()),
        };
        (inputs, sum::result(output, sum::io_error()))
    }

    /// Operands the primitive writes through or whose socket state it
    /// changes. Accepting and reading addresses share a listener, so several
    /// workers can accept from one borrowed listener.
    pub fn exclusive(self, operand: usize) -> bool {
        match self {
            Self::FinishConnect | Self::Shutdown | Self::SetOption => operand == 0,
            Self::SendTo => operand == 3,
            Self::ReceiveFrom => matches!(operand, 0 | 3 | 4),
            Self::Open | Self::Accept | Self::Address | Self::Resolve => false,
        }
    }

    pub fn opcode(self) -> i64 {
        410 + self as i64
    }

    pub fn effect(self) -> Effect {
        match self {
            Self::Open | Self::Resolve => Effect::Network,
            Self::Accept
            | Self::FinishConnect
            | Self::Address
            | Self::Shutdown
            | Self::SetOption
            | Self::SendTo
            | Self::ReceiveFrom => Effect::Handle,
        }
    }
}
