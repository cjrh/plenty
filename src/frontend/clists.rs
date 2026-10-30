//! Restricted column owners and local borrowed scalar iteration plans.
use super::collections::Iteration;
use super::*;
use crate::clist::ClistOp;

pub(super) fn check_row(row: &Ty, at: &Token) -> Result<()> {
    let Ty::Class(class) = row else {
        return Err(at.error("clist requires a class with primitive scalar fields"));
    };
    let Some(class) = class.try_get() else {
        return Err(at.error("clist requires an already defined class"));
    };
    if class.fields.is_empty()
        || class.destructor.is_some()
        || class.fallible_init
        || class
            .fields
            .iter()
            .any(|(_, t)| !t.is_numeric() && *t != Ty::Bool)
    {
        return Err(at.error("clist requires nonempty primitive scalar fields and no custom destructor or fallible initializer"));
    }
    Ok(())
}

impl Lower<'_> {
    pub(super) fn check_clist_access(&self, ty: &Ty, at: &Token) -> Result<()> {
        let Ty::Class(class) = crate::clist::row(ty) else {
            unreachable!()
        };
        check_row(crate::clist::row(ty), at)?;
        for (name, _) in &class.get().fields {
            modules::check_member(self.access, &class.name, name, at)?;
        }
        Ok(())
    }

    pub(super) fn is_clist_column(&self, e: &Expr) -> bool {
        matches!(&ungroup(e).kind, Expression::Method(base, name, _, _)
            if name == "column" && matches!(self.place_type(base), Some(Ty::Clist(_))))
    }

    pub(super) fn clist_projection<'e>(
        &self,
        e: &'e Expr,
    ) -> Result<Option<(&'e Expr, &'e Expr, Ty, usize)>> {
        let Expression::Member(indexed, name, _) = &ungroup(e).kind else {
            return Ok(None);
        };
        let Expression::Index(base, index) = &ungroup(indexed).kind else {
            return Ok(None);
        };
        let Some(owner @ Ty::Clist(_)) = self.place_type(base) else {
            return Ok(None);
        };
        let Ty::Class(class) = crate::clist::row(&owner) else {
            unreachable!()
        };
        let field = classes::field_index(class, name, &e.at)?;
        modules::check_member(self.access, &class.name, name, &e.at)?;
        Ok(Some((base, index, owner, field)))
    }

    pub(super) fn set_clist_field(
        &mut self,
        target: &Expr,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let (base, index, owner, field) = self.clist_projection(target)?.unwrap();
        let ty = crate::clist::field(&owner, field);
        // Match ordinary field assignment: RHS, then index, then destination loan.
        let actual = self.expr_expected(value, Some(ty.clone()), ops)?;
        self.same(actual, Some(ty.clone()), &value.at)?;
        let input = self.slot(ty, &value.at)?;
        ops.push(Op::StoreLocal(input));
        let mut indices = Vec::new();
        self.access_indices(base, &mut indices, ops, false)?;
        let actual = self.expr_expected(index, Some(Ty::I64), ops)?;
        self.same(actual, Some(Ty::I64), &index.at)?;
        let position = self.slot(Ty::I64, &index.at)?;
        ops.push(Op::StoreLocal(position));
        let (_, loan) = self.borrow_with_indices(base, true, &mut Some(indices.iter()), ops)?;
        self.mark(&target.at, ops);
        ops.extend([
            Op::MoveLocal(position),
            Op::MoveLocal(input),
            Op::Collection(CollectionOp::Clist(ClistOp::Set(owner, field))),
            Op::Drop,
            Op::UseLoan(loan),
        ]);
        Ok(())
    }

    pub(super) fn clist_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let ty = self.place_type(base).unwrap();
        let op = match name {
            "reserve" => ClistOp::Reserve(ty),
            "append" => ClistOp::Append(ty),
            "remove" => {
                self.check_clist_access(&ty, &base.at)?;
                ClistOp::Remove(ty)
            }
            "clear" => ClistOp::Clear(ty),
            "copy_row" => {
                self.check_clist_access(&ty, &base.at)?;
                ClistOp::CopyRow(ty)
            }
            "len" => ClistOp::Len(ty),
            "capacity" => ClistOp::Capacity(ty),
            _ => return Err(base.at.error(format!("unsupported clist method `{name}`; use column, reserve, append, remove, clear, copy_row, len, or capacity"))),
        };
        let (inputs, output) = op.signature();
        if args.len() + 1 != inputs.len() {
            return Err(base
                .at
                .error(format!("{name} requires {} argument(s)", inputs.len() - 1)));
        }
        let Ty::Ref(_, mutable) = &inputs[0] else {
            unreachable!()
        };
        let mut indices = Vec::new();
        if !mutable {
            self.access_indices(base, &mut indices, ops, false)?;
        }
        let mut slots = Vec::new();
        for (arg, expected) in args.iter().zip(&inputs[1..]) {
            let actual = self.expr_expected(arg, Some(expected.clone()), ops)?;
            self.same(actual, Some(expected.clone()), &arg.at)?;
            let slot = self.slot(expected.clone(), &arg.at)?;
            ops.push(Op::StoreLocal(slot));
            slots.push(slot);
        }
        if *mutable {
            self.access_indices(base, &mut indices, ops, false)?;
        }
        let (_, loan) = self.borrow_with_indices(base, *mutable, &mut Some(indices.iter()), ops)?;
        for slot in slots {
            ops.push(Op::MoveLocal(slot));
        }
        ops.extend([Op::Collection(CollectionOp::Clist(op)), Op::UseLoan(loan)]);
        if output == Ty::Unit {
            ops.push(Op::Drop);
            Ok(None)
        } else {
            Ok(Some(output))
        }
    }

    pub(super) fn augment_clist_field(
        &mut self,
        target: &Expr,
        operator: &str,
        value: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let (base, index, owner, field) = self.clist_projection(target)?.unwrap();
        let ty = crate::clist::field(&owner, field);
        if !ty.is_numeric() {
            return Err(target
                .at
                .error("augmented assignment requires a numeric destination"));
        }
        let mut indices = Vec::new();
        self.access_indices(base, &mut indices, ops, false)?;
        let mut read_indices = Vec::new();
        for index in &indices {
            let key = self.locals[*index as usize - self.parameters].clone();
            let slot = self.slot(key, &target.at)?;
            ops.extend([Op::LoadLocal(*index), Op::StoreLocal(slot)]);
            read_indices.push(slot);
        }
        let actual = self.expr_expected(index, Some(Ty::I64), ops)?;
        self.same(actual, Some(Ty::I64), &index.at)?;
        let position = self.slot(Ty::I64, &index.at)?;
        ops.push(Op::StoreLocal(position));
        let (_, loan) =
            self.borrow_with_indices(base, false, &mut Some(read_indices.iter()), ops)?;
        self.mark(&target.at, ops);
        ops.extend([
            Op::LoadLocal(position),
            Op::Collection(CollectionOp::Clist(ClistOp::Get(owner.clone(), field))),
        ]);
        let actual = self.expr_expected(value, Some(ty.clone()), ops)?;
        self.same(actual, Some(ty.clone()), &value.at)?;
        self.mark_arithmetic(operator, &ty, &target.at, ops);
        self.numeric_operation(operator, ty.clone(), &target.at, ops)?;
        let result = self.slot(ty, &target.at)?;
        ops.extend([Op::StoreLocal(result), Op::UseLoan(loan)]);
        let (_, loan) = self.borrow_with_indices(base, true, &mut Some(indices.iter()), ops)?;
        self.mark(&target.at, ops);
        ops.extend([
            Op::MoveLocal(position),
            Op::MoveLocal(result),
            Op::Collection(CollectionOp::Clist(ClistOp::Set(owner, field))),
            Op::Drop,
            Op::UseLoan(loan),
        ]);
        Ok(())
    }

    pub(super) fn clist_iteration(
        &mut self,
        names: &[String],
        expression: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        if self.yield_type.is_some() {
            return Err(expression
                .at
                .error("column iteration is not supported inside generator functions"));
        }
        let [name] = names else {
            return Err(expression.at.error("column iteration requires one binding"));
        };
        let Expression::Method(base, _, args, _) = &ungroup(expression).kind else {
            unreachable!()
        };
        let [selector] = args.as_slice() else {
            return Err(expression
                .at
                .error("column requires one class field selector, e.g. Point.x"));
        };
        let Expression::Member(class_expr, name_field, _) = &ungroup(selector).kind else {
            return Err(selector
                .at
                .error("column requires a class field selector, e.g. Point.x"));
        };
        let ty = self.place_type(base).unwrap();
        let row = crate::clist::row(&ty);
        let selected = self.qualified_type(class_expr)?;
        self.same(selected, Some(row.clone()), &selector.at)?;
        let Ty::Class(class) = row else {
            unreachable!()
        };
        let field = classes::field_index(class, name_field, &selector.at)?;
        modules::check_member(self.access, &class.name, name_field, &selector.at)?;
        let element = class.get().fields[field].1.clone();
        let (_, loan) = self.borrow(base, false, ops)?;
        let receiver = self.slot(Ty::Ref(Arc::new(ty.clone()), false), &base.at)?;
        let length = self.slot(Ty::I64, &base.at)?;
        let address = self.slot(Ty::I64, &base.at)?;
        let index = self.slot(Ty::I64, &base.at)?;
        let target = self.slot(element.clone(), &base.at)?;
        ops.extend([
            Op::StoreLocal(receiver),
            Op::LoadLocal(receiver),
            Op::Collection(CollectionOp::Clist(ClistOp::Len(ty.clone()))),
            Op::StoreLocal(length),
            Op::LoadLocal(receiver),
            Op::Collection(CollectionOp::Clist(ClistOp::Column(ty, field))),
            Op::StoreLocal(address),
            Op::PushInt(Value::I64(0)),
            Op::StoreLocal(index),
        ]);
        self.bind(
            name.clone(),
            Local {
                slot: target,
                ty: element.clone(),
                mutable: false,
            },
        );
        Ok(Iteration {
            exact_len: Some(vec![Op::LoadLocal(length)]),
            cleanup: vec![],
            absent: vec![],
            condition: vec![
                Op::UseLoan(loan),
                Op::LoadLocal(index),
                Op::LoadLocal(length),
                Op::Lt,
            ],
            body: vec![
                Op::UseLoan(loan),
                Op::LoadLocal(address),
                Op::LoadLocal(index),
                Op::Collection(CollectionOp::Clist(ClistOp::Load(element))),
                Op::StoreLocal(target),
            ],
            step: vec![
                Op::LoadLocal(index),
                Op::PushInt(Value::I64(1)),
                Op::Integer(IntegerOp::WrappingAdd),
                Op::StoreLocal(index),
            ],
            target,
        })
    }
}
