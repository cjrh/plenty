//! Declaration annotations: compiler-known facts written alone on the lines
//! above a declaration, such as `@copy`, `@newtype`, `@redacted`,
//! `@thread_safe`, `@host_callback`, `@thread_movable` or `@thread_shareable`. The set
//! is closed; there are no user decorators. `@serial.name("wire")` is the one
//! member annotation and the one that takes an argument. Names live after `@`, outside the
//! value namespace, so `@copy` does not reserve the builtin `copy` function's
//! name. See
//! book/src/design/language/03-types/classes.md#copy-classes,
//! book/src/design/language/03-types/classes.md#redacted-types,
//! book/src/design/language/04-expressions/operators.md#newtype-forwarding and
//! book/src/design/facilities/04-interop/trusted-declarations.md#worker-threads.
use super::*;

pub(super) const WIRE_NAME_PLACE: &str =
    "`@serial.name` applies only to fields of an `@serial` class and variants of an `@serial` enum";

/// An explicit wire name: `@serial.name("wire")` above a field or variant.
/// Derived hooks encode and decode the member under this name instead of its
/// source name; see book/src/design/stdlib/serial-derive.md#wire-names.
#[derive(Clone)]
pub(super) struct WireName {
    pub(super) at: Token,
    pub(super) name: String,
}

/// One known annotation. Later annotations, such as the agreed `@final` and
/// `@protocol.Name`, extend this set.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Annotation {
    Serial,
    Copy,
    Newtype,
    Redacted,
    ThreadSafe,
    HostCallback,
    ThreadMovable,
    ThreadShareable,
}

impl Annotation {
    fn named(name: &str) -> Option<Self> {
        match name {
            "serial" => Some(Self::Serial),
            "copy" => Some(Self::Copy),
            "newtype" => Some(Self::Newtype),
            "redacted" => Some(Self::Redacted),
            "thread_safe" => Some(Self::ThreadSafe),
            "host_callback" => Some(Self::HostCallback),
            "thread_movable" => Some(Self::ThreadMovable),
            "thread_shareable" => Some(Self::ThreadShareable),
            _ => None,
        }
    }
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Serial => "serial",
            Self::Copy => "copy",
            Self::Newtype => "newtype",
            Self::Redacted => "redacted",
            Self::ThreadSafe => "thread_safe",
            Self::HostCallback => "host_callback",
            Self::ThreadMovable => "thread_movable",
            Self::ThreadShareable => "thread_shareable",
        }
    }
    /// The declarations this annotation applies to, for diagnostics.
    pub(super) fn targets(self) -> &'static str {
        match self {
            Self::Redacted | Self::Serial => "class and enum declarations",
            Self::Copy | Self::Newtype => "class declarations",
            Self::ThreadSafe => "`extern def` and `export def` declarations",
            Self::HostCallback => "indirect `extern def` declarations",
            Self::ThreadMovable | Self::ThreadShareable => "`opaque` declarations",
        }
    }
}

impl Parser {
    /// Parse the annotation lines at the parser's position. Each is `@name`
    /// alone on its line; an unknown name, arguments, or a repeat is an error.
    pub(super) fn annotations(&mut self) -> Result<Vec<(Token, Annotation)>> {
        let mut annotations = Vec::new();
        while self.peek().is("@") {
            let at = self.take();
            let name = self.take();
            let Kind::Word(spelling) = &name.kind else {
                return Err(name.error("expected an annotation name after `@`, such as `@copy`"));
            };
            if spelling.as_ref() == "serial" && self.peek().is(".") {
                return Err(at.error(WIRE_NAME_PLACE));
            }
            let Some(annotation) = Annotation::named(spelling) else {
                return Err(name.error(format!(
                    "unknown annotation `@{spelling}`; the annotations are `@copy`, `@newtype`, `@redacted`, `@thread_safe`, `@host_callback`, `@thread_movable`, `@thread_shareable` and `@serial`"
                )));
            };
            if self.peek().kind != Kind::Newline {
                return Err(self.peek().error(format!(
                    "`@{}` takes no arguments; write it alone on the line above the declaration",
                    annotation.name()
                )));
            }
            self.take();
            if annotations.iter().any(|(_, a)| *a == annotation) {
                return Err(at.error(format!("duplicate annotation `@{}`", annotation.name())));
            }
            annotations.push((at, annotation));
        }
        Ok(annotations)
    }

    /// Parse an optional `@serial.name("wire")` line above a class field or
    /// enum variant: the one member annotation. Whether the member and its
    /// owner accept it is checked by the caller and by `serial_derive`.
    pub(super) fn wire_name(&mut self) -> Result<Option<WireName>> {
        let is_wire = |p: &Self| {
            p.peek().is("@")
                && p.token(p.pos + 1).is_some_and(|t| t.is("serial"))
                && p.token(p.pos + 2).is_some_and(|t| t.is("."))
        };
        if !is_wire(self) {
            return Ok(None);
        }
        let at = self.take();
        self.take();
        self.take();
        if !self.eat("name") {
            return Err(self.peek().error(
                "expected `name` after `@serial.`; the member annotation is `@serial.name(\"wire name\")`",
            ));
        }
        self.expect("(")?;
        let Kind::Text(name) = self.peek().kind.clone() else {
            return Err(self.peek().error(
                "`@serial.name` takes one string literal, such as `@serial.name(\"user_id\")`",
            ));
        };
        self.take();
        if !self.peek().is(")") {
            return Err(self.peek().error(
                "`@serial.name` takes one string literal, such as `@serial.name(\"user_id\")`",
            ));
        }
        self.take();
        if self.peek().kind != Kind::Newline {
            return Err(self
                .peek()
                .error("write `@serial.name(...)` alone on the line above the field or variant"));
        }
        self.take();
        if name.is_empty() {
            return Err(at.error("`@serial.name` needs a nonempty name"));
        }
        if is_wire(self) {
            return Err(self
                .peek()
                .error("duplicate `@serial.name`; a member has one wire name"));
        }
        // The next line must declare a field or variant, possibly after `pub`
        // or another annotation, which `misplaced_annotations` then reports.
        let mut next = self.pos;
        if self.peek().is("pub") {
            next += 1;
        }
        let member = self.token(next).is_some_and(|t| match &t.kind {
            Kind::Word(word) => !reserved(word),
            _ => t.is("@"),
        });
        if !member {
            return Err(at.error(WIRE_NAME_PLACE));
        }
        Ok(Some(WireName {
            at,
            name: name.to_string(),
        }))
    }

    /// Reject annotations where no declaration accepts them, after checking
    /// that each is well formed.
    pub(super) fn misplaced_annotations(&mut self, place: &str) -> Result<()> {
        // Report the first known annotation before parsing the rest, so one
        // stacked above `@serial.name` is named rather than the wire name.
        if self.peek().is("@")
            && let Some(annotation) = self
                .token(self.pos + 1)
                .and_then(|t| t.text())
                .and_then(Annotation::named)
            && !self.token(self.pos + 2).is_some_and(|t| t.is("."))
        {
            return Err(self.peek().error(format!(
                "`@{}` applies only to {}, not {place}",
                annotation.name(),
                annotation.targets()
            )));
        }
        if let Some((at, annotation)) = self.annotations()?.into_iter().next() {
            return Err(at.error(format!(
                "`@{}` applies only to {}, not {place}",
                annotation.name(),
                annotation.targets()
            )));
        }
        Ok(())
    }
}
