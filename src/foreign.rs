//! Explicit C argument adaptation, shared by checking and native lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argument {
    Direct,
    /// Borrow UTF-8 bytes as (const uint8_t *, size_t) for this call only.
    Utf8,
    /// Validate NULs and allocate a temporary terminated byte buffer.
    CString,
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
