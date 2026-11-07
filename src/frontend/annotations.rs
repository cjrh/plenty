//! Declaration annotations: compiler-known facts written alone on the lines
//! above a declaration, such as `@copy`, `@newtype`, `@redacted`,
//! `@thread_safe`, `@thread_movable` or `@thread_shareable`. The set
//! is closed; there are no user decorators. Names live after `@`, outside the
//! value namespace, so `@copy` does not reserve the builtin `copy` function's
//! name. See
//! book/src/design/language/03-types/classes.md#copy-classes,
//! book/src/design/language/03-types/classes.md#redacted-types,
//! book/src/design/language/04-expressions/operators.md#newtype-forwarding and
//! book/src/design/facilities/04-interop/trusted-declarations.md#worker-threads.
use super::*;

/// One known annotation. Later annotations, such as the agreed `@final` and
/// `@protocol.Name`, extend this set.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Annotation {
    Serial,
    Copy,
    Newtype,
    Redacted,
    ThreadSafe,
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
            Self::ThreadMovable => "thread_movable",
            Self::ThreadShareable => "thread_shareable",
        }
    }
    /// The declarations this annotation applies to, for diagnostics.
    pub(super) fn targets(self) -> &'static str {
        match self {
            Self::Redacted | Self::Serial => "class and enum declarations",
            Self::Copy | Self::Newtype => "class declarations",
            Self::ThreadSafe => "`extern def` declarations",
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
            let Some(annotation) = Annotation::named(spelling) else {
                return Err(name.error(format!(
                    "unknown annotation `@{spelling}`; the annotations are `@copy`, `@newtype`, `@redacted`, `@thread_safe`, `@thread_movable`, `@thread_shareable` and `@serial`"
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

    /// Reject annotations where no declaration accepts them, after checking
    /// that each is well formed.
    pub(super) fn misplaced_annotations(&mut self, place: &str) -> Result<()> {
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
