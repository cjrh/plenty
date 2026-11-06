//! Compiler diagnostics as values. A diagnostic keeps its position apart from
//! its text, so the editor reads positions without parsing rendered errors and
//! a generic instance's body error can move to the use that required it. It
//! displays as the command line prints it: `path:line:column: message`.

use std::error::Error;
use std::fmt;
use std::sync::Arc;

/// A position in source. Lines and columns count from one. `file` is absent
/// for source compiled from a string rather than read from a file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Position {
    pub file: Option<Arc<str>>,
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}:")?;
        }
        write!(f, "{}:{}", self.line, self.column)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Diagnostic {
    /// Absent for an error that no source position explains.
    pub position: Option<Position>,
    /// The text after the position, including any notes on later lines.
    pub message: String,
    /// The error is already reported at the use that required its generic
    /// instance. Lowering one function can lower another on demand; this
    /// stops the outer function from attributing the inner one's error to its
    /// own instantiation.
    pub attributed: bool,
    /// What the rejected operation needed from a value's type, which an
    /// instance's caller may have supplied as a type argument (issue #518).
    pub demand: Option<Box<crate::ownership::Demand>>,
}

impl Diagnostic {
    pub fn new(position: Option<Position>, message: impl Into<String>) -> Self {
        Diagnostic {
            position,
            message: message.into(),
            attributed: false,
            demand: None,
        }
    }

    /// The diagnostic `error` carries. Any other error becomes a diagnostic
    /// without a position: its text is never searched for one.
    pub fn of(error: &(dyn Error + 'static)) -> Diagnostic {
        if let Some(diagnostic) = error.downcast_ref::<Diagnostic>() {
            return diagnostic.clone();
        }
        if let Some(error) = error.downcast_ref::<crate::ownership::Error>() {
            return error.diagnostic();
        }
        Diagnostic::new(None, error.to_string())
    }

    /// `error` as a diagnostic, without copying one that already is.
    pub fn from_boxed(error: Box<dyn Error>) -> Box<Diagnostic> {
        error
            .downcast::<Diagnostic>()
            .unwrap_or_else(|error| Box::new(Diagnostic::of(&*error)))
    }

    /// Whether two recorded diagnostics report the same failure: the same
    /// position and text. The same wording at two sites is two failures.
    pub fn same(&self, other: &Diagnostic) -> bool {
        self.position == other.position && self.message == other.message
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(position) = &self.position {
            write!(f, "{position}: ")?;
        }
        f.write_str(&self.message)
    }
}

impl Error for Diagnostic {}

/// A check that reports code it accepts. Lints never reject a program or
/// change its exit status; each has a stable name that output shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Lint {
    /// `copy(value)?` of a value whose type already copies on use.
    RedundantCopy,
}

impl Lint {
    /// The stable name, as output and editors show it.
    pub fn name(self) -> &'static str {
        match self {
            Lint::RedundantCopy => "redundant-copy",
        }
    }
}

impl fmt::Display for Lint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A lint's report at a position. It displays as the command line prints
/// it: `path:line:column: warning: message [name]`.
#[derive(Clone, Debug)]
pub struct Warning {
    pub(crate) lint: Lint,
    pub(crate) diagnostic: Diagnostic,
}

impl Warning {
    pub(crate) fn new(lint: Lint, position: Position, message: impl Into<String>) -> Self {
        Warning {
            lint,
            diagnostic: Diagnostic::new(Some(position), message),
        }
    }

    pub fn lint(&self) -> Lint {
        self.lint
    }

    /// The text after the position, without the lint name.
    pub fn message(&self) -> &str {
        &self.diagnostic.message
    }

    /// The order output lists warnings in: by file, then position.
    fn key(&self) -> (Option<&str>, u32, u32, Lint) {
        let position = self.diagnostic.position.as_ref();
        (
            position.and_then(|p| p.file.as_deref()),
            position.map_or(0, |p| p.line),
            position.map_or(0, |p| p.column),
            self.lint,
        )
    }
}

/// Sort `warnings` into output order and drop repeats: a body lowered twice
/// reports each warning once.
pub(crate) fn settle(warnings: &mut Vec<Warning>) {
    warnings.sort_by(|a, b| {
        a.key()
            .cmp(&b.key())
            .then_with(|| a.diagnostic.message.cmp(&b.diagnostic.message))
    });
    warnings.dedup_by(|a, b| a.lint == b.lint && a.diagnostic.same(&b.diagnostic));
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(position) = &self.diagnostic.position {
            write!(f, "{position}: ")?;
        }
        write!(f, "warning: {} [{}]", self.diagnostic.message, self.lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(file: Option<&str>, line: u32, column: u32) -> Option<Position> {
        Some(Position {
            file: file.map(Arc::from),
            line,
            column,
        })
    }

    #[test]
    fn a_diagnostic_displays_as_the_command_line_prints_it() {
        let found = Diagnostic::new(at(Some("/a b/c:d.plenty"), 3, 7), "expected i64");
        assert_eq!(found.to_string(), "/a b/c:d.plenty:3:7: expected i64");
        assert_eq!(Diagnostic::new(at(None, 2, 1), "x").to_string(), "2:1: x");
        assert_eq!(
            Diagnostic::new(None, "reading x: missing").to_string(),
            "reading x: missing"
        );
    }

    #[test]
    fn other_errors_have_no_position_even_when_their_text_looks_like_one() {
        let error: Box<dyn Error> = "/a/b.plenty:3:7: expected i64".into();
        let found = Diagnostic::from_boxed(error);
        assert_eq!(found.position, None);
        assert_eq!(found.message, "/a/b.plenty:3:7: expected i64");
    }

    #[test]
    fn the_same_wording_at_two_sites_is_two_failures() {
        let first = Diagnostic::new(at(Some("a.plenty"), 1, 1), "unknown binding `x`");
        let again = Diagnostic::new(at(Some("a.plenty"), 1, 1), "unknown binding `x`");
        let elsewhere = Diagnostic::new(at(Some("a.plenty"), 2, 1), "unknown binding `x`");
        assert!(first.same(&again));
        assert!(!first.same(&elsewhere));
    }
}
