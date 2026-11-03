//! Quantity operators check dimensions, then use the storage type's ordinary
//! lowering. The builtin copy class has one private field: no runtime unit tag.
use super::*;
use crate::dimension::Dimension;

impl TypeAliases {
    pub(super) fn quantity(&self, ty: &Ty) -> Option<(Ty, Arc<Dimension>, Token)> {
        let ty = match ty {
            Ty::Ref(inner, _) => inner,
            other => other,
        };
        let (declaration, args) = self.data.origin_class(ty)?;
        if declaration.name != "Quantity" {
            return None;
        }
        let [storage, Ty::Dimension(dimension)] = args.as_slice() else {
            return None;
        };
        Some((storage.clone(), dimension.clone(), declaration.at.clone()))
    }
}

/// Dimension rules shared by ordinary lowering and module-constant evaluation.
pub(super) fn binary_dimension(
    op: &str,
    a: Option<&Dimension>,
    b: Option<&Dimension>,
    at: &Token,
) -> Result<Dimension> {
    let dimension = match (a, b) {
        (Some(a), Some(b)) => match op {
            "*" => a.product(b, false),
            "/" | "//" => a.product(b, true),
            "+" | "-" | "%" | "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                if a != b {
                    return Err(at.error(format!("quantity `{op}` requires equal dimensions, not `{a}` and `{b}`")));
                }
                Ok(a.clone())
            }
            _ => return Err(at.error(format!("quantities do not support `{op}`"))),
        },
        (Some(d), None) | (None, Some(d)) => {
            if !matches!(op, "*" | "/") {
                return Err(at.error("scalars only scale quantities with `*` or `/`; addition, remainder and comparisons require quantities"));
            }
            if a.is_none() && op == "/" {
                Dimension::default().product(d, true)
            } else {
                Ok(d.clone())
            }
        }
        _ => unreachable!(),
    }.map_err(|error| at.error(error.to_string()))?;
    Ok(dimension)
}

fn field(value: &Expr, declared: &Token) -> Expr {
    Expr {
        at: declared.clone(),
        kind: Expression::Member(Box::new(value.clone()), "value".into(), Pos::default()),
    }
}

impl Lower<'_> {
    fn quantity_wrap(
        &mut self,
        result: Type,
        dimension: Dimension,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let storage = result
            .as_ref()
            .ok_or_else(|| at.error("expected a numeric result"))?;
        let dimension = self.aliases.intern_dimension(dimension);
        let quantity = TypeRef {
            name: Some("Quantity".into()),
            args: vec![
                generics::type_ref(storage, at),
                generics::type_ref(&dimension, at),
            ],
            at: at.clone(),
            concrete: None,
            metadata: None,
        }
        .resolve(self.aliases)?
        .unwrap();
        self.sync_data()?;
        let (_, _, declared) = self.aliases.quantity(&quantity).unwrap();
        let Ty::Class(class) = quantity else {
            unreachable!()
        };
        self.rewrap(&class, &declared, result, ops)
    }

    pub(super) fn quantity_binary(
        &mut self,
        op: &str,
        left: &Expr,
        right: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Type>> {
        let a = self.operator_hint(left);
        let b = self.operator_hint(right);
        let qa = a.as_ref().and_then(|ty| self.aliases.quantity(ty));
        let qb = b.as_ref().and_then(|ty| self.aliases.quantity(ty));
        if qa.is_none() && qb.is_none() {
            return Ok(None);
        }
        if a.is_none() || b.is_none() {
            return self.retry_binary(op, left, right, at, ops).map(Some);
        }
        let dimension = binary_dimension(
            op,
            qa.as_ref().map(|(_, d, _)| d.as_ref()),
            qb.as_ref().map(|(_, d, _)| d.as_ref()),
            at,
        )?;
        for scalar in [
            if qa.is_none() { a.as_ref() } else { None },
            if qb.is_none() { b.as_ref() } else { None },
        ]
        .into_iter()
        .flatten()
        {
            let scalar = match scalar {
                Ty::Ref(t, _) => t,
                t => t,
            };
            if !scalar.is_numeric() && scalar.integer_domain().is_none() {
                return Err(at.error("a quantity scalar must be numeric"));
            }
        }
        let inner = Expr {
            at: at.clone(),
            kind: Expression::Binary(
                op.into(),
                Box::new(
                    qa.as_ref()
                        .map_or_else(|| left.clone(), |(_, _, at)| field(left, at)),
                ),
                Box::new(
                    qb.as_ref()
                        .map_or_else(|| right.clone(), |(_, _, at)| field(right, at)),
                ),
            ),
        };
        let result = self.expr(&inner, ops)?;
        if is_comparison(op) {
            return Ok(Some(result));
        }
        self.quantity_wrap(result, dimension, at, ops).map(Some)
    }

    pub(super) fn quantity_unary(
        &mut self,
        op: &str,
        value: &Expr,
        ty: &Ty,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Type>> {
        let Some((_, dimension, declared)) = self.aliases.quantity(ty) else {
            return Ok(None);
        };
        if !matches!(op, "+" | "-") {
            return Err(value.at.error(format!("quantities do not support `{op}`")));
        }
        let inner = Expr {
            at: value.at.clone(),
            kind: Expression::Unary(op.into(), Box::new(field(value, &declared))),
        };
        let result = self.expr(&inner, ops)?;
        self.quantity_wrap(result, (*dimension).clone(), &value.at, ops)
            .map(Some)
    }

    pub(super) fn quantity_static(
        &mut self,
        ty: &Ty,
        name: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Type>> {
        let Some((storage, dimension, declared)) = self.aliases.quantity(ty) else {
            return Ok(None);
        };
        let Ty::Class(class) = ty else { unreachable!() };
        if name == "try_from" {
            let Some((domain, element)) = storage.integer_domain() else {
                return Err(at.error("Quantity.try_from requires integer-domain storage"));
            };
            let result = self.domain_try_from(domain, Some(element), args, at, ops)?;
            self.sync_data()?;
            return self.rewrap(class, &declared, result, ops).map(Some);
        }
        if name == "from" {
            if !storage.is_float() {
                return Err(at.error("Quantity.from requires float storage"));
            }
            let [value] = args else {
                return Err(at.error("Quantity.from takes one quantity"));
            };
            let (held, loan) = self.hold_operator_operand(value, ops)?;
            let source = self.operator_hint(&held).unwrap();
            let Some((source_storage, d, source_at)) = self.aliases.quantity(&source) else {
                return Err(at.error("Quantity.from takes a quantity of the same dimension"));
            };
            if dimension != d || !source_storage.is_numeric() {
                return Err(
                    at.error("Quantity.from requires equal dimensions and plain numeric storage")
                );
            }
            let result = self.construct(storage, &[field(&held, &source_at)], at, ops)?;
            self.release_operator_operand(&held, loan, ops);
            self.sync_data()?;
            return self.rewrap(class, &declared, Some(result), ops).map(Some);
        }
        Ok(None)
    }

    // Bind an already evaluated copy value for ordinary expression lowering.
    fn quantity_held(&mut self, ty: Ty, at: &Token, ops: &mut Vec<Op>) -> Result<Expr> {
        let slot = self.slot(ty.clone(), at)?;
        ops.push(Op::StoreLocal(slot));
        let name = format!("$quantity{slot}");
        self.bind(
            name.clone(),
            Local {
                slot,
                ty,
                mutable: false,
            },
        );
        Ok(Expr {
            at: at.clone(),
            kind: Expression::Name(name),
        })
    }

    pub(super) fn quantity_augment(
        &mut self,
        target: &Expr,
        op: &str,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<bool> {
        let Some(ty) = self.place_type(target) else {
            return Ok(false);
        };
        let Some((storage, _, _)) = self.aliases.quantity(&ty) else {
            return Ok(false);
        };
        if !matches!(op, "+" | "-" | "*" | "/") {
            return Err(target
                .at
                .error("quantity compound assignment supports only `+=`, `-=`, `*=` and `/=`"));
        }
        if self.dict_index_target(target) {
            return Err(target.at.error(references::dict_index_augment(op)));
        }
        // Save indices once, and keep the read loan through RHS evaluation, as
        // ordinary numeric augmentation does. This also handles inferred calls
        // on the RHS without evaluating them before the destination.
        let mut indices = Vec::new();
        self.assignment_indices(target, &mut indices, ops)?;
        let mut read_indices = Vec::new();
        for index in &indices {
            let key = self.locals[*index as usize - self.parameters].clone();
            let slot = self.slot(key, &target.at)?;
            ops.push(Op::LoadLocal(*index));
            ops.push(Op::StoreLocal(slot));
            read_indices.push(slot);
        }
        let (owned, read) =
            self.borrow_with_indices(target, false, &mut Some(read_indices.iter()), ops)?;
        let Ty::Ref(owned, _) = owned else {
            unreachable!()
        };
        let owned = (*owned).clone();
        ops.push(Op::ReadRef(owned.clone()));
        let left = self.quantity_held(owned.clone(), &target.at, ops)?;
        let expected = if matches!(op, "+" | "-") {
            owned.clone()
        } else {
            storage
        };
        let actual = self.expr_expected(value, Some(expected.clone()), ops)?;
        if actual != Some(expected.clone()) {
            return Err(value.at.error(format!("quantity `+=` and `-=` require equal dimensions and storage; `*=` and `/=` require a scalar `{expected}`, got {}", type_name(actual))));
        }
        let right = self.quantity_held(expected, &value.at, ops)?;
        let expression = Expr {
            at: target.at.clone(),
            kind: Expression::Binary(op.into(), Box::new(left.clone()), Box::new(right.clone())),
        };
        let result = self.expr(&expression, ops)?;
        self.same(result, Some(owned.clone()), &target.at)?;
        self.release_operator_operand(&right, None, ops);
        self.release_operator_operand(&left, None, ops);
        let result = self.slot(owned.clone(), &target.at)?;
        ops.push(Op::StoreLocal(result));
        ops.push(Op::UseLoan(read));
        let (_, write) = self.borrow_with_indices(target, true, &mut Some(indices.iter()), ops)?;
        ops.push(Op::MoveLocal(result));
        ops.push(Op::Swap);
        ops.push(Op::WriteRef(owned));
        ops.push(Op::UseLoan(write));
        Ok(true)
    }
}
