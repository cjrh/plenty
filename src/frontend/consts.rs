//! Compile-time evaluation of module constants and static assertions.
//!
//! After name resolution each constant is a getter whose body is its
//! initializer, with references to other constants resolved to getter calls.
//! [`fold`] evaluates every initializer, replaces it with the literal it
//! denotes, and removes static assertions. Codegen then substitutes the
//! literal at each read (`codegen::small_calls`). The rules follow the
//! runtime operators in `book/src/design/language/03-types/numbers.md`, but
//! an overflow, division by zero, or invalid shift is a compile-time error
//! here. See `book/src/design/language/02-source/modules.md#constants`.
use super::*;
use std::rc::Rc;

/// The parse-time name prefix of a static assertion's getter.
pub(super) const ASSERT: &str = "$assert";

const GETTER: &str = "$const.";

/// The longest chain of constants that depend on one another.
const MAX_DEPTH: usize = 256;

/// The largest `str` or `bytes` value concatenation may build.
const MAX_TEXT: usize = 1 << 24;

#[derive(Clone)]
enum Value {
    Int(i128, Ty),
    Float(f64, Ty),
    Bool(bool),
    Str(Rc<str>),
    Bytes(Rc<[u8]>),
}

impl Value {
    fn ty(&self) -> Ty {
        match self {
            Value::Int(_, t) | Value::Float(_, t) => t.clone(),
            Value::Bool(_) => Ty::Bool,
            Value::Str(_) => Ty::Str,
            Value::Bytes(_) => Ty::ByteStr,
        }
    }
}

struct Folder<'a> {
    getters: HashMap<&'a str, &'a Function>,
    values: HashMap<String, Value>,
    active: Vec<String>,
    /// Depth of branches that run time would not take: the unselected arm of
    /// `x if c else y` and the right operand of a short-circuited `and` or
    /// `or`. They are type-checked, but a value that would trap there becomes
    /// zero of its type instead of an error.
    unselected: u32,
}

/// Evaluates every constant and static assertion in `functions`, replacing
/// each constant's initializer with its value and removing the assertions.
pub(super) fn fold(functions: &mut Vec<Function>) -> Result<()> {
    let ide = super::ide::active();
    let mut folded = Vec::new();
    {
        let mut folder = Folder {
            getters: functions
                .iter()
                .filter(|f| f.name.starts_with(GETTER))
                .map(|f| (f.name.as_str(), f))
                .collect(),
            values: HashMap::default(),
            active: vec![],
            unselected: 0,
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
        let ty = declared(f);
        let value = value.unwrap_or_else(|| default(&ty));
        f.body = vec![Stmt {
            at: f.at.clone(),
            kind: Statement::Expr(literal(&value, &f.at)),
        }];
    }
    let mut index = 0;
    functions.retain(|_| {
        index += 1;
        !asserts.contains(&(index - 1))
    });
    Ok(())
}

fn is_assert(f: &Function) -> bool {
    f.name
        .rsplit('.')
        .next()
        .is_some_and(|n| n.starts_with(ASSERT))
}

fn declared(f: &Function) -> Ty {
    f.output
        .concrete
        .clone()
        .or_else(|| f.output.name.as_deref().and_then(named_type))
        .expect("module constants have a concrete type")
}

fn default(ty: &Ty) -> Value {
    match ty {
        Ty::Bool => Value::Bool(false),
        Ty::Str => Value::Str("".into()),
        Ty::ByteStr => Value::Bytes(Rc::from(&[][..])),
        t if t.is_float() => Value::Float(0.0, t.clone()),
        t => Value::Int(0, t.clone()),
    }
}

/// A literal expression denoting `value`, in the form the checker types
/// exactly as the constant's declared type.
fn literal(value: &Value, at: &Token) -> Expr {
    let expr = |kind| Expr {
        at: at.clone(),
        kind,
    };
    let negate = |inner: Expr| expr(Expression::Unary("-".into(), Box::new(inner)));
    match value {
        Value::Int(n, ty) => {
            let number = expr(Expression::Number(format!("{}{ty}", n.unsigned_abs())));
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

impl Folder<'_> {
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
        let ty = declared(f);
        let [
            Stmt {
                kind: Statement::Expr(initializer),
                ..
            },
        ] = f.body.as_slice()
        else {
            // An editor check clears a body that does not resolve.
            return Ok(default(&ty));
        };
        self.active.push(f.name.clone());
        let value = self.typed(initializer, &ty);
        self.active.pop();
        let value = value?;
        self.values.insert(f.name.clone(), value.clone());
        Ok(value)
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
            return Err(e.at.error(format!("expected {ty}, got {got}")));
        }
        Ok(value)
    }

    /// The type `e` has regardless of context, or None when an unsuffixed
    /// literal decides it.
    fn natural(&self, e: &Expr) -> Option<Ty> {
        match &e.kind {
            Expression::Group(inner) => self.natural(inner),
            Expression::Number(text) => {
                let split = text.find(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E');
                split.and_then(|i| named_type(&text[i..]))
            }
            Expression::Text(_) => Some(Ty::Str),
            Expression::Bytes(_) => Some(Ty::ByteStr),
            Expression::Bool(_) => Some(Ty::Bool),
            Expression::Call(name, args) if args.is_empty() && name.starts_with(GETTER) => {
                self.getters.get(name.as_str()).map(|f| declared(f))
            }
            Expression::Call(name, _) => named_type(name).filter(|t| t.is_int() || t.is_float()),
            Expression::Member(base, _, _) => match &base.kind {
                Expression::Name(n) => named_type(n),
                _ => None,
            },
            Expression::Unary(op, inner) if op != "not" => self.natural(inner),
            Expression::Unary(..) => Some(Ty::Bool),
            Expression::Binary(op, a, b) => match op.as_str() {
                "==" | "!=" | "<" | "<=" | ">" | ">=" | "and" | "or" => Some(Ty::Bool),
                "<<" | ">>" => self.natural(a),
                _ => self.natural(a).or_else(|| self.natural(b)),
            },
            Expression::Conditional { yes, no, .. } => {
                self.natural(yes).or_else(|| self.natural(no))
            }
            _ => None,
        }
    }

    fn eval(&mut self, e: &Expr, expected: Option<&Ty>) -> Result<Value> {
        match &e.kind {
            Expression::Group(inner) => self.eval(inner, expected),
            Expression::Number(text) => number(text, false, expected, &e.at),
            Expression::Text(t) => Ok(Value::Str(t.as_str().into())),
            Expression::Bytes(b) => Ok(Value::Bytes(b.as_slice().into())),
            Expression::Bool(b) => Ok(Value::Bool(*b)),
            Expression::Call(name, args) if args.is_empty() && name.starts_with(GETTER) => {
                let Some(f) = self.getters.get(name.as_str()).copied() else {
                    return Err(e.at.error(format!("unknown constant `{}`", display(name))));
                };
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
