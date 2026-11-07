//! Explicit C argument adaptation, shared by checking and native lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argument {
    Direct,
    /// Borrow UTF-8 bytes as (const uint8_t *, size_t) for this call only.
    Utf8,
    /// Validate NULs and allocate a temporary terminated byte buffer.
    CString,
    /// Borrow numeric list elements or a shared byte view as (data, length,
    /// byte stride). Views have stride 1. A mutable list loan permits scalar
    /// writes only; no form permits retention/resizing.
    Strided,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Symbol(String),
    /// A trusted opaque function address passed as a Plenty-only parameter.
    Parameter(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    pub target: Target,
    pub arguments: Vec<Argument>,
    /// The binding author's `@thread_safe` promise that the C function may
    /// run on any thread, concurrently with any call. Worker certification
    /// trusts it; without it a reachable call rejects the worker. See
    /// book/src/design/facilities/04-interop/trusted-declarations.md#worker-threads.
    pub thread_safe: bool,
}

impl Declaration {
    pub fn symbol(&self) -> Option<&str> {
        match &self.target {
            Target::Symbol(name) => Some(name),
            Target::Parameter(_) => None,
        }
    }

    pub fn fallible(&self) -> bool {
        self.arguments.contains(&Argument::CString)
    }

    /// The declared Plenty return wraps the C return when conversion can fail.
    /// Called only after the frontend has validated the adapter contract.
    pub fn output<'a>(&self, sig: &'a crate::op::FnSig) -> Option<&'a crate::op::Ty> {
        use crate::op::Ty;
        if !self.fallible() {
            return sig.outputs.first();
        }
        let Ty::Enum(t) = &sig.outputs[0] else {
            unreachable!("checked C-string result")
        };
        let payload = &t.local().variants[0].fields[0];
        (payload != &Ty::Unit).then_some(payload)
    }
}
