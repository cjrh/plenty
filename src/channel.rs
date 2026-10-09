//! Typed bounded-channel operations. Endpoint sharing is explicit; messages move.
use crate::op::Ty;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum ChannelOp {
    New(Ty),
    Send(Ty, bool),
    Recv(Ty, bool),
    SendTimeout(Ty),
    RecvTimeout(Ty),
    Select(Ty, Ty, SelectMode),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectMode {
    Wait,
    Nowait,
    Timeout,
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
            Self::SendTimeout(t) => (
                vec![Ty::Channel(Rc::new(t.clone()), true), t.clone(), Ty::U64],
                crate::sum::result(Ty::Unit, crate::sum::send_timeout_error(t.clone())),
            ),
            Self::RecvTimeout(t) => (
                vec![Ty::Channel(Rc::new(t.clone()), false), Ty::U64],
                crate::sum::result(t.clone(), crate::sum::recv_timeout_error()),
            ),
            Self::Select(first, second, mode) => {
                let mut inputs = vec![
                    Ty::Channel(Rc::new(first.clone()), false),
                    Ty::Channel(Rc::new(second.clone()), false),
                ];
                if *mode == SelectMode::Timeout {
                    inputs.push(Ty::U64);
                }
                (
                    inputs,
                    crate::sum::result(
                        crate::sum::selected(first.clone(), second.clone()),
                        crate::sum::select_error(),
                    ),
                )
            }
        }
    }
    pub fn opcode(&self) -> i64 {
        match self {
            Self::New(_) => 0,
            Self::Send(_, false) => 1,
            Self::Send(_, true) => 2,
            Self::Recv(_, false) => 3,
            Self::Recv(_, true) => 4,
            Self::SendTimeout(_) => 5,
            Self::RecvTimeout(_) => 6,
            Self::Select(_, _, SelectMode::Wait) => 7,
            Self::Select(_, _, SelectMode::Nowait) => 8,
            Self::Select(_, _, SelectMode::Timeout) => 9,
        }
    }
}
