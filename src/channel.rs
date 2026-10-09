//! Typed bounded-channel operations. Endpoint sharing is explicit; messages move.
use crate::op::Ty;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum ChannelOp {
    New(Ty),
    Send(Ty, bool),
    Recv(Ty, bool),
}

impl ChannelOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        match self {
            Self::New(t) => (
                vec![Ty::U64],
                crate::sum::result(
                    crate::sum::tuple(vec![
                        Ty::Channel(Rc::new(t.clone()), true),
                        Ty::Channel(Rc::new(t.clone()), false),
                    ]),
                    crate::sum::channel_error(),
                ),
            ),
            Self::Send(t, _) => (
                vec![Ty::Channel(Rc::new(t.clone()), true), t.clone()],
                crate::sum::result(Ty::Unit, crate::sum::send_error(t.clone())),
            ),
            Self::Recv(t, _) => (
                vec![Ty::Channel(Rc::new(t.clone()), false)],
                crate::sum::result(t.clone(), crate::sum::recv_error()),
            ),
        }
    }
    pub fn opcode(&self) -> i64 {
        match self {
            Self::New(_) => 0,
            Self::Send(_, false) => 1,
            Self::Send(_, true) => 2,
            Self::Recv(_, false) => 3,
            Self::Recv(_, true) => 4,
        }
    }
}
