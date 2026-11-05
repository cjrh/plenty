//! Quantity constant expressions retain dimensions while reusing scalar
//! evaluation. No initializer may run a user constructor or function.
use super::*;
use crate::dimension::Dimension;

impl Folder<'_> {
    pub(super) fn quantity_type(
        &self,
        storage: &Ty,
        dimension: Dimension,
        at: &Token,
    ) -> Result<Ty> {
        let dimension = self.aliases.intern_dimension(dimension);
        TypeRef {
            concrete: None,
            metadata: None,
            at: at.clone(),
            name: Some("Quantity".into()),
            args: vec![
                generics::type_ref(storage, at),
                generics::type_ref(&dimension, at),
            ],
        }
        .resolve(self.aliases)?
        .ok_or_else(|| at.error("a quantity needs numeric storage"))
    }

    pub(super) fn constructor<'e>(&self, e: &'e Expr) -> Option<Result<(Ty, &'e [Expr])>> {
        let (ty, args) = match &e.kind {
            Expression::Constructor(ty, args) => ((**ty).clone(), args),
            Expression::GenericCall(name, types, args) => (
                TypeRef {
                    concrete: None,
                    metadata: None,
                    at: e.at.clone(),
                    name: Some(name.clone()),
                    args: types.clone(),
                },
                args,
            ),
            _ => return None,
        };
        Some((|| {
            let ty = ty.resolve(self.aliases)?.ok_or_else(|| not_constant(e))?;
            if self.aliases.quantity(&ty).is_none() {
                return Err(not_constant(e));
            }
            Ok((ty, args.as_slice()))
        })())
    }

    fn storage_value(&mut self, e: &Expr, storage: &Ty) -> Result<Value> {
        if let Some((domain, inner)) = storage.integer_domain() {
            if super::super::nonzero::literal(e) {
                let value = self.typed(e, &inner)?;
                let Value::Int(n, _) = value else {
                    unreachable!()
                };
                if !admits(domain, n) {
                    return Err(e.at.error(format!("literal {n} is not a `{storage}`")));
                }
                return Ok(Value::Int(n, storage.clone()));
            }
            if let Some(f) = self.domain_read(e)
                && declared(f, self.aliases)? == (inner, Some(domain))
            {
                let Value::Int(n, _) = self.constant(f)? else {
                    unreachable!()
                };
                return Ok(Value::Int(n, storage.clone()));
            }
        }
        self.typed(e, storage)
    }

    pub(super) fn quantity_eval(
        &mut self,
        e: &Expr,
        expected: Option<&Ty>,
    ) -> Option<Result<Value>> {
        if let Some(constructor) = self.constructor(e) {
            return Some((|| {
                let (ty, args) = constructor?;
                let [value] = args else {
                    return Err(e.at.error("Quantity construction takes one coherent value"));
                };
                let (storage, _, _) = self.aliases.quantity(&ty).unwrap();
                Ok(Value::Quantity(
                    Box::new(self.storage_value(value, &storage)?),
                    ty,
                ))
            })());
        }
        match &e.kind {
            Expression::Method(base, method, args, _)
                if method == "get"
                    && args.is_empty()
                    && self.natural(base).is_some_and(|ty| {
                        self.aliases.quantity(&ty).is_some() || ty.integer_domain().is_some()
                    }) =>
            {
                Some(self.eval(base, None).and_then(|value| match value {
                    Value::Quantity(value, _) => Ok(*value),
                    Value::Int(n, ty) => Ok(Value::Int(n, ty.domain_element().unwrap_or(ty))),
                    _ => Err(not_constant(e)),
                }))
            }
            Expression::Method(base, method, args, _)
                if method == "to"
                    && args.len() == 1
                    && self
                        .natural(base)
                        .is_some_and(|ty| self.aliases.quantity(&ty).is_some()) =>
            {
                Some((|| {
                    let left = self.eval(base, None)?;
                    let right = self.typed(&args[0], &left.ty())?;
                    let (Value::Quantity(a, _), Value::Quantity(b, _)) = (left, right) else {
                        unreachable!()
                    };
                    self.combine("/", *a, *b, &e.at)
                })())
            }
            Expression::Unary(op, inner)
                if self
                    .natural(inner)
                    .is_some_and(|ty| self.aliases.quantity(&ty).is_some()) =>
            {
                Some((|| {
                    if !matches!(op.as_str(), "+" | "-") {
                        return Err(e.at.error(format!("quantities do not support `{op}`")));
                    }
                    let Value::Quantity(value, ty) = self.eval(inner, expected)? else {
                        unreachable!()
                    };
                    let (_, dimension, _) = self.aliases.quantity(&ty).unwrap();
                    if op == "+" {
                        return Ok(Value::Quantity(value, ty));
                    }
                    let value = match *value {
                        Value::Int(n, storage) => {
                            let storage = storage.domain_element().unwrap_or(storage);
                            if !storage.int_range().is_some_and(|(min, _)| min < 0) {
                                return Err(e
                                    .at
                                    .error(format!("cannot negate unsigned {storage}")));
                            }
                            int(
                                -n,
                                &storage,
                                &e.at,
                                "constant arithmetic overflows",
                                self.spare(),
                            )?
                        }
                        Value::Float(n, storage) => Value::Float(-n, storage),
                        _ => unreachable!(),
                    };
                    let ty = self.quantity_type(&value.ty(), (*dimension).clone(), &e.at)?;
                    Ok(Value::Quantity(Box::new(value), ty))
                })())
            }
            _ => None,
        }
    }

    pub(super) fn quantity_result(
        &self,
        op: &str,
        left: &Ty,
        right: &Ty,
        at: &Token,
    ) -> Result<Ty> {
        let a = self.aliases.quantity(left);
        let b = self.aliases.quantity(right);
        let dimension = super::super::quantities::binary_dimension(
            op,
            a.as_ref().map(|(_, d, _)| d.as_ref()),
            b.as_ref().map(|(_, d, _)| d.as_ref()),
            at,
            self.aliases,
        )?;
        let left = a.map_or_else(|| left.clone(), |(storage, _, _)| storage);
        let right = b.map_or_else(|| right.clone(), |(storage, _, _)| storage);
        let storage = self.numeric_result(op, &left, &right, at)?;
        if is_comparison(op) {
            return Ok(Ty::Bool);
        }
        self.quantity_type(&storage, dimension, at)
    }

    pub(super) fn numeric_result(&self, op: &str, left: &Ty, right: &Ty, at: &Token) -> Result<Ty> {
        if matches!(op, "//" | "%")
            && left.is_int()
            && right.integer_domain().is_some_and(|(domain, inner)| {
                inner == *left && domain != IntegerDomain::NonNegative
            })
        {
            return Ok(left.clone());
        }
        if left != right {
            return Err(at.error(format!("cannot combine {left} with {right} in a constant")));
        }
        if is_comparison(op) {
            return Ok(Ty::Bool);
        }
        if let Some((domain, inner)) = left.integer_domain() {
            if !super::super::nonzero::forwards(op) {
                return Err(at.error(super::super::nonzero::unsupported(op, left)));
            }
            return Ok(
                if op == "*" || (op == "+" && domain != IntegerDomain::NonZero) {
                    left.clone()
                } else {
                    inner
                },
            );
        }
        if !left.is_numeric() {
            return Err(at.error("a quantity scalar must be numeric"));
        }
        Ok(left.clone())
    }

    pub(super) fn storage_context(&self, left: Option<&Ty>, right: Option<&Ty>) -> Option<Ty> {
        left.and_then(|ty| self.aliases.quantity(ty))
            .or_else(|| right.and_then(|ty| self.aliases.quantity(ty)))
            .map(|(storage, _, _)| storage)
            .or_else(|| {
                left.into_iter()
                    .chain(right)
                    .find(|ty| ty.integer_domain().is_some())
                    .cloned()
            })
    }

    /// Literal-domain inference matches ordinary operator lowering, including
    /// suffixed literals; an explicit incompatible width still fails typed().
    pub(super) fn operand_type(&self, e: &Expr, natural: Option<Ty>, storage: &Ty) -> Ty {
        if storage.integer_domain().is_some() && nonzero::literal(e) {
            storage.clone()
        } else {
            natural.unwrap_or_else(|| storage.clone())
        }
    }

    pub(super) fn wrapped_result(&self, op: &str, left: &Ty, right: &Ty, at: &Token) -> Result<Ty> {
        if self.aliases.quantity(left).is_some() || self.aliases.quantity(right).is_some() {
            self.quantity_result(op, left, right, at)
        } else {
            self.numeric_result(op, left, right, at)
        }
    }

    pub(super) fn wrapped_binary(
        &mut self,
        e: &Expr,
        op: &str,
        a: &Expr,
        b: &Expr,
    ) -> Option<Result<Value>> {
        let an = self.natural(a);
        let bn = self.natural(b);
        let storage = self.storage_context(an.as_ref(), bn.as_ref())?;
        Some((|| {
            let left_type = self.operand_type(a, an, &storage);
            let right_type = self.operand_type(b, bn, &storage);
            let ty = self.wrapped_result(op, &left_type, &right_type, &e.at)?;
            let left = self.storage_value(a, &left_type)?;
            let right = self.storage_value(b, &right_type)?;
            let unwrap = |v: Value| {
                if let Value::Quantity(value, _) = v {
                    *value
                } else {
                    v
                }
            };
            let left = unwrap(left);
            let right = unwrap(right);
            if is_comparison(op) {
                return compare(op, &left, &right, &e.at);
            }
            let value = self.combine(op, left, right, &e.at)?;
            Ok(if self.aliases.quantity(&ty).is_some() {
                Value::Quantity(Box::new(value), ty)
            } else {
                value
            })
        })())
    }

    pub(super) fn domain_combine(
        &self,
        op: &str,
        left: &Value,
        right: &Value,
        at: &Token,
    ) -> Option<Result<Value>> {
        if left.ty().integer_domain().is_none() && right.ty().integer_domain().is_none() {
            return None;
        }
        Some((|| {
            let result = self.numeric_result(op, &left.ty(), &right.ty(), at)?;
            let (Value::Int(a, left), Value::Int(b, _)) = (left, right) else {
                return Err(at.error("integer domains require integer values"));
            };
            let base = left.domain_element().unwrap_or_else(|| left.clone());
            let Value::Int(n, _) = integer_op(op, *a, *b, &base, at, self.spare())? else {
                unreachable!()
            };
            Ok(Value::Int(n, result))
        })())
    }
}
