//! Restricted Rust emission from the real checked operations; not a supported backend.
use crate::op::{CompiledFn, IntegerOp, Op, Pattern, Ty};
use crate::value::{Heap, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(test)]
mod tests;

/// Explicit roots and companion replacements, both named by checked Plenty symbol.
#[derive(Default)]
pub struct Options {
    pub exports: BTreeMap<String, String>,
    pub companions: BTreeMap<String, String>,
    /// Share monomorphic checked-arithmetic helpers instead of spelling each check inline.
    pub helpers: bool,
    /// Zero emits a flat module; a positive size groups stable sorted symbols.
    pub module_size: usize,
    /// Keep source-function boundaries; arithmetic helpers remain optimizable.
    pub no_inline: bool,
}

pub struct Emission {
    pub source: String,
    pub locations: BTreeMap<usize, String>,
    pub check_time: Duration,
    pub emit_time: Duration,
}

/// Check the complete module before selecting roots. Unsupported reachable operations fail.
pub fn emit_file(path: &Path, options: &Options) -> Result<Emission> {
    let start = Instant::now();
    let mut heap = Heap::default();
    let program = crate::frontend::compile_file(
        path,
        None,
        false,
        &mut heap,
        crate::CheckOptions::default(),
    )?;
    crate::op::check(&program.ops)?;
    let check_time = start.elapsed();
    let start = Instant::now();
    let functions: BTreeMap<_, _> = program
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::DefineFn(name, f) => Some((name.clone(), f.as_ref())),
            _ => None,
        })
        .collect();
    if options.exports.is_empty() {
        return Err("select at least one --export PlentyName=RustName".into());
    }
    let mut names = BTreeSet::new();
    for alias in options.exports.values() {
        if !identifier(alias)
            || matches!(alias.as_str(), "crate" | "__plenty")
            || !names.insert(alias)
        {
            return Err(format!("invalid or duplicate Rust export: {alias}").into());
        }
    }
    for (name, target) in &options.companions {
        if !functions.contains_key(name)
            || !target.starts_with("crate::")
            || !target.split("::").all(identifier)
        {
            return Err(format!(
                "companion requires a checked symbol and crate:: Rust path: {name}={target}"
            )
            .into());
        }
    }
    let mut pending: Vec<_> = options.exports.keys().cloned().collect();
    let mut selected = BTreeSet::new();
    while let Some(name) = pending.pop() {
        let f = functions.get(&name).ok_or_else(|| {
            format!(
                "unknown checked function {name}; available: {}",
                functions.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })?;
        if selected.insert(name.clone()) && !options.companions.contains_key(&name) {
            calls(&f.body, &mut |callee| pending.push(callee.to_owned()));
        }
    }
    // Self recursion can become a loop. Reject all other cycles, even where a
    // particular ordinary Rust call might happen to be optimized into a jump.
    for name in &selected {
        let mut todo = Vec::new();
        let mut seen = BTreeSet::new();
        if !options.companions.contains_key(name) {
            calls(&functions[name].body, &mut |n| {
                if n != name {
                    todo.push(n.to_owned());
                }
            });
        }
        while let Some(n) = todo.pop() {
            if n == *name {
                return Err(format!("Rust spike rejects mutual recursion involving {name}").into());
            }
            if seen.insert(n.clone()) && !options.companions.contains_key(&n) {
                calls(&functions[&n].body, &mut |next| todo.push(next.to_owned()));
            }
        }
    }
    let ids: BTreeMap<_, _> = selected
        .iter()
        .enumerate()
        .map(|(i, n)| {
            (
                n.clone(),
                match i.checked_div(options.module_size) {
                    None => format!("p{i}"),
                    Some(group) => format!("g{group}::p{i}"),
                },
            )
        })
        .collect();
    let mut out = Output::default();
    out.line(
        "// Generated experimental Plenty scalar module. No dependencies or unsafe code.",
        "",
    );
    out.line("#![allow(unused_mut, unused_variables, unused_assignments, unused_imports, dead_code, unused_labels, unreachable_code)]", "");
    out.line("#[cfg(not(panic = \"abort\"))] compile_error!(\"the Rust-source spike requires panic=abort\");", "");
    out.line("mod __plenty {", "");
    out.line(
        "#[cold] #[inline(never)] fn trap() -> ! { std::process::abort() }",
        "",
    );
    let mut helpers = BTreeSet::new();
    let mut current_group = None;
    for (index, name) in selected.iter().enumerate() {
        if let Some(group) = index.checked_div(options.module_size)
            && current_group != Some(group)
        {
            if current_group.is_some() {
                out.line("}", "");
            }
            out.line(&format!("pub mod g{group} {{ use super::*;"), "");
            current_group = Some(group);
        }
        let f = functions[name];
        let location = f.location.as_deref().unwrap_or(name);
        if f.generator.is_some() {
            return Err(format!("{location}: Rust spike rejects generators").into());
        }
        for ty in f
            .sig
            .inputs
            .iter()
            .map(|(_, t)| t)
            .chain(f.sig.outputs.iter())
            .chain(f.locals.iter())
        {
            scalar(ty).map_err(|e| format!("{location}: {e}"))?;
        }
        let inputs = f
            .sig
            .inputs
            .iter()
            .enumerate()
            .map(|(i, (_, ty))| Ok(format!("mut v{i}: {}", scalar(ty)?)))
            .collect::<Result<Vec<_>>>()?
            .join(", ");
        let result = f
            .sig
            .outputs
            .first()
            .map(scalar)
            .transpose()?
            .unwrap_or("()");
        if options.no_inline {
            out.line("#[inline(never)]", location);
        }
        out.line(
            &format!(
                "pub fn {}({inputs}) -> {result} {{",
                ids[name].rsplit("::").next().unwrap()
            ),
            location,
        );
        if let Some(target) = options.companions.get(name) {
            let args = (0..f.sig.inputs.len())
                .map(|i| format!("v{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            out.line(&format!("{target}({args})"), location);
        } else {
            let mut emitter = Function {
                name,
                f,
                functions: &functions,
                ids: &ids,
                options,
                out: &mut out,
                helpers: &mut helpers,
                next: 0,
                site: location.to_owned(),
                loops: 0,
            };
            emitter.out.line("'tail: loop {", location);
            for (i, ty) in f.locals.iter().enumerate() {
                emitter.out.line(
                    &format!(
                        "let mut v{}: {} = {};",
                        i + f.sig.inputs.len(),
                        scalar(ty)?,
                        if *ty == Ty::Bool { "false" } else { "0" }
                    ),
                    location,
                );
            }
            let mut stack = Vec::new();
            let continues = emitter.ops(&f.body, &mut stack)?;
            if continues {
                emitter.return_value(&mut stack)?;
            }
            emitter.out.line("}", location);
        }
        out.line("}", location);
    }
    if current_group.is_some() {
        out.line("}", "");
    }
    for (method, ty) in helpers {
        out.line(
            &format!(
                "fn {method}_{ty}(a: {ty}, b: {ty}) -> {ty} {{ {} }}",
                arithmetic(&method, &ty, "a", "b")
            ),
            "",
        );
    }
    out.line("}", "");
    for (name, alias) in &options.exports {
        out.line(
            &format!("pub use __plenty::{} as r#{alias};", ids[name]),
            functions[name].location.as_deref().unwrap_or(name),
        );
    }
    Ok(Emission {
        source: out.text,
        locations: out.locations,
        check_time,
        emit_time: start.elapsed(),
    })
}

fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s != "_"
        && s.bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
        && !matches!(s, "self" | "Self" | "super")
}

fn scalar(ty: &Ty) -> Result<&'static str> {
    Ok(match ty {
        Ty::I8 => "i8",
        Ty::I16 => "i16",
        Ty::I32 => "i32",
        Ty::I64 => "i64",
        Ty::U8 => "u8",
        Ty::U16 => "u16",
        Ty::U32 => "u32",
        Ty::U64 => "u64",
        Ty::Bool => "bool",
        _ => {
            return Err(
                format!("Rust spike supports only integer/bool scalar storage, not {ty}").into(),
            );
        }
    })
}

fn arithmetic(method: &str, ty: &str, a: &str, b: &str) -> String {
    let checked =
        |op: &str| format!("match {a}.checked_{op}({b}) {{ Some(v) => v, None => trap() }}");
    match method {
        "floor_div" if ty.starts_with('i') => format!(
            "{{ let q = {}; let r = {}; if r != 0 && ((r < 0) != ({b} < 0)) {{ match q.checked_sub(1) {{ Some(v) => v, None => trap() }} }} else {{ q }} }}",
            checked("div"),
            checked("rem")
        ),
        // Unlike signed division, Plenty MIN % -1 is zero. Avoid Rust's overflow trap.
        "modulo" if ty.starts_with('i') => format!(
            "{{ let r = if {b} == -1 {{ 0 }} else {{ {} }}; if r != 0 && ((r < 0) != ({b} < 0)) {{ match r.checked_add({b}) {{ Some(v) => v, None => trap() }} }} else {{ r }} }}",
            checked("rem")
        ),
        "floor_div" => checked("div"),
        "modulo" => checked("rem"),
        _ => checked(method),
    }
}

fn calls(ops: &[Op], visit: &mut impl FnMut(&str)) {
    for op in ops {
        match op {
            Op::Call(n) | Op::TailCall(n) => visit(n),
            Op::Loop { condition, body } => {
                calls(condition, visit);
                calls(body, visit);
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                calls(then, visit);
                for arm in elifs.iter() {
                    calls(&arm.condition, visit);
                    calls(&arm.body, visit);
                }
                calls(otherwise, visit);
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    calls(&arm.body, visit);
                }
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct Output {
    text: String,
    locations: BTreeMap<usize, String>,
    lines: usize,
}
impl Output {
    fn append(&mut self, other: Self) {
        for (line, site) in other.locations {
            self.locations.insert(self.lines + line, site);
        }
        self.lines += other.lines;
        self.text.push_str(&other.text);
    }
    fn line(&mut self, text: &str, site: &str) {
        self.lines += 1;
        writeln!(self.text, "{text}").unwrap();
        if !site.is_empty() {
            self.locations.insert(self.lines, site.to_owned());
        }
    }
}
#[derive(Clone)]
struct Operand {
    name: String,
    ty: Ty,
}
struct Function<'a> {
    name: &'a str,
    f: &'a CompiledFn,
    functions: &'a BTreeMap<String, &'a CompiledFn>,
    ids: &'a BTreeMap<String, String>,
    options: &'a Options,
    out: &'a mut Output,
    helpers: &'a mut BTreeSet<(String, String)>,
    next: usize,
    site: String,
    loops: usize,
}
impl Function<'_> {
    fn branch(&mut self, then: &[Op], otherwise: &[Op], stack: &mut Vec<Operand>) -> Result<bool> {
        let condition = self.pop(stack)?;
        let parent = std::mem::take(self.out);
        let mut a = stack.clone();
        let ca = self.ops(then, &mut a)?;
        if ca {
            self.line(&format!(
                "({})",
                a.iter().map(|a| format!("{},", a.name)).collect::<String>()
            ));
        }
        let then_output = std::mem::take(self.out);
        let mut b = stack.clone();
        let cb = self.ops(otherwise, &mut b)?;
        if cb {
            self.line(&format!(
                "({})",
                b.iter().map(|a| format!("{},", a.name)).collect::<String>()
            ));
        }
        let else_output = std::mem::replace(self.out, parent);
        let values = if ca { a } else { b };
        let joined = values
            .into_iter()
            .map(|v| {
                let name = format!("t{}", self.next);
                self.next += 1;
                Operand { name, ty: v.ty }
            })
            .collect::<Vec<_>>();
        let pattern = joined
            .iter()
            .map(|v| format!("{},", v.name))
            .collect::<String>();
        self.line(&format!("let ({pattern}) = if {} {{", condition.name));
        self.out.append(then_output);
        self.line("} else {");
        self.out.append(else_output);
        self.line("};");
        *stack = joined;
        Ok(ca || cb)
    }
    fn line(&mut self, text: &str) {
        self.out.line(text, &self.site);
    }
    fn temp(&mut self, stack: &mut Vec<Operand>, ty: Ty, expr: String) -> Result<()> {
        let name = format!("t{}", self.next);
        self.next += 1;
        self.line(&format!("let {name}: {} = {expr};", scalar(&ty)?));
        stack.push(Operand { name, ty });
        Ok(())
    }
    fn pop(&self, stack: &mut Vec<Operand>) -> Result<Operand> {
        stack
            .pop()
            .ok_or_else(|| format!("{}: emitter operand underflow", self.site).into())
    }
    fn return_value(&mut self, stack: &mut Vec<Operand>) -> Result<()> {
        let result = if self.f.sig.outputs.is_empty() {
            "()".into()
        } else {
            self.pop(stack)?.name
        };
        if !stack.is_empty() {
            return Err("Rust spike requires empty residual operand stack".into());
        }
        self.line(&format!("return {result};"));
        Ok(())
    }
    fn ops(&mut self, ops: &[Op], stack: &mut Vec<Operand>) -> Result<bool> {
        for op in ops {
            match op {
                Op::Site(site) | Op::CallerSite(site) => {
                    self.site = format!(
                        "{}:{}:{}",
                        self.f.origins.source.as_deref().unwrap_or("<source>"),
                        site.line,
                        site.column
                    );
                }
                // The complete program was independently checked; all admitted
                // storage is Copy scalar, so these facts need no runtime action.
                Op::Access(..)
                | Op::Building(_)
                | Op::Returning(..)
                | Op::Leaving
                | Op::Loan(_)
                | Op::UseLoan(_)
                | Op::EndLoan(_) => {}
                Op::PushInt(value) => {
                    let (ty, text) = match value {
                        Value::I8(v) => (Ty::I8, v.to_string()),
                        Value::I16(v) => (Ty::I16, v.to_string()),
                        Value::I32(v) => (Ty::I32, v.to_string()),
                        Value::I64(v) => (Ty::I64, v.to_string()),
                        Value::U8(v) => (Ty::U8, v.to_string()),
                        Value::U16(v) => (Ty::U16, v.to_string()),
                        Value::U32(v) => (Ty::U32, v.to_string()),
                        Value::U64(v) => (Ty::U64, v.to_string()),
                    };
                    self.temp(stack, ty, text)?;
                }
                Op::PushBool(v) => self.temp(stack, Ty::Bool, v.to_string())?,
                Op::LoadLocal(i) => {
                    let ty = self
                        .f
                        .sig
                        .inputs
                        .get(*i as usize)
                        .map(|(_, t)| t)
                        .unwrap_or_else(|| &self.f.locals[*i as usize - self.f.sig.inputs.len()])
                        .clone();
                    self.temp(stack, ty, format!("v{i}"))?;
                }
                Op::StoreLocal(i) => {
                    let value = self.pop(stack)?;
                    self.line(&format!("v{i} = {};", value.name));
                }
                Op::Drop => {
                    self.pop(stack)?;
                }
                Op::Dup => {
                    let v = stack.last().ok_or("dup underflow")?.clone();
                    stack.push(v);
                }
                Op::Swap => {
                    let b = self.pop(stack)?;
                    let a = self.pop(stack)?;
                    stack.push(b);
                    stack.push(a);
                }
                Op::Add | Op::Sub | Op::Mul | Op::FloorDiv | Op::Modulo => {
                    let b = self.pop(stack)?;
                    let a = self.pop(stack)?;
                    let method = match op {
                        Op::Add => "add",
                        Op::Sub => "sub",
                        Op::Mul => "mul",
                        Op::FloorDiv => "floor_div",
                        _ => "modulo",
                    };
                    let expr = if self.options.helpers {
                        let ty = scalar(&a.ty)?;
                        self.helpers.insert((method.into(), ty.into()));
                        format!("{method}_{ty}({}, {})", a.name, b.name)
                    } else {
                        arithmetic(method, scalar(&a.ty)?, &a.name, &b.name)
                    };
                    self.temp(stack, a.ty, expr)?;
                }
                Op::Integer(
                    IntegerOp::WrappingAdd | IntegerOp::WrappingSub | IntegerOp::WrappingMul,
                ) => {
                    let b = self.pop(stack)?;
                    let a = self.pop(stack)?;
                    let method = match op {
                        Op::Integer(IntegerOp::WrappingAdd) => "wrapping_add",
                        Op::Integer(IntegerOp::WrappingSub) => "wrapping_sub",
                        _ => "wrapping_mul",
                    };
                    self.temp(stack, a.ty, format!("{}.{method}({})", a.name, b.name))?;
                }
                Op::Eq | Op::Ne | Op::Lt | Op::Gt | Op::Le | Op::Ge | Op::And | Op::Or => {
                    let b = self.pop(stack)?;
                    let a = self.pop(stack)?;
                    let operator = match op {
                        Op::Eq => "==",
                        Op::Ne => "!=",
                        Op::Lt => "<",
                        Op::Gt => ">",
                        Op::Le => "<=",
                        Op::Ge => ">=",
                        Op::And => "&",
                        _ => "|",
                    };
                    self.temp(stack, Ty::Bool, format!("{} {operator} {}", a.name, b.name))?;
                }
                Op::Not => {
                    let a = self.pop(stack)?;
                    self.temp(stack, Ty::Bool, format!("!{}", a.name))?;
                }
                Op::Cast(ty) | Op::CheckedCast(ty) => {
                    let a = self.pop(stack)?;
                    if !a.ty.is_int() || !ty.is_int() {
                        return Err(format!("{}: only integer casts supported", self.site).into());
                    }
                    let expr = if matches!(op, Op::Cast(_)) {
                        format!("{} as {}", a.name, scalar(ty)?)
                    } else {
                        format!(
                            "match {}::try_from({}) {{ Ok(v) => v, Err(_) => trap() }}",
                            scalar(ty)?,
                            a.name
                        )
                    };
                    self.temp(stack, ty.clone(), expr)?;
                }
                Op::Call(name) | Op::TailCall(name) => {
                    let sig = &self.functions[name].sig;
                    let count = sig.inputs.len();
                    if stack.len() < count {
                        return Err("call operand underflow".into());
                    }
                    let args = stack.split_off(stack.len() - count);
                    if name == self.name {
                        if !matches!(op, Op::TailCall(_)) {
                            return Err(format!(
                                "{}: Rust spike rejects non-tail recursion",
                                self.site
                            )
                            .into());
                        }
                        for (i, arg) in args.iter().enumerate() {
                            self.line(&format!("v{i} = {};", arg.name));
                        }
                        self.line("continue 'tail;");
                        return Ok(false);
                    }
                    let call = format!(
                        "{}({})",
                        self.ids[name],
                        args.iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    if let Some(ty) = sig.outputs.first() {
                        self.temp(stack, ty.clone(), call)?;
                    } else {
                        self.line(&format!("{call};"));
                    }
                    if matches!(op, Op::TailCall(_)) {
                        self.return_value(stack)?;
                        return Ok(false);
                    }
                }
                Op::Return => {
                    self.return_value(stack)?;
                    return Ok(false);
                }
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    let mut rest = otherwise.to_vec();
                    for arm in elifs.iter().rev() {
                        let nested = Op::If {
                            then: arm.body.clone(),
                            elifs: vec![].into(),
                            otherwise: rest.into(),
                        };
                        rest = arm.condition.to_vec();
                        rest.push(nested);
                    }
                    if !self.branch(then, &rest, stack)? {
                        return Ok(false);
                    }
                }
                Op::Match(arms) => {
                    let ty = stack.last().ok_or("match underflow")?.ty.clone();
                    let mut rest = vec![Op::Unreachable];
                    for arm in arms.iter().rev() {
                        let mut body = vec![Op::Drop];
                        body.extend_from_slice(&arm.body);
                        let pattern = match arm.pattern {
                            Pattern::Int { value, .. } => Op::PushInt(value),
                            Pattern::Bool(value) => Op::PushBool(value),
                            Pattern::Wildcard => {
                                rest = body;
                                continue;
                            }
                            _ => {
                                return Err(format!(
                                    "{}: Rust spike rejects non-scalar match",
                                    self.site
                                )
                                .into());
                            }
                        };
                        let branch = Op::If {
                            then: body.into(),
                            elifs: vec![].into(),
                            otherwise: rest.into(),
                        };
                        rest = vec![Op::Dup, pattern];
                        // Unsuffixed pattern literals retain i64 in checked IR
                        // even when matching a narrower integer scrutinee.
                        if ty.is_int() {
                            rest.push(Op::Cast(ty.clone()));
                        }
                        rest.extend([Op::Eq, branch]);
                    }
                    if !self.ops(&rest, stack)? {
                        return Ok(false);
                    }
                }
                Op::Loop { condition, body } => {
                    if !stack.is_empty() {
                        return Err("Rust spike requires empty loop entry stack".into());
                    }
                    self.line("loop {");
                    let mut cond = Vec::new();
                    self.ops(condition, &mut cond)?;
                    let test = self.pop(&mut cond)?;
                    if !cond.is_empty() {
                        return Err("loop condition residual operands".into());
                    }
                    self.line(&format!("if !{} {{ break; }}", test.name));
                    self.loops += 1;
                    let mut body_stack = Vec::new();
                    let continues = self.ops(body, &mut body_stack)?;
                    self.loops -= 1;
                    if continues && !body_stack.is_empty() {
                        return Err("loop residual operands".into());
                    }
                    self.line("}");
                }
                Op::Break | Op::Continue if self.loops > 0 => {
                    self.line(if matches!(op, Op::Break) {
                        "break;"
                    } else {
                        "continue;"
                    });
                    return Ok(false);
                }
                Op::Unreachable => {
                    self.line("trap();");
                    return Ok(false);
                }
                other => {
                    return Err(format!(
                        "{}: Rust spike unsupported operation: {other:?}",
                        self.site
                    )
                    .into());
                }
            }
        }
        Ok(true)
    }
}
