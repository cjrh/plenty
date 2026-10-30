//! Diagnostic-only value provenance. Equal concrete types are not evidence
//! that values came from the same type parameter. Shapes preserve projections
//! (a dictionary's key and value, for example) without changing type identity.
use super::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) enum Origin {
    #[default]
    Concrete,
    Parameter(Ty),
    Form(String, Vec<Origin>),
}

impl Origin {
    pub(super) fn written(ty: &TypeRef) -> Self {
        if ty.parameter {
            return ty
                .concrete
                .as_deref()
                .cloned()
                .map(Self::Parameter)
                .unwrap_or_default();
        }
        if ty.concrete.is_some() {
            return Self::Concrete;
        }
        Self::form(
            ty.name.as_deref().unwrap_or("()"),
            ty.args.iter().map(Self::written).collect(),
        )
    }

    fn form(name: &str, children: Vec<Self>) -> Self {
        if children.iter().all(|child| matches!(child, Self::Concrete)) {
            Self::Concrete
        } else {
            Self::Form(name.into(), children)
        }
    }

    fn field_type(ty: &TypeRef, parameters: &HashMap<&str, Origin>) -> Self {
        if let Some(value) = ty.name.as_deref().and_then(|name| parameters.get(name)) {
            return value.clone();
        }
        Self::form(
            ty.name.as_deref().unwrap_or("()"),
            ty.args
                .iter()
                .map(|t| Self::field_type(t, parameters))
                .collect(),
        )
    }

    fn child(&self, index: usize) -> Self {
        match self {
            Self::Form(_, children) => children.get(index).cloned().unwrap_or_default(),
            // Projecting inside an opaque T does not produce a T value.
            _ => Self::Concrete,
        }
    }

    fn common(self, other: Self) -> Self {
        if self == other { self } else { Self::Concrete }
    }

    fn parameter(&self) -> Option<Ty> {
        match self {
            Self::Parameter(ty) => Some(ty.clone()),
            _ => None,
        }
    }
}

impl Lower<'_> {
    pub(super) fn record_value_origin(&mut self, slot: u8, origin: Origin) {
        if self.generics.requirement(self.symbol).is_none() {
            return;
        }
        // A mutable slot may have different values across branches/iterations.
        // Retain only common provenance, rather than attributing one path's
        // concrete value to a parameter from another path.
        self.value_origins
            .entry(slot)
            .and_modify(|old| *old = old.clone().common(origin.clone()))
            .or_insert(origin);
    }

    pub(super) fn parameter_values(&self) -> Vec<Option<Ty>> {
        if self.value_origins.is_empty() {
            return Vec::new();
        }
        (0..self.parameters + self.locals.len())
            .map(|slot| {
                self.value_origins
                    .get(&(slot as u8))
                    .and_then(Origin::parameter)
            })
            .collect()
    }

    pub(super) fn self_origin(&self, ty: &Ty) -> Option<Origin> {
        match ty {
            Ty::Ref(inner, mutable) => Some(Origin::form(
                if *mutable { "&mut" } else { "&" },
                vec![self.self_origin(inner)?],
            )),
            _ => {
                let (declaration, actual) = self.aliases.data.origin_class(ty)?;
                Some(Origin::form(
                    &declaration.name,
                    actual.into_iter().map(Origin::Parameter).collect(),
                ))
            }
        }
    }

    fn receiver_origin(&self, value: &Expr) -> (Type, Origin) {
        // place_type already removes a named reference's outer borrow. Keep
        // that layer here so the type and provenance are peeled together.
        let mut ty = match &ungroup(value).kind {
            Expression::Name(name) => self.names.get(name).map(|local| local.ty.clone()),
            _ => self.place_type(value),
        };
        let mut origin = self.value_origin(value);
        while let Some(Ty::Ref(inner, _) | Ty::Box(inner) | Ty::Shared(inner)) = ty {
            origin = origin.child(0);
            ty = Some((*inner).clone());
        }
        (ty, origin)
    }

    pub(super) fn value_origin(&self, value: &Expr) -> Origin {
        if self.generics.requirement(self.symbol).is_none() {
            return Origin::Concrete;
        }
        match &value.kind {
            Expression::Name(name) => self
                .names
                .get(name)
                .and_then(|local| self.value_origins.get(&local.slot))
                .cloned()
                .unwrap_or_default(),
            Expression::Group(value) | Expression::NamedArgument(_, value) => {
                self.value_origin(value)
            }
            Expression::Unary(op, value) if matches!(op.as_str(), "&" | "&mut") => {
                Origin::form(op, vec![self.value_origin(value)])
            }
            Expression::Unary(op, value) if op == "*" => self.value_origin(value).child(0),
            Expression::Unary(op, value) if matches!(op.as_str(), "-" | "+" | "~") => {
                self.value_origin(value)
            }
            Expression::Try(value) => self.value_origin(value).child(0),
            Expression::Tuple(values) => Origin::form(
                "tuple",
                values.iter().map(|v| self.value_origin(v)).collect(),
            ),
            Expression::Conditional { yes, no, .. } => {
                self.value_origin(yes).common(self.value_origin(no))
            }
            Expression::Binary(op, left, right)
                if !matches!(
                    op.as_str(),
                    "==" | "!=" | "<" | ">" | "<=" | ">=" | "and" | "or"
                ) =>
            {
                self.value_origin(left).common(self.value_origin(right))
            }
            Expression::Member(base, field, _) => {
                let (Some(ty), Origin::Form(name, children)) = self.receiver_origin(base) else {
                    return Origin::Concrete;
                };
                let Some((declaration, _)) = self.aliases.data.origin_class(&ty) else {
                    return Origin::Concrete;
                };
                if name != declaration.name {
                    return Origin::Concrete;
                }
                let Some((_, field)) = declaration.fields.iter().find(|(name, _)| name == field)
                else {
                    return Origin::Concrete;
                };
                let parameters = declaration
                    .type_params
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .zip(children)
                    .collect();
                Origin::field_type(field, &parameters)
            }
            Expression::Index(base, index) => {
                let (ty, origin) = self.receiver_origin(base);
                match ty {
                    Some(Ty::List(_)) => origin.child(0),
                    Some(Ty::Dict(..)) => origin.child(1),
                    Some(Ty::Enum(t)) if t.tuple() => {
                        let len = t.get().variants[0].fields.len();
                        slices::tuple_position(index, len)
                            .map(|i| origin.child(i))
                            .unwrap_or_default()
                    }
                    _ => Origin::Concrete,
                }
            }
            Expression::Collection {
                kind,
                entries,
                clauses,
            } if clauses.is_empty() => {
                let common = |values: Vec<Origin>| {
                    values
                        .into_iter()
                        .reduce(Origin::common)
                        .unwrap_or_default()
                };
                let first = common(
                    entries
                        .iter()
                        .map(|(value, _)| self.value_origin(value))
                        .collect(),
                );
                let children = if kind == "dict" {
                    vec![
                        first,
                        common(
                            entries
                                .iter()
                                .filter_map(|(_, value)| value.as_ref())
                                .map(|value| self.value_origin(value))
                                .collect(),
                        ),
                    ]
                } else {
                    vec![first]
                };
                Origin::form(
                    "Result",
                    vec![Origin::form(kind, children), Origin::Concrete],
                )
            }
            // Unknown calls/transformations conservatively retain the ordinary
            // instance diagnostic; their concrete result type proves no origin.
            _ => Origin::Concrete,
        }
    }

    pub(super) fn value_demand_error(
        &self,
        at: &Token,
        value: &Expr,
        message: impl std::fmt::Display,
        demand: crate::ownership::Demand,
        receiver: bool,
    ) -> Box<dyn Error> {
        let origin = if matches!(demand, crate::ownership::Demand::Operator(..))
            && let Expression::Binary(_, left, right) = &value.kind
        {
            // A comparison produces bool, but the rejected operator applies
            // to its operands. One known parameter operand establishes the
            // demand even if the other came through an untracked binding.
            self.value_origin(left)
                .parameter()
                .or_else(|| self.value_origin(right).parameter())
                .map(Origin::Parameter)
                .unwrap_or_default()
        } else if receiver {
            self.receiver_origin(value).1
        } else {
            self.value_origin(value)
        };
        let demanded = match &demand {
            crate::ownership::Demand::Copy(ty, _)
            | crate::ownership::Demand::Operator(_, ty)
            | crate::ownership::Demand::Method(_, ty) => ty,
        };
        if origin.parameter().as_ref() == Some(demanded) {
            at.demand_error(message, demand)
        } else {
            at.error(message)
        }
    }
}
