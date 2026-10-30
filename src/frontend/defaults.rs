//! Declaration-time checks for parameter default expressions.
use super::*;

pub(super) fn validate_dependencies(
    inputs: &[(String, TypeRef)],
    defaults: &[Option<Expr>],
) -> Result<()> {
    if defaults.is_empty() {
        return Ok(());
    }
    let names: HashSet<_> = inputs.iter().map(|(name, _)| name.clone()).collect();
    for default in defaults.iter().flatten() {
        expression(default, &names)?;
    }
    Ok(())
}

fn name(value: &str, at: &Token, forbidden: &HashSet<String>) -> Result<()> {
    if forbidden.contains(value) {
        return Err(at.error(format!(
            "default expressions cannot reference parameter `{value}` or self"
        )));
    }
    Ok(())
}

fn expression(e: &Expr, forbidden: &HashSet<String>) -> Result<()> {
    match &e.kind {
        Expression::Name(n) | Expression::GenericValue(n, _) => name(n, &e.at, forbidden)?,
        Expression::Call(n, args) | Expression::GenericCall(n, _, args) => {
            name(n, &e.at, forbidden)?;
            for arg in args {
                expression(arg, forbidden)?;
            }
        }
        Expression::Tuple(args) | Expression::Constructor(_, args) => {
            for arg in args {
                expression(arg, forbidden)?;
            }
        }
        Expression::Format(format) => {
            for arg in format.operands() {
                expression(arg, forbidden)?;
            }
        }
        Expression::Member(base, _, _)
        | Expression::NamedArgument(_, base)
        | Expression::Unary(_, base)
        | Expression::Group(base)
        | Expression::Try(base)
        | Expression::ClassReady(_, base) => expression(base, forbidden)?,
        Expression::Method(base, _, args, _)
        | Expression::GenericMethod(base, _, _, args, _)
        | Expression::Invoke(base, args) => {
            expression(base, forbidden)?;
            for arg in args {
                expression(arg, forbidden)?;
            }
        }
        Expression::Index(a, b) | Expression::Binary(_, a, b) => {
            expression(a, forbidden)?;
            expression(b, forbidden)?;
        }
        Expression::Ambiguous { generic, index } => {
            expression(generic, forbidden)?;
            expression(index, forbidden)?;
        }
        Expression::Conditional { condition, yes, no } => {
            expression(condition, forbidden)?;
            expression(yes, forbidden)?;
            expression(no, forbidden)?;
        }
        Expression::Collection {
            entries, clauses, ..
        } => {
            let mut inner = forbidden.clone();
            for clause in clauses {
                match clause {
                    Clause::For(names, iterable) => {
                        expression(iterable, &inner)?;
                        for name in names {
                            inner.remove(name);
                        }
                    }
                    Clause::If(condition) => expression(condition, &inner)?,
                }
            }
            for (key, value) in entries {
                expression(key, &inner)?;
                if let Some(value) = value {
                    expression(value, &inner)?;
                }
            }
        }
        Expression::Anonymous(function) => {
            for capture in &function.captures {
                name(&capture.name, &e.at, forbidden)?;
            }
            // Defaults are written inside the parameter brackets, where
            // `inline_body` admits only a one-expression body; statements,
            // including `match`, `for`, and `with`, cannot appear here.
            let [
                Stmt {
                    kind: Statement::Expr(body),
                    ..
                },
            ] = function.body.as_slice()
            else {
                unreachable!("an anonymous function inside brackets has one expression body")
            };
            let mut inner = forbidden.clone();
            for (name, _) in &function.inputs {
                inner.remove(name);
            }
            expression(body, &inner)?;
        }
        Expression::Match(..) | Expression::Checked(_) => {
            unreachable!("a match or checked block is a statement's whole value")
        }

        Expression::ClassNew(_)
        | Expression::Type(_)
        | Expression::Number(_)
        | Expression::Text(_)
        | Expression::Bytes(_)
        | Expression::Bool(_)
        | Expression::Unit => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostic(source: &str) -> String {
        match compile(source, &mut Heap::default()) {
            Ok(_) => panic!("expected invalid default declaration"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn defaults_cannot_depend_on_any_parameter() {
        for source in [
            "def f(a: i64, b: i64 = a) -> i64:\n    b\n",
            "def f(a: i64 = b, b: i64 = 1) -> i64:\n    a\n",
            "def f(a: i64 = a) -> i64:\n    a\n",
            "def f(a: i64, b: i64 = a + 1) -> i64:\n    b\n",
        ] {
            assert!(diagnostic(source).contains("default expressions cannot reference parameter"));
        }
    }

    #[test]
    fn required_parameters_cannot_follow_defaults() {
        assert!(
            diagnostic("def f(a: i64 = 1, b: i64) -> i64:\n    b\n")
                .contains("required parameters must precede")
        );
    }

    #[test]
    fn protocol_defaults_are_not_callable_contracts() {
        assert!(
            diagnostic("protocol P:\n    def f(self, a: i64 = 1) -> i64:\n        pass\n")
                .contains("protocol requirements cannot declare parameter defaults")
        );
    }

    #[test]
    fn defaults_do_not_capture_a_shadowing_comprehension_variable() {
        // A comprehension's own binding is not a dependency on the parameter.
        compile("def f(a: i64, values: list[i64] = [a for a in range(2)]?) -> Result[i64, AllocError]:\n    drop(values)\n    Ok(a)\ndef main() -> ():\n    pass\n", &mut Heap::default()).unwrap();
    }

    #[test]
    fn anonymous_function_defaults_check_their_one_expression_body() {
        // The anonymous function's own parameter shadows the outer one.
        compile("def f(a: i64, g: Callable[[i64], i64] = def(a: i64) -> i64: a + 1) -> i64:\n    g(a)\ndef main() -> ():\n    pass\n", &mut Heap::default()).unwrap();
        assert!(diagnostic(
            "def f(a: i64, g: Callable[[i64], i64] = def(x: i64) -> i64: x + a) -> i64:\n    g(a)\n"
        )
        .contains("default expressions cannot reference parameter"));
    }
}
