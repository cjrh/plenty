//! Compile-time evaluation of module constants and static assertions.
//!
//! After name and type resolution each constant is a getter whose body is its
//! initializer, with references to other constants resolved to getter calls.
//! [`fold`] evaluates every initializer, replaces it with the literal it
//! denotes, and removes static assertions. Codegen then substitutes the
//! literal at each read (`codegen::small_calls`). The rules follow the
//! runtime operators in `book/src/design/language/03-types/numbers.md`, but
//! an overflow, division by zero, or invalid shift is a compile-time error
//! here. See `book/src/design/language/02-source/modules.md#constants`.
use super::*;
use crate::op::IntegerDomain;
use std::rc::Rc;

mod quantities;

/// The parse-time name prefix of a static assertion's getter.
pub(super) const ASSERT: &str = "$assert";

const GETTER: &str = "$const.";

/// The longest chain of constants that depend on one another.
const MAX_DEPTH: usize = 256;

/// The largest `str` or `bytes` value concatenation may build.
const MAX_TEXT: usize = 1 << 24;

#[derive(Clone)]
enum Value {
    Quantity(Box<Value>, Ty),
    Int(i128, Ty),
    Float(f64, Ty),
    Bool(bool),
    Str(Rc<str>),
    Bytes(Rc<[u8]>),
}

impl Value {
    fn ty(&self) -> Ty {
        match self {
            Value::Int(_, t) | Value::Float(_, t) | Value::Quantity(_, t) => t.clone(),
            Value::Bool(_) => Ty::Bool,
            Value::Str(_) => Ty::Str,
            Value::Bytes(_) => Ty::ByteStr,
        }
    }
}

/// Lazy output types for constants read before the ordinary folding pass.
/// The pending entry rejects a constant whose type depends on its own value.
#[derive(Clone, Default)]
pub(super) struct Types(std::rc::Rc<std::cell::RefCell<HashMap<String, ConstantType>>>);
#[derive(Clone)]
enum ConstantType {
    Resolving,
    Resolved(std::result::Result<Ty, Diagnostic>),
}
impl Types {
    fn resolve(&self, f: &Function, aliases: &TypeAliases) -> Result<Ty> {
        if let Some(state) = self.0.borrow().get(&f.name).cloned() {
            return match state {
                ConstantType::Resolving => Err(f.at.error(format!(
                    "constant `{}` has a cyclic type dependency",
                    display(&f.name)
                ))),
                ConstantType::Resolved(result) => {
                    result.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
                }
            };
        }
        self.0
            .borrow_mut()
            .insert(f.name.clone(), ConstantType::Resolving);
        let result = f.output.resolve(aliases).and_then(|ty| {
            ty.ok_or_else(|| f.at.error("module constants require a concrete type"))
        });
        self.0.borrow_mut().insert(
            f.name.clone(),
            ConstantType::Resolved(result.as_ref().cloned().map_err(|e| Diagnostic::of(&**e))),
        );
        result
    }
}

/// Folding owns a reference index over its mutable function list; value
/// arguments borrow the compilation's already indexed constant definitions.
enum Getters<'a> {
    References(HashMap<&'a str, &'a Function>),
    Shared(&'a HashMap<String, Function>),
}
impl<'a> Getters<'a> {
    fn get(&self, name: &str) -> Option<&'a Function> {
        match self {
            Self::References(getters) => getters.get(name).copied(),
            Self::Shared(getters) => getters.get(name),
        }
    }
}

struct Folder<'a> {
    aliases: &'a TypeAliases,
    getters: Getters<'a>,
    values: HashMap<String, Value>,
    active: Vec<String>,
    /// Depth of branches that run time would not take: the unselected arm of
    /// `x if c else y` and the right operand of a short-circuited `and` or
    /// `or`. They are type-checked, but a value that would trap there becomes
    /// zero of its type instead of an error.
    unselected: u32,
    /// Ceiling checks compare domain-preserving arithmetic as its integer.
    /// Ordinary module constants still require `.get()` to read a domain.
    domain_arguments: bool,
}

/// Evaluates every constant and static assertion in `functions`, replacing
/// each constant's initializer with its value and removing the assertions.
pub(super) fn fold(functions: &mut Vec<Function>, aliases: &TypeAliases) -> Result<()> {
    for f in functions.iter_mut().filter(|f| f.name.starts_with(GETTER)) {
        if f.output.name.as_deref() == Some("Quantity") {
            f.output.concrete = f.output.resolve(aliases)?.map(Box::new);
        }
    }
    let ide = super::ide::active();
    let mut folded = Vec::new();
    {
        let mut folder = Folder {
            aliases,
            getters: Getters::References(
                functions
                    .iter()
                    .filter(|f| f.name.starts_with(GETTER))
                    .map(|f| (f.name.as_str(), f))
                    .collect(),
            ),
            values: HashMap::default(),
            active: vec![],
            unselected: 0,
            domain_arguments: false,
        };
        for (i, f) in functions.iter().enumerate() {
            let result = if f.name.starts_with(GETTER) {
                folder.constant(f).map(Some)
            } else if is_assert(f) {
                folder.assertion(f).map(|()| None)
            } else {
                continue;
            };
            match result {
                Ok(value) => folded.push((i, value)),
                Err(error) if ide => {
                    super::ide::recover(&*error);
                    folded.push((i, None));
                }
                Err(error) => return Err(error),
            }
        }
    }
    let mut asserts = HashSet::default();
    for (i, value) in folded {
        let f = &mut functions[i];
        if is_assert(f) {
            asserts.insert(i);
            continue;
        }
        let (ty, domain) = declared(f, aliases)?;
        let value = value.unwrap_or_else(|| default(&ty, domain));
        f.body = vec![Stmt {
            at: f.at.clone(),
            kind: Statement::Expr(literal(&value, domain.is_some(), &f.at)),
        }];
    }
    let mut index = 0;
    functions.retain(|_| {
        index += 1;
        !asserts.contains(&(index - 1))
    });
    Ok(())
}

/// Speculative evaluation for std ceiling diagnostics. A dynamic expression or
/// one this evaluator cannot prove remains subject to the std runtime guard.
pub(super) fn known_integer(
    e: &Expr,
    constants: &HashMap<String, Arc<Function>>,
    aliases: &TypeAliases,
) -> Option<i128> {
    match known(e, &Ty::I64, constants, aliases)? {
        Value::Int(n, _) => Some(n),
        _ => None,
    }
}

/// A literal or module-constant string, for open-mode diagnostics. Other
/// strings are checked when the runtime receives them.
pub(super) fn known_text(
    e: &Expr,
    constants: &HashMap<String, Arc<Function>>,
    aliases: &TypeAliases,
) -> Option<String> {
    if let Expression::Text(text) = &ungroup(e).kind {
        return Some(text.clone());
    }
    match known(e, &Ty::Str, constants, aliases)? {
        Value::Str(text) => Some(text.to_string()),
        _ => None,
    }
}

fn known(
    e: &Expr,
    expected: &Ty,
    constants: &HashMap<String, Arc<Function>>,
    aliases: &TypeAliases,
) -> Option<Value> {
    // Most std calls forward a local value. It is dynamic, and needs neither
    // a constants table nor speculative evaluation.
    if matches!(ungroup(e).kind, Expression::Name(_)) {
        return None;
    }
    let mut folder = Folder {
        aliases,
        getters: Getters::References(
            constants
                .iter()
                .map(|(n, f)| (n.as_str(), f.as_ref()))
                .collect(),
        ),
        values: HashMap::default(),
        active: Vec::new(),
        unselected: 0,
        domain_arguments: true,
    };
    match folder.domain_read(e) {
        Some(f) => folder.constant(f),
        None => folder.eval(e, Some(expected)),
    }
    .ok()
}

fn is_assert(f: &Function) -> bool {
    f.name
        .rsplit('.')
        .next()
        .is_some_and(|n| n.starts_with(ASSERT))
}

/// The type a constant folds as, and its integer domain: `Positive[i64]`
/// folds as `i64`.
fn declared(f: &Function, aliases: &TypeAliases) -> Result<(Ty, Option<IntegerDomain>)> {
    let output = &f.output;
    let domain = output.name.as_deref().and_then(IntegerDomain::named);
    let ty = match (domain, output.args.as_slice()) {
        (Some(_), [element]) => element.name.as_deref().and_then(named_type),
        _ => output
            .concrete
            .as_deref()
            .cloned()
            .or_else(|| output.name.as_deref().and_then(named_type)),
    };
    // Value arguments may read this getter before `fold` prepares Quantity
    // outputs. Resolve only the getter actually reached by the evaluator.
    let ty = match ty {
        Some(ty) => ty,
        None => aliases.constant_types.resolve(f, aliases)?,
    };
    Ok((ty, domain))
}

fn default(ty: &Ty, domain: Option<IntegerDomain>) -> Value {
    if let Ty::Class(class) = ty
        && class.get().quantity.is_some()
    {
        let storage = &class.get().fields[0].1;
        let value = default(storage, storage.integer_domain().map(|(domain, _)| domain));
        return Value::Quantity(Box::new(value), ty.clone());
    }
    match ty {
        Ty::Bool => Value::Bool(false),
        Ty::Str => Value::Str("".into()),
        Ty::ByteStr => Value::Bytes(Rc::from(&[][..])),
        t if t.is_float() => Value::Float(0.0, t.clone()),
        // An editor placeholder for a domain constant stays in its domain.
        t => Value::Int(
            i128::from(domain.is_some_and(|d| d != IntegerDomain::NonNegative)),
            t.clone(),
        ),
    }
}

fn admits(domain: IntegerDomain, n: i128) -> bool {
    match domain {
        IntegerDomain::NonZero => n != 0,
        IntegerDomain::Positive => n > 0,
        IntegerDomain::NonNegative => n >= 0,
    }
}

/// A literal expression denoting `value`, in the form the checker types
/// exactly as the constant's declared type. An integer domain's literal is
/// unsuffixed, so the checker converts it as it converts a literal argument.
fn literal(value: &Value, domain: bool, at: &Token) -> Expr {
    let expr = |kind| Expr {
        at: at.clone(),
        kind,
    };
    let negate = |inner: Expr| expr(Expression::Unary("-".into(), Box::new(inner)));
    match value {
        Value::Quantity(value, ty) => expr(Expression::Constructor(
            Box::new(generics::type_ref(ty, at)),
            vec![literal(value, value.ty().integer_domain().is_some(), at)],
        )),
        Value::Int(n, ty) => {
            let suffix = if domain {
                String::new()
            } else {
                ty.to_string()
            };
            let number = expr(Expression::Number(format!("{}{suffix}", n.unsigned_abs())));
            if *n < 0 { negate(number) } else { number }
        }
        Value::Float(x, ty) => {
            let member = |name: &str| {
                expr(Expression::Member(
                    Box::new(expr(Expression::Name(ty.to_string()))),
                    name.into(),
                    Default::default(),
                ))
            };
            let magnitude = if x.is_nan() {
                return member("NAN");
            } else if x.is_infinite() {
                member("INF")
            } else if *ty == Ty::F32 {
                expr(Expression::Number(format!("{:?}f32", x.abs() as f32)))
            } else {
                expr(Expression::Number(format!("{:?}f64", x.abs())))
            };
            if x.is_sign_negative() {
                negate(magnitude)
            } else {
                magnitude
            }
        }
        Value::Bool(b) => expr(Expression::Bool(*b)),
        Value::Str(s) => expr(Expression::Text(s.to_string())),
        Value::Bytes(b) => expr(Expression::Bytes(b.to_vec())),
    }
}

fn display(getter: &str) -> &str {
    getter.strip_prefix(GETTER).unwrap_or(getter)
}

impl<'a> Folder<'a> {
    fn constant(&mut self, f: &Function) -> Result<Value> {
        if let Some(value) = self.values.get(&f.name) {
            return Ok(value.clone());
        }
        if let Some(start) = self.active.iter().position(|n| *n == f.name) {
            let mut chain: Vec<_> = self.active[start..].iter().map(|n| display(n)).collect();
            chain.push(display(&f.name));
            return Err(f.at.error(format!(
                "constant `{}` depends on itself: {}",
                display(&f.name),
                chain.join(" -> ")
            )));
        }
        if self.active.len() >= MAX_DEPTH {
            return Err(f.at.error(format!(
                "constant dependencies nest more than {MAX_DEPTH} deep"
            )));
        }
        let (ty, domain) = declared(f, self.aliases)?;
        let [
            Stmt {
                kind: Statement::Expr(initializer),
                ..
            },
        ] = f.body.as_slice()
        else {
            // An editor check clears a body that does not resolve.
            return Ok(default(&ty, domain));
        };
        self.active.push(f.name.clone());
        let value = match (domain, self.domain_read(initializer)) {
            // Another constant of the same domain type is already checked.
            (Some(_), Some(g)) if declared(g, self.aliases)? == (ty.clone(), domain) => {
                self.constant(g)
            }
            _ => self.typed(initializer, &ty),
        };
        self.active.pop();
        let value = value?;
        if let (Some(domain), Value::Int(n, _)) = (domain, &value)
            && !admits(domain, *n)
        {
            let reason = match domain {
                IntegerDomain::NonZero => "a NonZero value cannot be zero",
                IntegerDomain::Positive => "a Positive value must be greater than zero",
                IntegerDomain::NonNegative => "a NonNegative value cannot be negative",
            };
            return Err(initializer.at.error(format!(
                "constant `{}` is {n}, which is not a `{}[{ty}]`: {reason}",
                display(&f.name),
                domain.name()
            )));
        }
        self.values.insert(f.name.clone(), value.clone());
        Ok(value)
    }

    /// The getter `e` reads, when it reads an integer domain constant.
    fn domain_read(&self, e: &Expr) -> Option<&'a Function> {
        match &ungroup(e).kind {
            Expression::Call(name, args) if args.is_empty() && name.starts_with(GETTER) => {
                self.getters.get(name.as_str()).filter(|f| {
                    f.output
                        .name
                        .as_deref()
                        .and_then(IntegerDomain::named)
                        .is_some()
                })
            }
            _ => None,
        }
    }

    fn assertion(&mut self, f: &Function) -> Result<()> {
        let mut parts = f.body.iter().filter_map(|s| match &s.kind {
            Statement::Expr(e) => Some(e),
            _ => None,
        });
        let Some(condition) = parts.next() else {
            return Ok(());
        };
        let holds = self.typed(condition, &Ty::Bool)?;
        let message = parts.next().map(|m| self.typed(m, &Ty::Str)).transpose()?;
        if matches!(holds, Value::Bool(true)) {
            return Ok(());
        }
        Err(condition.at.error(match message {
            Some(Value::Str(m)) => format!("static assertion failed: {m}"),
            _ => "static assertion failed".into(),
        }))
    }

    /// `typed`, or only checked when run time would not evaluate `e`.
    fn branch_typed(&mut self, e: &Expr, ty: &Ty, taken: bool) -> Result<Value> {
        self.unselected += u32::from(!taken);
        let value = self.typed(e, ty);
        self.unselected -= u32::from(!taken);
        value
    }

    /// Evaluates `e`, which must have type `ty`.
    fn typed(&mut self, e: &Expr, ty: &Ty) -> Result<Value> {
        let value = self.eval(e, Some(ty))?;
        let got = value.ty();
        if got != *ty {
            return Err(self.aliases.dimension_error(
                &e.at,
                format!("expected {ty}, got {got}"),
                [ty, &got],
            ));
        }
        Ok(value)
    }

    /// The type `e` has regardless of context, or None when an unsuffixed
    /// literal decides it.
    fn natural(&self, e: &Expr) -> Option<Ty> {
        if let Some(Ok((ty, _))) = self.constructor(e) {
            return Some(ty);
        }
        match &e.kind {
            Expression::Group(inner) => self.natural(inner),
            Expression::Number(text) => {
                let split = text.find(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E');
                split.and_then(|i| named_type(&text[i..]))
            }
            Expression::Text(_) => Some(Ty::Str),
            Expression::Bytes(_) => Some(Ty::ByteStr),
            Expression::Bool(_) => Some(Ty::Bool),
            Expression::Call(name, args) if args.is_empty() && name.starts_with(GETTER) => self
                .getters
                .get(name.as_str())
                .and_then(|f| declared(f, self.aliases).ok())
                .filter(|(_, domain)| domain.is_none())
                .map(|(ty, _)| ty),
            Expression::Method(base, method, args, _) if method == "get" && args.is_empty() => {
                if let Some(ty) = self.natural(base) {
                    self.aliases
                        .quantity(&ty)
                        .map(|(storage, _, _)| storage)
                        .or_else(|| ty.domain_element())
                } else {
                    self.domain_read(base)
                        .and_then(|f| declared(f, self.aliases).ok())
                        .map(|(ty, _)| ty)
                }
            }
            Expression::Method(base, method, args, _) if method == "to" && args.len() == 1 => self
                .aliases
                .quantity(&self.natural(base)?)
                .map(|(storage, _, _)| storage),
            Expression::Call(name, _) => named_type(name).filter(|t| t.is_int() || t.is_float()),
            Expression::Member(base, _, _) => match &base.kind {
                Expression::Name(n) => named_type(n),
                _ => None,
            },
            Expression::Unary(op, inner) if op != "not" => {
                let ty = self.natural(inner)?;
                if let Some((storage, dimension, _)) = self.aliases.quantity(&ty) {
                    let storage = if op == "-" {
                        storage.domain_element().unwrap_or(storage)
                    } else {
                        storage
                    };
                    self.quantity_type(&storage, (*dimension).clone(), &e.at)
                        .ok()
                } else if op == "-" {
                    Some(ty.domain_element().unwrap_or(ty))
                } else {
                    Some(ty)
                }
            }
            Expression::Unary(..) => Some(Ty::Bool),
            Expression::Binary(op, a, b) => match op.as_str() {
                "==" | "!=" | "<" | "<=" | ">" | ">=" | "and" | "or" => Some(Ty::Bool),
                "<<" | ">>" => self.natural(a),
                _ => {
                    let an = self.natural(a);
                    let bn = self.natural(b);
                    let storage = self.storage_context(an.as_ref(), bn.as_ref());
                    if let Some(storage) = storage {
                        self.wrapped_result(
                            op,
                            &self.operand_type(a, an, &storage),
                            &self.operand_type(b, bn, &storage),
                            &e.at,
                        )
                        .ok()
                    } else {
                        an.or(bn)
                    }
                }
            },
            Expression::Conditional { yes, no, .. } => {
                self.natural(yes).or_else(|| self.natural(no))
            }
            _ => None,
        }
    }

    fn eval(&mut self, e: &Expr, expected: Option<&Ty>) -> Result<Value> {
        if let Some(value) = self.quantity_eval(e, expected) {
            return value;
        }
        match &e.kind {
            Expression::Group(inner) => self.eval(inner, expected),
            Expression::Number(text) => number(text, false, expected, &e.at),
            Expression::Text(t) => Ok(Value::Str(t.as_str().into())),
            Expression::Bytes(b) => Ok(Value::Bytes(b.as_slice().into())),
            Expression::Bool(b) => Ok(Value::Bool(*b)),
            Expression::Call(name, args) if args.is_empty() && name.starts_with(GETTER) => {
                let Some(f) = self.getters.get(name.as_str()) else {
                    return Err(e.at.error(format!("unknown constant `{}`", display(name))));
                };
                if let (ty, Some(domain)) = declared(f, self.aliases)?
                    && !self.domain_arguments
                {
                    let name = display(name);
                    return Err(e.at.error(format!(
                        "`{name}` is a `{}[{ty}]`; read its integer with `{name}.get()`",
                        domain.name()
                    )));
                }
                self.constant(f)
            }
            Expression::Method(base, method, args, _)
                if method == "get" && args.is_empty() && self.domain_read(base).is_some() =>
            {
                let f = self.domain_read(base).unwrap();
                self.constant(f)
            }
            Expression::Call(name, args) => {
                let target = named_type(name).filter(|t| t.is_int() || t.is_float());
                match (target, args.as_slice()) {
                    (Some(target), [operand]) => self.cast(operand, &target),
                    (Some(_), _) => Err(e.at.error("a numeric cast takes one operand")),
                    (None, _) => Err(not_constant(e)),
                }
            }
            Expression::Member(base, name, _) => {
                let ty = match &base.kind {
                    Expression::Name(n) => named_type(n).filter(|t| t.is_int() || t.is_float()),
                    _ => None,
                };
                let Some(ty) = ty else {
                    return Err(not_constant(e));
                };
                match super::numeric::constant(&ty, name) {
                    Some(Op::PushInt(v)) => Ok(Value::Int(integer_value(&v), ty)),
                    Some(Op::PushFloat { bits, .. }) => Ok(Value::Float(
                        if ty == Ty::F32 {
                            f64::from(f32::from_bits(bits as u32))
                        } else {
                            f64::from_bits(bits)
                        },
                        ty,
                    )),
                    _ => Err(e.at.error(format!("unknown constant `{ty}.{name}`"))),
                }
            }
            Expression::Unary(op, inner) => self.unary(e, op, inner, expected),
            Expression::Binary(op, a, b) => self.binary(e, op, a, b, expected),
            Expression::Conditional { condition, yes, no } => {
                let Value::Bool(holds) = self.typed(condition, &Ty::Bool)? else {
                    unreachable!("typed as bool")
                };
                let expected = expected
                    .cloned()
                    .or_else(|| self.natural(yes).or_else(|| self.natural(no)));
                // As at run time, only the selected branch is evaluated; both
                // must be constant expressions of one type.
                let yes = self.branch(yes, expected.as_ref(), holds)?;
                let no = self.branch(no, expected.as_ref(), !holds)?;
                if yes.ty() != no.ty() {
                    return Err(e.at.error(format!(
                        "conditional branches differ: {} and {}",
                        yes.ty(),
                        no.ty()
                    )));
                }
                Ok(if holds { yes } else { no })
            }
            _ => Err(not_constant(e)),
        }
    }

    /// Evaluates `e`, or only checks it when run time would not evaluate it.
    fn branch(&mut self, e: &Expr, expected: Option<&Ty>, taken: bool) -> Result<Value> {
        if taken {
            return self.eval(e, expected);
        }
        self.unselected += 1;
        let value = self.eval(e, expected);
        self.unselected -= 1;
        value
    }

    fn spare(&self) -> bool {
        self.unselected > 0
    }

    fn cast(&mut self, operand: &Expr, target: &Ty) -> Result<Value> {
        let source = self.natural(operand);
        let value = self.eval(operand, source.as_ref())?;
        let at = &operand.at;
        if value.ty().integer_domain().is_some() {
            return Err(at.error("numeric casts require a number; read the domain with `.get()`"));
        }
        match value {
            Value::Int(n, _) if target.is_int() => {
                int(n, target, at, "integer cast out of range for", self.spare())
            }
            Value::Int(n, _) if *target == Ty::F32 => {
                Ok(Value::Float(f64::from(n as f32), Ty::F32))
            }
            Value::Int(n, _) => Ok(Value::Float(n as f64, Ty::F64)),
            Value::Float(x, _) if target.is_int() => {
                if (x.is_nan() || x.is_infinite()) && !self.spare() {
                    return Err(at.error(format!("integer cast out of range for {target}: {x}")));
                }
                int(
                    if x.is_finite() { x.trunc() as i128 } else { 0 },
                    target,
                    at,
                    "integer cast out of range for",
                    self.spare(),
                )
            }
            Value::Float(x, _) if *target == Ty::F32 => {
                Ok(Value::Float(f64::from(x as f32), Ty::F32))
            }
            Value::Float(x, _) => Ok(Value::Float(x, Ty::F64)),
            other => Err(at.error(format!(
                "numeric casts require a number, not {}",
                other.ty()
            ))),
        }
    }

    fn unary(&mut self, e: &Expr, op: &str, inner: &Expr, expected: Option<&Ty>) -> Result<Value> {
        if op == "-"
            && let Expression::Number(text) = &ungroup(inner).kind
        {
            return number(text, true, expected, &e.at);
        }
        let value = self.eval(inner, expected)?;
        match (op, value) {
            ("-", Value::Int(n, ty)) => {
                let ty = ty.domain_element().unwrap_or(ty);
                if !matches!(ty, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64) {
                    return Err(e.at.error(format!("cannot negate unsigned {ty}")));
                }
                int(
                    -n,
                    &ty,
                    &e.at,
                    "constant arithmetic overflows",
                    self.spare(),
                )
            }
            ("-", Value::Float(x, ty)) => Ok(Value::Float(-x, ty)),
            ("~", Value::Int(_, ty)) if ty.integer_domain().is_some() => {
                Err(e.at.error(nonzero::unsupported(op, &ty)))
            }
            ("+", value) if value.ty().is_numeric() || value.ty().integer_domain().is_some() => {
                Ok(value)
            }
            ("~", Value::Int(n, ty)) => {
                let (min, end) = ty.int_range().unwrap();
                Ok(Value::Int(if min < 0 { !n } else { end - 1 - n }, ty))
            }
            ("not", Value::Bool(b)) => Ok(Value::Bool(!b)),
            (_, value) => Err(e
                .at
                .error(format!("operator `{op}` does not apply to {}", value.ty()))),
        }
    }

    fn binary(
        &mut self,
        e: &Expr,
        op: &str,
        a: &Expr,
        b: &Expr,
        expected: Option<&Ty>,
    ) -> Result<Value> {
        if let Some(value) = self.wrapped_binary(e, op, a, b) {
            return value;
        }
        let at = &e.at;
        match op {
            "and" | "or" => {
                let Value::Bool(left) = self.typed(a, &Ty::Bool)? else {
                    unreachable!()
                };
                let taken = left == (op == "and");
                let Value::Bool(right) = self.branch_typed(b, &Ty::Bool, taken)? else {
                    unreachable!()
                };
                return Ok(Value::Bool(if op == "and" {
                    left && right
                } else {
                    left || right
                }));
            }
            "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                let operand = self.natural(a).or_else(|| self.natural(b));
                let left = self.eval(a, operand.as_ref())?;
                let right = self.eval(b, operand.as_ref())?;
                return compare(op, &left, &right, at);
            }
            _ => {}
        }
        let operand = match (op, expected) {
            ("<<" | ">>", _) => expected.cloned().or_else(|| self.natural(a)),
            (_, Some(Ty::Bool)) => self.natural(a).or_else(|| self.natural(b)),
            _ => expected
                .cloned()
                .or_else(|| self.natural(a).or_else(|| self.natural(b))),
        };
        let left = self.eval(a, operand.as_ref())?;
        let count = match (op, &left) {
            ("<<" | ">>", Value::Int(_, ty)) => Some(ty.clone()),
            _ => operand,
        };
        let right = self.eval(b, count.as_ref())?;
        self.combine(op, left, right, at)
    }

    fn combine(&self, op: &str, left: Value, right: Value, at: &Token) -> Result<Value> {
        if let Some(value) = self.domain_combine(op, &left, &right, at) {
            return value;
        }
        if left.ty() != right.ty() {
            return Err(at.error(format!(
                "cannot combine {} with {} in a constant",
                left.ty(),
                right.ty()
            )));
        }
        match (left, right) {
            (Value::Int(x, ty), Value::Int(y, _)) => integer_op(op, x, y, &ty, at, self.spare()),
            (Value::Float(x, ty), Value::Float(y, _)) => {
                let f32 = ty == Ty::F32;
                let z = match op {
                    "+" if f32 => f64::from(x as f32 + y as f32),
                    "-" if f32 => f64::from(x as f32 - y as f32),
                    "*" if f32 => f64::from(x as f32 * y as f32),
                    "/" if f32 => f64::from(x as f32 / y as f32),
                    "+" => x + y,
                    "-" => x - y,
                    "*" => x * y,
                    "/" => x / y,
                    _ => {
                        return Err(at.error(format!("operator `{op}` does not apply to {ty}")));
                    }
                };
                Ok(Value::Float(z, ty))
            }
            (Value::Str(x), Value::Str(y)) if op == "+" => {
                if x.len() + y.len() > MAX_TEXT {
                    return Err(at.error(format!(
                        "constant string exceeds the limit of {MAX_TEXT} bytes"
                    )));
                }
                Ok(Value::Str(format!("{x}{y}").into()))
            }
            (left, _) => Err(at.error(format!(
                "operator `{op}` does not apply to {} in a constant",
                left.ty()
            ))),
        }
    }
}

/// Integer arithmetic with run-time semantics. Where run time would trap,
/// returns an error, or zero when `spare` (see `Folder::unselected`).
fn integer_op(op: &str, x: i128, y: i128, ty: &Ty, at: &Token, spare: bool) -> Result<Value> {
    let (min, end) = ty.int_range().unwrap();
    let width = (end - min).trailing_zeros() as i128;
    let overflow = "constant arithmetic overflows";
    let z = match op {
        "+" => x + y,
        "-" => x - y,
        "*" => match x.checked_mul(y) {
            Some(z) => z,
            None if spare => 0,
            None => return Err(at.error(format!("{overflow} {ty}"))),
        },
        "//" | "%" if y == 0 && spare => 0,
        "//" | "%" if y == 0 => return Err(at.error("division by zero in a constant")),
        "//" => {
            let q = x / y;
            if x % y != 0 && ((x < 0) != (y < 0)) {
                q - 1
            } else {
                q
            }
        }
        "%" => {
            let r = x % y;
            if r != 0 && ((r < 0) != (y < 0)) {
                r + y
            } else {
                r
            }
        }
        "&" => x & y,
        "|" => x | y,
        "^" => x ^ y,
        "<<" | ">>" => {
            if (y < 0 || y >= width) && spare {
                return Ok(Value::Int(0, ty.clone()));
            }
            if y < 0 || y >= width {
                return Err(at.error(format!("shift count {y} is outside 0..{width} for {ty}")));
            }
            if op == ">>" {
                x >> y
            } else {
                // Left shifts discard the bits shifted past the width.
                let bits = ((x as u128) << y) & ((1u128 << width) - 1);
                if min < 0 && bits >> (width - 1) == 1 {
                    bits as i128 - (1i128 << width)
                } else {
                    bits as i128
                }
            }
        }
        "/" => {
            return Err(at.error(format!(
                "`/` divides floats; use `//` for {ty} floor division"
            )));
        }
        _ => return Err(at.error(format!("operator `{op}` does not apply to {ty}"))),
    };
    int(z, ty, at, overflow, spare)
}

fn int(n: i128, ty: &Ty, at: &Token, message: &str, spare: bool) -> Result<Value> {
    let (min, end) = ty.int_range().unwrap();
    if (n < min || n >= end) && spare {
        return Ok(Value::Int(0, ty.clone()));
    }
    if n < min || n >= end {
        return Err(at.error(format!("{message} {ty}: {n}")));
    }
    Ok(Value::Int(n, ty.clone()))
}

fn compare(op: &str, left: &Value, right: &Value, at: &Token) -> Result<Value> {
    use std::cmp::Ordering;
    if left.ty() != right.ty() {
        return Err(at.error(format!("cannot compare {} with {}", left.ty(), right.ty())));
    }
    let ordering = match (left, right) {
        (Value::Int(x, _), Value::Int(y, _)) => Some(x.cmp(y)),
        (Value::Float(x, _), Value::Float(y, _)) => x.partial_cmp(y),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        (Value::Bytes(x), Value::Bytes(y)) if matches!(op, "==" | "!=") => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) if matches!(op, "==" | "!=") => Some(x.cmp(y)),
        _ => {
            return Err(at.error(format!("operator `{op}` does not apply to {}", left.ty())));
        }
    };
    Ok(Value::Bool(match op {
        "==" => ordering == Some(Ordering::Equal),
        "!=" => ordering != Some(Ordering::Equal),
        "<" => ordering == Some(Ordering::Less),
        "<=" => matches!(ordering, Some(Ordering::Less | Ordering::Equal)),
        ">" => ordering == Some(Ordering::Greater),
        _ => matches!(ordering, Some(Ordering::Greater | Ordering::Equal)),
    }))
}

/// A numeric literal, typed by its suffix, else by `expected`, else by the
/// defaults `i64` and `f64`.
fn number(text: &str, negative: bool, expected: Option<&Ty>, at: &Token) -> Result<Value> {
    let float = text.contains(['.', 'e', 'E']) || text.ends_with("f32") || text.ends_with("f64");
    if float {
        let (digits, ty) = if let Some(s) = text.strip_suffix("f32") {
            (s, Ty::F32)
        } else if let Some(s) = text.strip_suffix("f64") {
            (s, Ty::F64)
        } else {
            (
                text,
                expected
                    .filter(|t| t.is_float())
                    .cloned()
                    .unwrap_or(Ty::F64),
            )
        };
        let digits = digits.replace('_', "");
        let x = if ty == Ty::F32 {
            let x: f32 = digits
                .parse()
                .map_err(|_| at.error("invalid f32 literal"))?;
            x.is_finite()
                .then_some(f64::from(x))
                .ok_or_else(|| at.error("f32 literal is out of range"))?
        } else {
            let x: f64 = digits
                .parse()
                .map_err(|_| at.error("invalid f64 literal"))?;
            x.is_finite()
                .then_some(x)
                .ok_or_else(|| at.error("f64 literal is out of range"))?
        };
        return Ok(Value::Float(if negative { -x } else { x }, ty));
    }
    let split = text
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    let ty = if split == text.len() {
        expected.filter(|t| t.is_int()).cloned().unwrap_or(Ty::I64)
    } else {
        named_type(&text[split..])
            .filter(|t| t.is_int())
            .ok_or_else(|| at.error("invalid integer suffix"))?
    };
    // Unsuffixed or not, the checker's literal rules apply to the result.
    let suffixed = if split == text.len() {
        format!("{text}{ty}")
    } else {
        text.to_string()
    };
    let value = integer(&suffixed, negative, at)?;
    Ok(Value::Int(integer_value(&value), ty))
}

fn integer_value(v: &crate::value::Value) -> i128 {
    use crate::value::Value as V;
    match *v {
        V::I8(n) => n.into(),
        V::I16(n) => n.into(),
        V::I32(n) => n.into(),
        V::I64(n) => n.into(),
        V::U8(n) => n.into(),
        V::U16(n) => n.into(),
        V::U32(n) => n.into(),
        V::U64(n) => n.into(),
    }
}

fn not_constant(e: &Expr) -> Box<dyn Error> {
    e.at.error(
        "not a constant expression: constants may use literals, other constants, numeric type constants and casts, and operators",
    )
}

/// Evaluate the deliberately restricted value-argument syntax with the same
/// width and signedness checks as module constants.
pub(super) fn generic_value(
    e: &Expr,
    expected: &Ty,
    aliases: &TypeAliases,
) -> Result<value_generics::Constant> {
    let mut folder = Folder {
        aliases,
        getters: Getters::Shared(&aliases.constants),
        values: HashMap::default(),
        active: vec![],
        unselected: 0,
        domain_arguments: false,
    };
    let value = folder.typed(e, expected)?;
    match value {
        Value::Int(value, ty) if ty.is_int() => Ok(value_generics::Constant { ty, value }),
        Value::Bool(value) => Ok(value_generics::Constant {
            ty: Ty::Bool,
            value: i128::from(value),
        }),
        _ => Err(e.at.error("value arguments must be integers or bool")),
    }
}
