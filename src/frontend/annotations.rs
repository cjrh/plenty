//! Declaration annotations: compiler-known facts written alone on the lines
//! above a declaration, such as `@copy`. The set is closed; there are no user
//! decorators. Names live after `@`, outside the value namespace, so `@copy`
//! does not reserve the builtin `copy` function's name. See
//! book/src/design/language/03-types/classes.md#copy-classes.
use super::*;

/// One known annotation. Later annotations, such as the agreed `@final`,
/// `@protocol.Name` and `@serial`, extend this set.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Annotation {
    Copy,
}

impl Annotation {
    fn named(name: &str) -> Option<Self> {
        match name {
            "copy" => Some(Self::Copy),
            _ => None,
        }
    }
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Copy => "copy",
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
                    "unknown annotation `@{spelling}`; the only annotation is `@copy`"
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
                "`@{}` applies only to class declarations, not {place}",
                annotation.name()
            )));
        }
        Ok(())
    }
}
