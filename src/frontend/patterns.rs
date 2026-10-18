//! `match` patterns: their syntax, the check against the matched type, and the
//! decision tree that tests each variant tag and each literal at most once per
//! path. The reference is `book/src/design/language/04-expressions/matching.md`.
use super::*;
use crate::boxed::BoxOp;
use crate::nominal::Nominal;
use crate::sum::{EnumOp, EnumType};

/// Recursion bound for the parser and for every later walk over a pattern.
const MAX_DEPTH: usize = 64;
/// `|` alternatives multiply across payload positions; each product is one row.
const MAX_ALTERNATIVES: usize = 256;
/// Bounds decision-tree growth for adversarial combinations of nested patterns.
const MAX_TESTS: usize = 16_384;
const MAX_WITNESSES: usize = 8;

#[derive(Clone)]
pub(super) struct Pat {
    pub(super) at: Token,
    pub(super) kind: PatKind,
}

#[derive(Clone)]
pub(super) enum PatKind {
    Wildcard,
    /// A bare name at a payload position, or `_ as name` at the case position.
    Binding(String),
    Member(String, Box<TypeRef>),
    /// Qualifier (absent for `Some`, `Nothing`, `Ok`, and `Err`), variant name,
    /// and payload patterns (absent without parentheses).
    Variant(Option<Box<TypeRef>>, String, Option<Vec<Pat>>),
    Or(Vec<Pat>),
    Literal(Literal),
}

#[derive(Clone)]
pub(super) enum Literal {
    /// The digits as written, with any suffix, and whether a `-` precedes them.
    Integer(String, bool),
    Text(String),
    Bool(bool),
}

impl Pat {
    /// Every name the pattern binds, once each, in source order.
    pub(super) fn bindings(&self) -> Vec<&String> {
        fn walk<'a>(pattern: &'a Pat, names: &mut Vec<&'a String>) {
            match &pattern.kind {
                PatKind::Wildcard | PatKind::Literal(_) => {}
                PatKind::Binding(name) | PatKind::Member(name, _) => {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
                PatKind::Variant(_, _, payload) => {
                    for pattern in payload.iter().flatten() {
                        walk(pattern, names);
                    }
                }
                PatKind::Or(alternatives) => {
                    for pattern in alternatives {
                        walk(pattern, names);
                    }
                }
            }
        }
        let mut names = Vec::new();
        walk(self, &mut names);
        names
    }

    pub(super) fn qualifiers_mut(
        &mut self,
        visit: &mut dyn FnMut(&mut TypeRef) -> Result<()>,
    ) -> Result<()> {
        match &mut self.kind {
            PatKind::Member(_, ty) => visit(ty),
            PatKind::Wildcard | PatKind::Binding(_) | PatKind::Literal(_) => Ok(()),
            PatKind::Variant(qualifier, _, payload) => {
                if let Some(qualifier) = qualifier {
                    visit(qualifier)?;
                }
                for pattern in payload.iter_mut().flatten() {
                    pattern.qualifiers_mut(visit)?;
                }
                Ok(())
            }
            PatKind::Or(alternatives) => {
                for pattern in alternatives {
                    pattern.qualifiers_mut(visit)?;
                }
                Ok(())
            }
        }
    }
}

impl Parser {
    /// `depth` is 0 at the case position, where a bare name cannot bind and
    /// `_ as name` binds the whole value.
    pub(super) fn pattern(&mut self, depth: usize) -> Result<Pat> {
        let first = self.pattern_alternative(depth)?;
        if !self.peek().is("|") {
            return Ok(first);
        }
        let at = first.at.clone();
        let mut alternatives = vec![first];
        while self.eat("|") {
            alternatives.push(self.pattern_alternative(depth)?);
        }
        if depth == 0 {
            // Only `_ as name` yields a binding here. It matches every value,
            // so beside it any other alternative would be dead or would leave
            // the name unbound.
            if let Some(Pat {
                at,
                kind: PatKind::Binding(name),
            }) = alternatives
                .iter()
                .find(|p| matches!(p.kind, PatKind::Binding(_)))
            {
                return Err(at.error(format!(
                    "`_ as {name}` cannot be a `|` alternative; give it a case of its own"
                )));
            }
        }
        Ok(Pat {
            at,
            kind: PatKind::Or(alternatives),
        })
    }

    fn pattern_alternative(&mut self, depth: usize) -> Result<Pat> {
        let at = self.peek().clone();
        if depth > MAX_DEPTH {
            return Err(at.error("pattern nesting exceeds the implementation limit of 64"));
        }
        if self.eat("_") {
            if !self.peek().is("as") {
                return Ok(Pat {
                    at,
                    kind: PatKind::Wildcard,
                });
            }
            let keyword = self.take();
            let at = self.peek().clone();
            if at.is("mut") {
                return Err(at.error(
                    "a whole-value binding cannot be declared `mut`; move it into a mutable local in the arm, as in `mut local = name`, or match `&mut value` to change it in place",
                ));
            }
            let name = self.name()?;
            if depth != 0 {
                return Err(keyword.error(format!(
                    "`_ as {name}` binds only the whole matched value; bind a payload with the name alone, as in `{name}`"
                )));
            }
            if name == "_" {
                return Err(at.error("`_ as` needs a name; match any value with `_` alone"));
            }
            return Ok(Pat {
                at,
                kind: PatKind::Binding(name),
            });
        }
        if let Some(literal) = self.literal_pattern()? {
            return Ok(Pat {
                at,
                kind: PatKind::Literal(literal),
            });
        }
        if at.is("mut") {
            return Err(at.error(
                "payload bindings cannot be declared `mut`; bind the payload and move it into a mutable local in the arm, as in `mut local = payload`, or match `&mut value` to change it in place",
            ));
        }
        if at.is("(") {
            return Err(at.error(
                "tuple patterns are not supported; bind the tuple and unpack it in the arm",
            ));
        }
        let mut path = vec![self.name()?];
        if depth == 0
            && !enums::prelude_variant(&path[0])
            && self.peek().is(":")
            && self
                .token(self.pos + 1)
                .is_some_and(|t| !matches!(t.kind, Kind::Newline | Kind::Eof))
            && self.eat(":")
        {
            let ty = self.ty()?;
            return Ok(Pat {
                at,
                kind: PatKind::Member(path.pop().unwrap(), Box::new(ty)),
            });
        }
        while self.eat(".") {
            path.push(self.name()?);
        }
        if self.peek().is("[") {
            return Err(self.type_arguments_in_pattern(&path.join(".")));
        }
        let name = path.pop().unwrap();
        let qualifier = (!path.is_empty()).then(|| {
            Box::new(TypeRef {
                concrete: None,
                at: at.clone(),
                name: Some(path.join(".")),
                args: vec![],
            })
        });
        if qualifier.is_none() && !enums::prelude_variant(&name) {
            if self.peek().is("(") {
                return Err(at.error("qualify user-defined variants with their enum type"));
            }
            if depth == 0 {
                return Err(at.error(format!(
                    "a case cannot bind the whole value to the bare name `{name}`; bind it with `_ as {name}`, match any value with `_`, or qualify a variant with its enum type"
                )));
            }
            return Ok(Pat {
                at,
                kind: PatKind::Binding(name),
            });
        }
        let payload = if self.eat("(") {
            let mut payload = Vec::new();
            while !self.eat(")") {
                payload.push(self.pattern(depth + 1)?);
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
            Some(payload)
        } else {
            None
        };
        Ok(Pat {
            at,
            kind: PatKind::Variant(qualifier, name, payload),
        })
    }

    fn literal_pattern(&mut self) -> Result<Option<Literal>> {
        let at = self.peek().clone();
        let negative = at.is("-")
            && self
                .token(self.pos + 1)
                .is_some_and(|t| matches!(t.kind, Kind::Number(_)));
        if negative {
            self.take();
        }
        let literal = match &self.peek().kind {
            Kind::Number(text) => {
                if text.contains(['.', 'e', 'E']) || text.ends_with("f32") || text.ends_with("f64")
                {
                    return Err(at.error(
                        "a pattern cannot be a float; compare floats with an `if`/`elif` chain",
                    ));
                }
                Literal::Integer(text.clone(), negative)
            }
            Kind::Text(text) => Literal::Text(text.clone()),
            Kind::Format(_) => {
                return Err(
                    at.error("a pattern cannot be an f-string; match a plain string literal")
                )
            }
            Kind::Bytes(_) => {
                return Err(at.error("a pattern cannot be a byte literal; compare with `==`"))
            }
            Kind::Word(word) if word == "True" || word == "False" => Literal::Bool(word == "True"),
            _ => return Ok(None),
        };
        self.take();
        Ok(Some(literal))
    }

    /// Names the accepted spelling when it can be read past the brackets.
    fn type_arguments_in_pattern(&self, qualifier: &str) -> Box<dyn Error> {
        let at = self.peek();
        let mut depth = 0usize;
        let mut end = self.pos;
        while let Some(token) = self.token(end) {
            if token.is("[") {
                depth += 1;
            } else if token.is("]") {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            } else if matches!(token.kind, Kind::Newline | Kind::Eof) {
                break;
            }
            end += 1;
        }
        let variant = match (self.token(end + 1), self.token(end + 2)) {
            (
                Some(dot),
                Some(Token {
                    kind: Kind::Word(name),
                    ..
                }),
            ) if dot.is(".") => Some(name),
            _ => None,
        };
        let advice = match variant {
            Some(name) if enums::prelude_variant(name) => format!("; write `{name}`"),
            Some(name) => format!("; write `{qualifier}.{name}`"),
            None => String::new(),
        };
        at.error(format!(
            "a pattern takes no type arguments, because the matched value supplies them{advice}"
        ))
    }
}

/// A pattern resolved against the type at its position.
#[derive(Clone)]
enum Checked {
    Wildcard,
    Binding(String),
    Variant {
        /// Contents of the boxes around the enum at this position, outermost
        /// first: a variant pattern matches through them.
        boxes: Vec<Ty>,
        t: Nominal<EnumType>,
        tag: usize,
        fields: Vec<Checked>,
    },
    /// Each alternative keeps its position for the unreachable diagnostic.
    Or(Vec<(Token, Checked)>),
    Literal {
        /// As for a variant: a literal matches through the boxes around it.
        boxes: Vec<Ty>,
        ty: Ty,
        value: Constant,
    },
}

/// The value a literal pattern names, in the type at its position.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Constant {
    Int(Value),
    Str(String),
    Bool(bool),
}

impl Constant {
    /// The constant as a pattern would spell it.
    fn text(&self) -> String {
        match self {
            Constant::Int(value) => match value {
                Value::I8(n) => n.to_string(),
                Value::I16(n) => n.to_string(),
                Value::I32(n) => n.to_string(),
                Value::I64(n) => n.to_string(),
                Value::U8(n) => n.to_string(),
                Value::U16(n) => n.to_string(),
                Value::U32(n) => n.to_string(),
                Value::U64(n) => n.to_string(),
            },
            Constant::Str(text) => format!("{text:?}"),
            Constant::Bool(true) => "True".into(),
            Constant::Bool(false) => "False".into(),
        }
    }
}

/// An unsuffixed integer takes the integer type at its position; a suffixed
/// one must already have it.
fn integer_constant(text: &str, negative: bool, ty: &Ty, at: &Token) -> Result<Value> {
    if text.contains(|c: char| c.is_ascii_alphabetic()) {
        let value = integer(text, negative, at)?;
        let written = Ty::from(value);
        if written != *ty {
            return Err(at.error(format!(
                "this pattern has type {written}, but the value it matches has type {ty}"
            )));
        }
        return Ok(value);
    }
    if negative && ty.int_range().is_some_and(|(min, _)| min == 0) {
        return Err(at.error(format!(
            "a negative pattern cannot match a value of type {ty}"
        )));
    }
    integer(&format!("{text}{ty}"), negative, at)
}

/// A pattern free of `|`, with the alternatives chosen to reach it.
type Alternative = (Checked, Vec<usize>);

impl Checked {
    fn binds(&self) -> bool {
        match self {
            Checked::Wildcard | Checked::Literal { .. } => false,
            Checked::Binding(_) => true,
            Checked::Variant { fields, .. } => fields.iter().any(Checked::binds),
            Checked::Or(alternatives) => alternatives.iter().any(|(_, p)| p.binds()),
        }
    }

    /// A variant whose payload positions all match anything.
    fn plain_variant(&self) -> Option<usize> {
        match self {
            Checked::Variant { tag, fields, .. }
                if fields
                    .iter()
                    .all(|f| matches!(f, Checked::Wildcard | Checked::Binding(_))) =>
            {
                Some(*tag)
            }
            _ => None,
        }
    }

    /// One pattern per combination of `|` alternatives, in source order.
    /// `tokens` numbers every alternative met on the way.
    fn alternatives(&self, tokens: &mut Vec<Token>) -> Option<Vec<Alternative>> {
        Some(match self {
            Checked::Wildcard | Checked::Binding(_) | Checked::Literal { .. } => {
                vec![(self.clone(), Vec::new())]
            }
            Checked::Or(alternatives) => {
                let mut rows = Vec::new();
                for (at, alternative) in alternatives {
                    let id = tokens.len();
                    tokens.push(at.clone());
                    for (pattern, mut chosen) in alternative.alternatives(tokens)? {
                        chosen.push(id);
                        rows.push((pattern, chosen));
                    }
                    if rows.len() > MAX_ALTERNATIVES {
                        return None;
                    }
                }
                rows
            }
            Checked::Variant {
                boxes,
                t,
                tag,
                fields,
            } => {
                let mut payloads: Vec<(Vec<Checked>, Vec<usize>)> = vec![(Vec::new(), Vec::new())];
                for field in fields {
                    let options = field.alternatives(tokens)?;
                    if payloads.len() * options.len() > MAX_ALTERNATIVES {
                        return None;
                    }
                    payloads = payloads
                        .iter()
                        .flat_map(|(payload, chosen)| {
                            options.iter().map(move |(option, choice)| {
                                let mut payload = payload.clone();
                                payload.push(option.clone());
                                let mut chosen = chosen.clone();
                                chosen.extend(choice);
                                (payload, chosen)
                            })
                        })
                        .collect();
                }
                payloads
                    .into_iter()
                    .map(|(fields, chosen)| {
                        (
                            Checked::Variant {
                                boxes: boxes.clone(),
                                t: t.clone(),
                                tag: *tag,
                                fields,
                            },
                            chosen,
                        )
                    })
                    .collect()
            }
        })
    }

    /// Where each binding sits below the matched value.
    fn paths<'a>(&'a self, prefix: &mut Vec<Step>, out: &mut Vec<(&'a str, Vec<Step>)>) {
        match self {
            Checked::Wildcard | Checked::Or(_) | Checked::Literal { .. } => {}
            Checked::Binding(name) => out.push((name, prefix.clone())),
            Checked::Variant {
                boxes,
                t,
                tag,
                fields,
            } => {
                let start = prefix.len();
                prefix.extend(boxes.iter().cloned().map(Step::Unbox));
                for (field, pattern) in fields.iter().enumerate() {
                    prefix.push(Step::Field(t.clone(), *tag, field));
                    pattern.paths(prefix, out);
                    prefix.pop();
                }
                prefix.truncate(start);
            }
        }
    }
}

/// One projection from a matched value toward a payload position.
#[derive(Clone)]
enum Step {
    Field(Nominal<EnumType>, usize, usize),
    Unbox(Ty),
}

fn project(steps: &[Step], mutable: bool, ops: &mut Vec<Op>) {
    for step in steps {
        ops.push(match step {
            Step::Field(t, tag, field) => {
                Op::Enum(EnumOp::FieldRef(t.clone(), *tag, *field, mutable))
            }
            Step::Unbox(content) => Op::Box(BoxOp::Ref(content.clone(), mutable)),
        });
    }
}

struct Row<'a> {
    patterns: Vec<&'a Checked>,
    index: usize,
}

impl Row<'_> {
    fn duplicate(&self) -> Self {
        Row {
            patterns: self.patterns.clone(),
            index: self.index,
        }
    }
}

enum Tree {
    /// The row selected when every earlier test succeeded.
    Leaf(usize),
    /// No row matches; reported as a non-exhaustive match before lowering.
    Fail,
    Switch {
        steps: Vec<Step>,
        t: Nominal<EnumType>,
        branches: Vec<(usize, Tree)>,
        /// Present only while some variant has no branch of its own.
        default: Option<Box<Tree>>,
    },
    /// Compare the integer, string, or boolean at a position with each
    /// constant in turn.
    Compare {
        steps: Vec<Step>,
        ty: Ty,
        branches: Vec<(Constant, Tree)>,
        /// Absent only when `True` and `False` both have a branch.
        default: Option<Box<Tree>>,
    },
}

impl Tree {
    fn leaves(&self, counts: &mut [usize], arms: &[usize]) {
        match self {
            Tree::Leaf(row) => counts[arms[*row]] += 1,
            Tree::Fail => {}
            Tree::Switch {
                branches, default, ..
            } => {
                for (_, tree) in branches {
                    tree.leaves(counts, arms);
                }
                if let Some(tree) = default {
                    tree.leaves(counts, arms);
                }
            }
            Tree::Compare {
                branches, default, ..
            } => {
                for (_, tree) in branches {
                    tree.leaves(counts, arms);
                }
                if let Some(tree) = default {
                    tree.leaves(counts, arms);
                }
            }
        }
    }
}

struct Compiler<'a> {
    wildcard: &'a Checked,
    reached: Vec<bool>,
    tests: usize,
}

impl<'a> Compiler<'a> {
    /// Returns the tree for `rows` and the value shapes no row matches, one
    /// rendered pattern per column.
    fn compile(
        &mut self,
        rows: Vec<Row<'a>>,
        occurrences: &[Vec<Step>],
    ) -> (Tree, Vec<Vec<String>>) {
        let Some(first) = rows.first() else {
            return (Tree::Fail, vec![vec!["_".into(); occurrences.len()]]);
        };
        let Some(column) = first
            .patterns
            .iter()
            .position(|p| matches!(p, Checked::Variant { .. } | Checked::Literal { .. }))
        else {
            self.reached[first.index] = true;
            return (Tree::Leaf(first.index), vec![]);
        };
        self.tests += 1;
        if self.tests > MAX_TESTS {
            return (Tree::Fail, vec![]);
        }
        let (boxes, t) = match first.patterns[column] {
            Checked::Variant { boxes, t, .. } => (boxes, t),
            Checked::Literal { boxes, ty, .. } => {
                return self.compare(rows, occurrences, column, boxes, ty)
            }
            _ => unreachable!(),
        };
        let definition = t.get();
        // Rows grouped by the variant they name, variants in the order the
        // cases first name them. Grouping in one pass keeps a match with one
        // case per variant linear in the number of variants (issue #71).
        let mut index: HashMap<usize, usize> = HashMap::new();
        let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
        let mut rest = Vec::new();
        for (position, row) in rows.iter().enumerate() {
            if let Checked::Variant { tag, .. } = row.patterns[column] {
                let group = *index.entry(*tag).or_insert_with(|| {
                    groups.push((*tag, Vec::new()));
                    groups.len() - 1
                });
                groups[group].1.push(position);
            } else {
                rest.push(position);
            }
        }
        let mut steps = occurrences[column].clone();
        steps.extend(boxes.iter().cloned().map(Step::Unbox));
        let mut missing = Vec::new();
        let mut branches = Vec::new();
        for (tag, named) in &groups {
            let tag = *tag;
            let arity = definition.variants[tag].fields.len();
            let wildcard = self.wildcard;
            let specialize = |position: usize| {
                let row = &rows[position];
                let mut patterns = row.patterns[..column].to_vec();
                match row.patterns[column] {
                    Checked::Variant { fields, .. } => patterns.extend(fields),
                    _ => patterns.extend(std::iter::repeat_n(wildcard, arity)),
                }
                patterns.extend(&row.patterns[column + 1..]);
                Row {
                    patterns,
                    index: row.index,
                }
            };
            // A row naming no variant joins every branch, in source order.
            let mut specialized = Vec::with_capacity(named.len() + rest.len());
            let mut others = rest.iter().copied().peekable();
            for &position in named {
                while let Some(other) = others.next_if(|&other| other < position) {
                    specialized.push(specialize(other));
                }
                specialized.push(specialize(position));
            }
            specialized.extend(others.map(specialize));
            let mut inner = occurrences[..column].to_vec();
            inner.extend((0..arity).map(|field| {
                let mut steps = steps.clone();
                steps.push(Step::Field(t.clone(), tag, field));
                steps
            }));
            inner.extend_from_slice(&occurrences[column + 1..]);
            let (tree, witnesses) = self.compile(specialized, &inner);
            for mut witness in witnesses {
                let payload: Vec<String> = witness.drain(column..column + arity).collect();
                witness.insert(column, variant_text(t, tag, &payload));
                missing.push(witness);
            }
            branches.push((tag, tree));
        }
        let default = if groups.len() == definition.variants.len() {
            None
        } else {
            let rest = rest
                .iter()
                .map(|&position| {
                    let row = &rows[position];
                    let mut patterns = row.patterns.clone();
                    patterns.remove(column);
                    Row {
                        patterns,
                        index: row.index,
                    }
                })
                .collect();
            let mut outer = occurrences.to_vec();
            outer.remove(column);
            let (tree, witnesses) = self.compile(rest, &outer);
            for witness in witnesses {
                for (tag, variant) in definition.variants.iter().enumerate() {
                    if !index.contains_key(&tag) {
                        let mut witness = witness.clone();
                        let payload = vec!["_".to_string(); variant.fields.len()];
                        witness.insert(column, variant_text(t, tag, &payload));
                        missing.push(witness);
                    }
                }
            }
            Some(Box::new(tree))
        };
        // One past the limit records that the reported list is incomplete.
        missing.truncate(MAX_WITNESSES + 1);
        (
            Tree::Switch {
                steps,
                t: t.clone(),
                branches,
                default,
            },
            missing,
        )
    }
}

impl<'a> Compiler<'a> {
    /// The test for a column of literals. A row naming no literal there joins
    /// every branch, in source order, so each constant is compared once.
    fn compare(
        &mut self,
        rows: Vec<Row<'a>>,
        occurrences: &[Vec<Step>],
        column: usize,
        boxes: &[Ty],
        ty: &Ty,
    ) -> (Tree, Vec<Vec<String>>) {
        let mut steps = occurrences[column].clone();
        steps.extend(boxes.iter().cloned().map(Step::Unbox));
        let mut outer = occurrences.to_vec();
        outer.remove(column);

        let mut index: HashMap<&'a Constant, usize> = HashMap::new();
        let mut groups: Vec<(&'a Constant, Vec<Row<'a>>)> = Vec::new();
        let mut rest: Vec<Row<'a>> = Vec::new();
        for row in rows {
            let pattern = row.patterns[column];
            let mut patterns = row.patterns;
            patterns.remove(column);
            let row = Row {
                patterns,
                index: row.index,
            };
            if let Checked::Literal { value, .. } = pattern {
                let group = *index.entry(value).or_insert_with(|| {
                    // Rows seen so far that match any value come first.
                    groups.push((value, rest.iter().map(Row::duplicate).collect()));
                    groups.len() - 1
                });
                groups[group].1.push(row);
            } else {
                for (_, group) in &mut groups {
                    group.push(row.duplicate());
                }
                rest.push(row);
            }
        }

        let complete = *ty == Ty::Bool && groups.len() == 2;
        let mut missing = Vec::new();
        let mut branches = Vec::with_capacity(groups.len());
        let named: Vec<&Constant> = groups.iter().map(|(value, _)| *value).collect();
        for (value, group) in groups {
            let (tree, witnesses) = self.compile(group, &outer);
            for mut witness in witnesses {
                witness.insert(column, value.text());
                missing.push(witness);
            }
            branches.push((value.clone(), tree));
        }
        let default = if complete {
            None
        } else {
            // Integers and strings have more values than cases can name, so
            // the values no case names are spelled `_`.
            let unnamed = match (ty, named.as_slice()) {
                (Ty::Bool, [Constant::Bool(named)]) => Constant::Bool(!named).text(),
                _ => "_".to_string(),
            };
            let (tree, witnesses) = self.compile(rest, &outer);
            for mut witness in witnesses {
                witness.insert(column, unnamed.clone());
                missing.push(witness);
            }
            Some(Box::new(tree))
        };
        missing.truncate(MAX_WITNESSES + 1);
        (
            Tree::Compare {
                steps,
                ty: ty.clone(),
                branches,
                default,
            },
            missing,
        )
    }
}

/// A variant as a pattern would spell it.
fn variant_text(t: &Nominal<EnumType>, tag: usize, payload: &[String]) -> String {
    let name = &t.get().variants[tag].name;
    let owner = t.display_name().split('[').next().unwrap_or_default();
    let head = if t.propagatable() {
        name.clone()
    } else {
        format!("{owner}.{name}")
    };
    if payload.is_empty() {
        head
    } else {
        format!("{head}({})", payload.join(", "))
    }
}

/// How the matched value is held for the whole statement.
pub(super) struct Subject {
    pub(super) source: u8,
    /// The scrutinee's loan and mutability when it is a reference.
    pub(super) borrowed: Option<(usize, bool)>,
}

impl Lower<'_> {
    /// `root` is true for the case's own pattern, which matches the whole value.
    fn check_pattern(
        &self,
        pattern: &Pat,
        ty: &Ty,
        root: bool,
        bound: &mut Vec<(String, Ty)>,
    ) -> Result<Checked> {
        let mut boxes = Vec::new();
        let mut content = ty;
        while let Ty::Box(inner) = content {
            boxes.push((**inner).clone());
            content = inner;
        }
        match &pattern.kind {
            PatKind::Member(name, member) => {
                let member = member.resolve(self.aliases)?.unwrap_or(Ty::Unit);
                if matches!(&member, Ty::Enum(t) if t.anonymous()) {
                    return Err(pattern
                        .at
                        .error("a member pattern requires one whole member type, not a sum"));
                }
                if bound.iter().any(|(other, _)| other == name) {
                    return Err(pattern.at.error("duplicate payload binding"));
                }
                if member == *ty {
                    bound.push((name.clone(), member));
                    return Ok(Checked::Binding(name.clone()));
                }
                let Ty::Enum(t) = content else {
                    return Err(pattern
                        .at
                        .error(format!("member type {member} is absent from {ty}")));
                };
                let tag = t
                    .anonymous()
                    .then(|| t.get().variants.iter().position(|v| v.fields[0] == member))
                    .flatten()
                    .ok_or_else(|| {
                        pattern
                            .at
                            .error(format!("member type {member} is absent from {ty}"))
                    })?;
                bound.push((name.clone(), member));
                Ok(Checked::Variant {
                    boxes,
                    t: t.clone(),
                    tag,
                    fields: vec![Checked::Binding(name.clone())],
                })
            }
            PatKind::Wildcard => Ok(Checked::Wildcard),
            PatKind::Binding(name) => {
                if let Ty::Enum(t) = content {
                    if t.get().tag(name).is_some() {
                        let owner = t.display_name().split('[').next().unwrap_or_default();
                        let role = if root {
                            "the matched value"
                        } else {
                            "a payload binding"
                        };
                        return Err(pattern.at.error(format!(
                            "`{name}` is a variant of `{owner}`, so it cannot name {role}; write `{owner}.{name}` to match the variant"
                        )));
                    }
                }
                if bound.iter().any(|(other, _)| other == name) {
                    return Err(pattern.at.error("duplicate payload binding"));
                }
                bound.push((name.clone(), ty.clone()));
                Ok(Checked::Binding(name.clone()))
            }
            PatKind::Variant(qualifier, name, payload) => {
                let t = match content {
                    Ty::Enum(t) if !t.tuple() => t,
                    _ if root => {
                        return Err(pattern.at.error(format!(
                            "a variant pattern cannot match a value of type {ty}"
                        )))
                    }
                    _ => {
                        return Err(pattern.at.error(format!(
                            "a variant pattern requires an enum payload, but this payload has type {ty}; bind it with a name or skip it with `_`"
                        )))
                    }
                };
                self.pattern_owner(qualifier.as_deref(), name, t, &pattern.at)?;
                let definition = t.get();
                let tag = definition
                    .tag(name)
                    .ok_or_else(|| pattern.at.error(format!("unknown variant `{name}`")))?;
                let fields = &definition.variants[tag].fields;
                if payload.as_ref().map_or(0, Vec::len) != fields.len()
                    || (fields.is_empty() && payload.is_some())
                {
                    return Err(pattern.at.error(format!("case `{name}` requires {} payload bindings (no parentheses for nullary variants)", fields.len())));
                }
                let fields = payload
                    .iter()
                    .flatten()
                    .zip(fields)
                    .map(|(pattern, ty)| self.check_pattern(pattern, ty, false, bound))
                    .collect::<Result<_>>()?;
                Ok(Checked::Variant {
                    boxes,
                    t: t.clone(),
                    tag,
                    fields,
                })
            }
            PatKind::Literal(literal) => {
                let value = match (literal, content) {
                    (Literal::Integer(text, negative), ty) if ty.is_int() => {
                        Constant::Int(integer_constant(text, *negative, ty, &pattern.at)?)
                    }
                    (Literal::Text(text), Ty::Str) => Constant::Str(text.clone()),
                    (Literal::Bool(value), Ty::Bool) => Constant::Bool(*value),
                    _ => {
                        let kind = match literal {
                            Literal::Integer(..) => "an integer",
                            Literal::Text(_) => "a string",
                            Literal::Bool(_) => "a boolean",
                        };
                        return Err(pattern
                            .at
                            .error(format!("{kind} pattern cannot match a value of type {ty}")));
                    }
                };
                Ok(Checked::Literal {
                    boxes,
                    ty: content.clone(),
                    value,
                })
            }
            PatKind::Or(alternatives) => {
                let mut shared: Option<Vec<(String, Ty)>> = None;
                let mut checked = Vec::new();
                for alternative in alternatives {
                    let mut names = Vec::new();
                    checked.push((
                        alternative.at.clone(),
                        self.check_pattern(alternative, ty, root, &mut names)?,
                    ));
                    let Some(expected) = &shared else {
                        shared = Some(names);
                        continue;
                    };
                    for (name, ty) in expected {
                        match names.iter().find(|(other, _)| other == name) {
                            None => return Err(alternative.at.error(format!(
                                "every `|` alternative must bind the same names, but this one does not bind `{name}`"
                            ))),
                            Some((_, other)) if other != ty => return Err(alternative.at.error(format!(
                                "`{name}` must have one type in every `|` alternative, but it is {ty} in the first and {other} here"
                            ))),
                            Some(_) => {}
                        }
                    }
                    if let Some((name, _)) = names
                        .iter()
                        .find(|(name, _)| !expected.iter().any(|(other, _)| other == name))
                    {
                        return Err(alternative.at.error(format!(
                            "every `|` alternative must bind the same names, but only this one binds `{name}`"
                        )));
                    }
                }
                for (name, ty) in shared.unwrap_or_default() {
                    if bound.iter().any(|(other, _)| other == &name) {
                        return Err(pattern.at.error("duplicate payload binding"));
                    }
                    bound.push((name, ty));
                }
                Ok(Checked::Or(checked))
            }
        }
    }

    /// A qualifier names the enum and nothing else: the matched value supplies
    /// the type arguments, and the prelude variants are never qualified.
    fn pattern_owner(
        &self,
        qualifier: Option<&TypeRef>,
        variant: &str,
        t: &Nominal<EnumType>,
        at: &Token,
    ) -> Result<()> {
        let matched = Ty::Enum(t.clone());
        let Some(owner) = qualifier else {
            if !enums::prelude_owner(variant, &matched) {
                return Err(at.error(format!(
                    "`{variant}` matches only {} values; qualify user-defined variants",
                    enums::prelude_family(variant)
                )));
            }
            return Ok(());
        };
        if let Some(concrete) = &owner.concrete {
            return self.same(Some(concrete.clone()), Some(matched), &owner.at);
        }
        let name = owner.name.as_deref().unwrap_or_default();
        if matches!(name, "Option" | "Result") {
            return Err(owner.at.error(if enums::prelude_variant(variant) {
                format!("`{variant}` takes no qualifier in a pattern; write `{variant}`")
            } else {
                format!("unknown variant `{name}.{variant}`")
            }));
        }
        let declared = self.aliases.data.contains(name);
        if declared || LIBRARY_GENERICS.contains(&name) {
            let instance = if declared {
                t.name == name
                    || self
                        .aliases
                        .data
                        .arguments(&matched)
                        .is_some_and(|(declaration, _)| declaration == name)
            } else {
                t.name.starts_with(&format!("{name}["))
            };
            if instance {
                return Ok(());
            }
            return Err(owner.at.error(format!("expected {matched}, got {name}")));
        }
        self.same(owner.resolve(self.aliases)?, Some(matched), &owner.at)
    }

    pub(super) fn match_patterns(
        &mut self,
        value: &Expr,
        cases: &[enums::Case],
        matched: &Ty,
        subject: Subject,
        tail: &Tail,
        ops: &mut Vec<Op>,
    ) -> Result<BlockResult> {
        let mut checked = Vec::with_capacity(cases.len());
        for case in cases {
            let mut bound = Vec::new();
            let pattern = self.check_pattern(&case.pattern, matched, true, &mut bound)?;
            checked.push((pattern, bound));
        }
        // One row per `|` combination; `arms[row]` is its case.
        let mut rows = Vec::new();
        let mut chosen = Vec::new();
        let mut arms = Vec::new();
        let mut spans = Vec::with_capacity(cases.len());
        let mut tokens = Vec::new();
        let mut written = Vec::with_capacity(cases.len());
        for (arm, ((pattern, _), case)) in checked.iter().zip(cases).enumerate() {
            let first = tokens.len();
            let alternatives = pattern.alternatives(&mut tokens).ok_or_else(|| {
                case.at.error(format!(
                    "pattern expands to more than {MAX_ALTERNATIVES} `|` combinations"
                ))
            })?;
            written.push(first..tokens.len());
            spans.push(rows.len()..rows.len() + alternatives.len());
            arms.extend(std::iter::repeat_n(arm, alternatives.len()));
            for (pattern, choice) in alternatives {
                rows.push(pattern);
                chosen.push(choice);
            }
        }
        let wildcard = Checked::Wildcard;
        let mut compiler = Compiler {
            wildcard: &wildcard,
            reached: vec![false; rows.len()],
            tests: 0,
        };
        let (tree, missing) = compiler.compile(
            rows.iter()
                .enumerate()
                .map(|(index, pattern)| Row {
                    patterns: vec![pattern],
                    index,
                })
                .collect(),
            &[Vec::new()],
        );
        if compiler.tests > MAX_TESTS {
            return Err(value.at.error(format!(
                "match needs more than {MAX_TESTS} tests; split it into nested matches"
            )));
        }
        let reached = compiler.reached;
        // A combination of alternatives may be covered while each alternative
        // still matches something, so only an alternative with no reached
        // combination is an error.
        let mut useful = vec![false; tokens.len()];
        for (row, choice) in chosen.iter().enumerate() {
            if reached[row] {
                for &id in choice {
                    useful[id] = true;
                }
            }
        }
        for (arm, case) in cases.iter().enumerate() {
            let span = spans[arm].clone();
            if span.clone().any(|row| reached[row]) {
                if let Some(id) = written[arm].clone().find(|&id| !useful[id]) {
                    return Err(tokens[id].error(
                        "unreachable `|` alternative: earlier patterns already match every value it names",
                    ));
                }
                continue;
            }
            let duplicate = span.len() == 1
                && rows[span.start].plain_variant().is_some_and(|tag| {
                    rows[..span.start]
                        .iter()
                        .any(|earlier| earlier.plain_variant() == Some(tag))
                });
            return Err(case.at.error(if duplicate {
                "duplicate variant case"
            } else {
                "unreachable case"
            }));
        }
        if !missing.is_empty() {
            let more = if missing.len() > MAX_WITNESSES {
                ", and more"
            } else {
                ""
            };
            let missing: Vec<String> = missing
                .into_iter()
                .take(MAX_WITNESSES)
                .map(|mut witness| witness.remove(0))
                .collect();
            if missing == ["_"] {
                return Err(value.at.error(format!(
                    "non-exhaustive match; cases cannot name every {matched} value, so end with `case _:`"
                )));
            }
            let missing = missing.join(", ") + more;
            return Err(value
                .at
                .error(format!("non-exhaustive match; missing {missing}")));
        }

        let mut leaves = vec![0; cases.len()];
        tree.leaves(&mut leaves, &arms);
        // A body reached from one place is lowered there. Otherwise the tests
        // select a case number first, so each body is still lowered once.
        let indexed = leaves.iter().any(|&count| count > 1);
        // Which alternative matched decides how its bindings are filled.
        let alternative = if checked
            .iter()
            .zip(&spans)
            .any(|((_, bound), span)| span.len() > 1 && !bound.is_empty())
        {
            let slot = self.slot(Ty::I64, &value.at)?;
            // Every path stores before reading, but ownership analysis joins
            // paths that select other cases, so the slot starts initialized.
            ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(slot)]);
            Some(slot)
        } else {
            None
        };

        let saved = self.names.clone();
        let mut bodies = Vec::with_capacity(cases.len());
        let mut joined: Option<Type> = None;
        for (arm, case) in cases.iter().enumerate() {
            self.names = saved.clone();
            let case_start = self.locals.len();
            let mut body = Vec::new();
            let bound = &checked[arm].1;
            let mut slots = HashMap::new();
            for (name, ty) in bound {
                let ty = match subject.borrowed {
                    Some((_, mutable)) => Ty::Ref(Arc::new(ty.clone()), mutable),
                    None => ty.clone(),
                };
                let slot = self.slot(ty.clone(), &case.at)?;
                self.bind(
                    name.clone(),
                    Local {
                        slot,
                        ty,
                        mutable: false,
                    },
                );
                slots.insert(name.as_str(), slot);
            }
            let alternatives = &rows[spans[arm].clone()];
            self.fill_bindings(
                &subject,
                alternatives,
                bound,
                &slots,
                alternative,
                &mut body,
            );
            if let BlockResult::Continues(ty) =
                self.returning_block(&case.body, &case.at, &mut body, tail)?
            {
                if !matches!(tail, Tail::Function) {
                    self.cleanup(case_start, &mut body);
                }
                if let Some(expected) = &joined {
                    self.same(ty.clone(), expected.clone(), &case.at)?;
                } else {
                    joined = Some(ty);
                }
            }
            bodies.push(Some(body));
        }
        self.names = saved;

        // A case with alternatives and bindings records which one matched.
        let choose = |row: usize| {
            let span = &spans[arms[row]];
            alternative
                .filter(|_| span.len() > 1 && !checked[arms[row]].1.is_empty())
                .into_iter()
                .flat_map(move |slot| {
                    [
                        Op::PushInt(Value::I64((row - span.start) as i64)),
                        Op::StoreLocal(slot),
                    ]
                })
        };
        if indexed {
            let mut select = |row: usize| {
                let mut leaf: Vec<Op> = choose(row).collect();
                leaf.push(Op::PushInt(Value::I64(arms[row] as i64)));
                leaf
            };
            self.emit_tree(&tree, &subject, &mut select, ops);
            ops.push(Op::Match(numbered(
                bodies.into_iter().map(Option::unwrap).collect(),
            )));
        } else if let Tree::Leaf(row) = &tree {
            // A lone `case _:` keeps the dispatch that every other match has,
            // so the statement lowers the same way with or without further
            // cases.
            match matched {
                Ty::Enum(t) => self.peek(&subject, &[], t, ops),
                scalar => self.read(&subject, &[], scalar, ops),
            }
            ops.push(Op::Match(
                vec![MatchArm {
                    pattern: Pattern::Wildcard,
                    body: bodies[arms[*row]].take().unwrap().into(),
                }]
                .into(),
            ));
        } else {
            let mut inline = |row: usize| {
                let mut leaf: Vec<Op> = choose(row).collect();
                leaf.extend(bodies[arms[row]].take().unwrap());
                leaf
            };
            self.emit_tree(&tree, &subject, &mut inline, ops);
        }
        if !matches!(tail, Tail::Function) && joined.is_some() {
            ops.push(Op::DropLocal(subject.source));
        }
        Ok(joined.map_or(BlockResult::Exits, BlockResult::Continues))
    }

    /// Push the tag at a position, leaving the matched value in place.
    fn peek(&self, subject: &Subject, steps: &[Step], t: &Nominal<EnumType>, ops: &mut Vec<Op>) {
        if let Some((loan, mutable)) = subject.borrowed {
            ops.push(Op::LoadLocal(subject.source));
            ops.push(Op::Access(self.loans[loan].root, false, Some(loan)));
            project(steps, mutable, ops);
            ops.push(Op::Enum(EnumOp::TagRef(t.clone(), mutable)));
            ops.push(Op::UseLoan(loan));
        } else if steps.is_empty() {
            ops.push(Op::LoadLocal(subject.source));
            ops.push(Op::Enum(EnumOp::Tag(t.clone())));
        } else {
            // The reference is consumed by the tag read, so nothing can observe
            // the hidden slot through it and no loan is recorded.
            ops.push(Op::BorrowLocal(subject.source, false));
            project(steps, false, ops);
            ops.push(Op::Enum(EnumOp::TagRef(t.clone(), false)));
        }
    }

    /// Push a copy of the integer, string, or boolean at a position, leaving
    /// the matched value in place.
    fn read(&self, subject: &Subject, steps: &[Step], ty: &Ty, ops: &mut Vec<Op>) {
        if let Some((loan, mutable)) = subject.borrowed {
            ops.push(Op::LoadLocal(subject.source));
            ops.push(Op::Access(self.loans[loan].root, false, Some(loan)));
            project(steps, mutable, ops);
            ops.push(Op::ReadRef(ty.clone()));
            ops.push(Op::UseLoan(loan));
        } else if steps.is_empty() {
            ops.push(Op::LoadLocal(subject.source));
        } else {
            // As in `peek`: the read consumes the reference.
            ops.push(Op::BorrowLocal(subject.source, false));
            project(steps, false, ops);
            ops.push(Op::ReadRef(ty.clone()));
        }
    }

    fn emit_tree(
        &mut self,
        tree: &Tree,
        subject: &Subject,
        leaf: &mut dyn FnMut(usize) -> Vec<Op>,
        ops: &mut Vec<Op>,
    ) {
        match tree {
            Tree::Leaf(row) => ops.extend(leaf(*row)),
            Tree::Fail => unreachable!("a non-exhaustive match is rejected before lowering"),
            Tree::Switch {
                steps,
                t,
                branches,
                default,
            } => {
                self.peek(subject, steps, t, ops);
                let mut arms = Vec::with_capacity(branches.len() + 1);
                for (tag, tree) in branches {
                    let mut body = Vec::new();
                    self.emit_tree(tree, subject, leaf, &mut body);
                    arms.push(MatchArm {
                        pattern: Pattern::Int {
                            value: Value::I64(*tag as i64),
                            explicit_ty: true,
                        },
                        body: body.into(),
                    });
                }
                let mut body = Vec::new();
                match default {
                    Some(tree) => self.emit_tree(tree, subject, leaf, &mut body),
                    None => body.push(Op::Unreachable),
                }
                arms.push(MatchArm {
                    pattern: Pattern::Wildcard,
                    body: body.into(),
                });
                ops.push(Op::Match(arms.into()));
            }
            Tree::Compare {
                steps,
                ty,
                branches,
                default,
            } => {
                self.read(subject, steps, ty, ops);
                let mut arms = Vec::with_capacity(branches.len() + 1);
                for (value, tree) in branches {
                    let mut body = Vec::new();
                    self.emit_tree(tree, subject, leaf, &mut body);
                    arms.push(MatchArm {
                        pattern: match value {
                            Constant::Int(value) => Pattern::Int {
                                value: *value,
                                explicit_ty: true,
                            },
                            Constant::Str(text) => Pattern::Str(self.heap.add_str(text.clone())),
                            Constant::Bool(value) => Pattern::Bool(*value),
                        },
                        body: body.into(),
                    });
                }
                if let Some(tree) = default {
                    let mut body = Vec::new();
                    self.emit_tree(tree, subject, leaf, &mut body);
                    arms.push(MatchArm {
                        pattern: Pattern::Wildcard,
                        body: body.into(),
                    });
                }
                ops.push(Op::Match(arms.into()));
            }
        }
    }

    /// Give the case's bindings their values from whichever alternative matched.
    fn fill_bindings(
        &mut self,
        subject: &Subject,
        alternatives: &[Checked],
        bound: &[(String, Ty)],
        slots: &HashMap<&str, u8>,
        alternative: Option<u8>,
        body: &mut Vec<Op>,
    ) {
        let single = alternatives.len() == 1;
        let mut fills = Vec::with_capacity(alternatives.len());
        if let Some((parent, mutable)) = subject.borrowed {
            let paths: Vec<_> = alternatives
                .iter()
                .map(|pattern| {
                    let mut paths = Vec::new();
                    pattern.paths(&mut Vec::new(), &mut paths);
                    paths
                })
                .collect();
            let path = |row: usize, name: &str| -> &[Step] {
                &paths[row]
                    .iter()
                    .find(|(other, _)| *other == name)
                    .expect("every alternative binds every name")
                    .1
            };
            let load = |steps: &[Step], slot: u8, ops: &mut Vec<Op>| {
                ops.push(Op::LoadLocal(subject.source));
                project(steps, mutable, ops);
                ops.push(Op::StoreLocal(slot));
            };
            for (name, _) in bound {
                let slot = slots[name.as_str()];
                let loan = self.new_loan(self.loans[parent].root, mutable, Some(parent), body);
                // A payload loan protects its enclosing variant from
                // replacement, while sibling fields remain disjoint. With
                // alternatives it covers the part of the path they share.
                if self.loans[loan].precise {
                    let fields = |row: usize| {
                        path(row, name).iter().filter_map(|step| match step {
                            Step::Field(_, _, field) => Some(*field),
                            Step::Unbox(_) => None,
                        })
                    };
                    let mut shared: Vec<usize> = fields(0).collect();
                    for row in 1..alternatives.len() {
                        let common = shared
                            .iter()
                            .zip(fields(row))
                            .take_while(|(a, b)| **a == *b)
                            .count();
                        shared.truncate(common);
                    }
                    self.loans[loan].fields.extend(shared);
                    if let Some(Op::Loan(fact)) = body.last_mut() {
                        *fact = self.loans[loan].clone();
                    }
                }
                self.reference_locals.insert(slot, loan);
                if single {
                    load(path(0, name), slot, body);
                }
            }
            if !single && !bound.is_empty() {
                for row in 0..alternatives.len() {
                    let mut fill = Vec::new();
                    for (name, _) in bound {
                        load(path(row, name), slots[name.as_str()], &mut fill);
                    }
                    fills.push(fill);
                }
            }
        } else if single {
            owned_root(&alternatives[0], subject.source, slots, true, body);
        } else if !bound.is_empty() {
            for pattern in alternatives {
                let mut fill = Vec::new();
                owned_root(pattern, subject.source, slots, false, &mut fill);
                fills.push(fill);
            }
        }
        if !fills.is_empty() {
            body.push(Op::LoadLocal(
                alternative.expect("a case with alternatives and bindings has a selector"),
            ));
            body.push(Op::Match(numbered(fills)));
        }
    }
}

const LIBRARY_GENERICS: [&str; 6] = [
    "SpawnError",
    "SendError",
    "SendTimeoutError",
    "Selected",
    "SubmitError",
    "ParallelError",
];

/// Arms selected by position; the last takes the wildcard an integer match needs.
fn numbered(bodies: Vec<Vec<Op>>) -> Arc<[MatchArm]> {
    let last = bodies.len() - 1;
    bodies
        .into_iter()
        .enumerate()
        .map(|(index, body)| MatchArm {
            pattern: if index == last {
                Pattern::Wildcard
            } else {
                Pattern::Int {
                    value: Value::I64(index as i64),
                    explicit_ty: true,
                }
            },
            body: body.into(),
        })
        .collect()
}

/// Move the matched value out of its slot into the bindings. A consuming match
/// moves every field of a multi-field variant out at once, so a lone pattern
/// naming one splits it even when it binds nothing; unbound positions are
/// dropped immediately.
fn owned_root(
    pattern: &Checked,
    source: u8,
    slots: &HashMap<&str, u8>,
    single: bool,
    ops: &mut Vec<Op>,
) {
    // `_ as name` takes the whole value.
    if let Checked::Binding(name) = pattern {
        ops.extend([Op::MoveLocal(source), Op::StoreLocal(slots[name.as_str()])]);
        return;
    }
    let Checked::Variant { fields, .. } = pattern else {
        return;
    };
    if pattern.binds() || (single && fields.len() > 1) {
        ops.push(Op::MoveLocal(source));
        owned_variant(pattern, slots, ops);
    }
}

/// Consume the value on top of the operand stack.
fn owned(pattern: &Checked, slots: &HashMap<&str, u8>, ops: &mut Vec<Op>) {
    match pattern {
        Checked::Binding(name) => ops.push(Op::StoreLocal(slots[name.as_str()])),
        Checked::Variant { .. } if pattern.binds() => owned_variant(pattern, slots, ops),
        _ => ops.push(Op::Drop),
    }
}

fn owned_variant(pattern: &Checked, slots: &HashMap<&str, u8>, ops: &mut Vec<Op>) {
    let Checked::Variant {
        boxes,
        t,
        tag,
        fields,
    } = pattern
    else {
        unreachable!()
    };
    for content in boxes {
        ops.push(Op::Box(BoxOp::Take(content.clone())));
    }
    if let [field] = fields.as_slice() {
        let affine = t.get().variants[*tag].fields[0].affine();
        ops.push(Op::Enum(if affine {
            EnumOp::Take(t.clone(), *tag, 0)
        } else {
            EnumOp::Field(t.clone(), *tag, 0)
        }));
        owned(field, slots, ops);
    } else {
        ops.push(Op::Split(t.clone(), *tag));
        for field in fields.iter().rev() {
            owned(field, slots, ops);
        }
    }
}
