//! `__list_retag[list[U]](xs)`: std.units' zero-copy conversion between a
//! list of quantities and a list of their storage. The element types differ
//! only by a `Quantity` layer, so the rows are identical and the runtime only
//! needs the header's type descriptor replaced.
use super::*;

pub(super) const LIST_RETAG: &str = "__list_retag";

/// The element type with one `Quantity` layer removed.
fn storage(element: &Ty) -> Ty {
    element
        .quantity_storage()
        .unwrap_or_else(|| element.clone())
}

impl Lower<'_> {
    pub(super) fn list_retag(
        &mut self,
        types: &[TypeRef],
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let standard = at
            .source()
            .as_ref()
            .is_some_and(|source| self.generics.source_facts(source).1);
        if !standard {
            return Err(at.error(format!(
                "`{LIST_RETAG}` is reserved for the standard library"
            )));
        }
        let ([target], [value]) = (types, args) else {
            return Err(at.error(format!(
                "`{LIST_RETAG}` takes one target list type and one owned list"
            )));
        };
        let target_error = || {
            target
                .at
                .error(format!("`{LIST_RETAG}` needs a list target type"))
        };
        let to = target.resolve(self.aliases)?.ok_or_else(target_error)?;
        let Ty::List(to_element) = &to else {
            return Err(target_error());
        };
        let from = self.value(value, ops)?;
        let Ty::List(from_element) = &from else {
            return Err(value
                .at
                .error(format!("`{LIST_RETAG}` needs an owned list, got {from}")));
        };
        if from == to || storage(from_element) != storage(to_element) {
            return Err(at.error(format!(
                "`{LIST_RETAG}` converts only between a quantity and its storage, not {from} and {to}"
            )));
        }
        ops.push(Op::Collection(CollectionOp::ListRetag(Box::new((
            from,
            to.clone(),
        )))));
        Ok(Some(to))
    }
}
