//! `if const` conditions: a closed set of facts about the compilation target,
//! decided while parsing. Only the selected branch reaches name resolution
//! and type checking, so the others may name declarations or modules that
//! exist only on another target. See
//! `book/src/design/language/02-source/target-conditionals.md`.
use super::*;

/// A target fact's value.
#[derive(Clone, Copy, PartialEq)]
enum Fact {
    Text(&'static str),
    Number(u64),
}

/// Each fact with the values the compiler recognizes for it. A comparison
/// with any other value is rejected, so a misspelling cannot silently select
/// the `else` branch.
const FACTS: &[(&str, &[Fact])] = &[
    (
        "os",
        &[
            Fact::Text("linux"),
            Fact::Text("macos"),
            Fact::Text("windows"),
            Fact::Text("freebsd"),
        ],
    ),
    ("arch", &[Fact::Text("x86_64"), Fact::Text("aarch64")]),
    ("pointer_width", &[Fact::Number(32), Fact::Number(64)]),
    ("endian", &[Fact::Text("little"), Fact::Text("big")]),
];

/// The value of fact `name` for the compilation target. Plenty compiles only
/// for its native target (see `toolchain::validate_target`), so the facts
/// are the packaged runtime's.
fn fact(name: &str) -> Option<Fact> {
    let target = crate::toolchain::target_description(crate::toolchain::native_target())?;
    Some(match name {
        "os" => Fact::Text("linux"),
        "arch" => Fact::Text(target.arch),
        "pointer_width" => Fact::Number(64),
        "endian" => Fact::Text("little"),
        _ => return None,
    })
}

fn known(name: &str) -> Option<&'static [Fact]> {
    FACTS.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

fn describe(values: &[Fact]) -> String {
    values
        .iter()
        .map(|v| match v {
            Fact::Text(t) => format!("\"{t}\""),
            Fact::Number(n) => n.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Evaluates an `if const` condition: `target.KEY == VALUE` or `!=`,
/// combined with `and`, `or`, `not`, parentheses, `True` and `False`.
pub(super) fn condition(e: &Expr) -> Result<bool> {
    match &e.kind {
        Expression::Group(inner) => condition(inner),
        Expression::Bool(b) => Ok(*b),
        Expression::Unary(op, inner) if op == "not" => Ok(!condition(inner)?),
        Expression::Binary(op, a, b) if op == "and" => Ok(condition(a)? && condition(b)?),
        Expression::Binary(op, a, b) if op == "or" => Ok(condition(a)? || condition(b)?),
        Expression::Binary(op, a, b) if op == "==" || op == "!=" => {
            let (name, value) = match (key(a)?, key(b)?) {
                (Some(name), None) => (name, b),
                (None, Some(name)) => (name, a),
                _ => {
                    return Err(e.at.error(
                        "an `if const` comparison compares one `target` fact with a literal",
                    ));
                }
            };
            let values = known(name).unwrap();
            let value = match &ungroup(value).kind {
                Expression::Text(t) => values
                    .iter()
                    .find(|v| matches!(v, Fact::Text(x) if *x == t.as_str())),
                Expression::Number(n) => n
                    .parse()
                    .ok()
                    .and_then(|n| values.iter().find(|v| **v == Fact::Number(n))),
                _ => {
                    return Err(value
                        .at
                        .error("an `if const` comparison needs a string or integer literal"));
                }
            };
            let Some(value) = value else {
                return Err(e.at.error(format!(
                    "unknown value for `target.{name}`; known values: {}",
                    describe(values)
                )));
            };
            Ok((fact(name) == Some(*value)) == (op == "=="))
        }
        _ => Err(e.at.error(
            "an `if const` condition compares `target` facts with `==` or `!=`, joined by `and`, `or`, and `not`",
        )),
    }
}

/// The fact named by `target.KEY`, or `None` for anything that is not a
/// `target` member.
fn key(e: &Expr) -> Result<Option<&'static str>> {
    let e = ungroup(e);
    let Expression::Member(base, name, _) = &e.kind else {
        return Ok(None);
    };
    if !matches!(&base.kind, Expression::Name(n) if n == "target") {
        return Ok(None);
    }
    match FACTS.iter().find(|(n, _)| n == name) {
        Some((n, _)) => Ok(Some(n)),
        None => Err(e.at.error(format!(
            "unknown target fact `target.{name}`; known facts: {}",
            FACTS
                .iter()
                .map(|(n, _)| format!("`target.{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

impl Parser {
    /// Whether the next tokens begin an `if const` chain.
    pub(super) fn at_const_if(&self) -> bool {
        self.peek().is("if") && self.token(self.pos + 1).is_some_and(|t| t.is("const"))
    }

    /// Parses one `if const` / `elif const` head or an `else`, up to its
    /// `:`. Returns whether the arm's condition holds (`else` holds) and
    /// whether the arm is the `else`. `first` is true for the `if`.
    pub(super) fn const_arm(&mut self, first: bool) -> Result<(bool, bool)> {
        let at = self.take(); // if, elif, or else
        if at.is("else") && !first {
            return Ok((true, true));
        }
        if !self.eat("const") {
            return Err(at.error(if first {
                "expected `if const`"
            } else {
                "an `elif` in an `if const` chain is written `elif const`"
            }));
        }
        let condition = self.expr(0)?;
        Ok((self::condition(&condition)?, false))
    }

    /// An `if const` chain in a function body: the statements of the
    /// selected arm, which take the chain's place in the enclosing suite.
    /// The other arms are parsed and discarded.
    pub(super) fn const_statements(&mut self) -> Result<Vec<Stmt>> {
        let mut chosen: Option<Vec<Stmt>> = None;
        let mut first = true;
        loop {
            let (holds, else_arm) = self.const_arm(first)?;
            let body = self.suite()?;
            if holds && chosen.is_none() {
                chosen = Some(body);
            }
            first = false;
            if else_arm || !(self.peek().is("elif") || self.peek().is("else")) {
                break;
            }
        }
        Ok(chosen.unwrap_or_default())
    }
}
