//! Binary string convenience syntax over the view-based Plenty implementation.
//! Ordinary references and non-byte slice views keep their existing semantics.
use super::*;
use crate::bytes::BytesOp;

/// Method names whose byte implementation lives in `std.bytes`. The parser
/// uses this list only to arrange loading, before receiver types are known.
pub(super) fn supported(name: &str) -> bool {
    matches!(
        name,
        "find"
            | "rfind"
            | "count"
            | "startswith"
            | "endswith"
            | "isascii"
            | "isspace"
            | "isdigit"
            | "isalpha"
            | "isalnum"
            | "lower"
            | "upper"
            | "strip"
            | "lstrip"
            | "rstrip"
            | "split"
            | "replace"
            | "join"
    )
}

fn byte_sequence(ty: &Ty) -> bool {
    match ty {
        Ty::Ref(inner, _) => byte_sequence(inner),
        Ty::ByteStr => true,
        Ty::View(element, _) => **element == Ty::U8,
        ty => byte_buffers::byte_buffer(ty).is_some(),
    }
}

fn byte_buffer_receiver(ty: &Ty) -> bool {
    match ty {
        Ty::Ref(inner, _) => byte_buffer_receiver(inner),
        ty => byte_buffers::byte_buffer(ty).is_some(),
    }
}

fn byte_needle(ty: &Ty) -> bool {
    match ty {
        Ty::Ref(inner, _) => byte_needle(inner),
        Ty::List(element) if **element == Ty::U8 => true,
        ty => byte_sequence(ty),
    }
}

impl Lower<'_> {
    fn byte_argument_hint(&self, value: &Expr) -> Type {
        match &ungroup(value).kind {
            Expression::Unary(op, inner) if op == "&" || op == "&mut" => {
                if let Expression::Slice(owner, _) = &ungroup(inner).kind {
                    return self.slice_view_type(owner, op == "&mut");
                }
                self.operator_hint(value)
            }
            _ => self.operator_hint(value),
        }
    }

    fn byte_view_argument(&self, value: &Expr) -> Expr {
        if self.is_view(value)
            || matches!(self.operator_hint(value), Some(Ty::Ref(..)))
            || matches!(&value.kind, Expression::Bytes(_))
            || matches!(&value.kind, Expression::Unary(op, _) if op == "&" || op == "&mut")
        {
            value.clone()
        } else {
            Expr {
                kind: Expression::Unary("&".into(), Box::new(value.clone())),
                at: value.at.clone(),
            }
        }
    }

    pub(super) fn byte_string_method_hint(&self, base: &Expr, name: &str, args: &[Expr]) -> Type {
        if !supported(name)
            || !self
                .byte_argument_hint(base)
                .as_ref()
                .is_some_and(byte_sequence)
        {
            return None;
        }
        match name {
            "find" | "rfind" => Some(crate::sum::option(Ty::I64)),
            "count" => Some(Ty::I64),
            "startswith" | "endswith" | "isascii" | "isspace" | "isdigit" | "isalpha"
            | "isalnum" => Some(Ty::Bool),
            "split" => self
                .sigs
                .get(if args.is_empty() {
                    "std.bytes.split_whitespace"
                } else {
                    "std.bytes.split"
                })?
                .outputs
                .first()
                .cloned(),
            _ => self.sigs.get("std.bytes.lower")?.outputs.first().cloned(),
        }
    }

    pub(super) fn byte_string_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Option<Type>> {
        if !supported(name) {
            return Ok(None);
        }
        let Some(receiver) = self.byte_argument_hint(base) else {
            return Ok(None);
        };
        if !byte_sequence(&receiver) {
            return Ok(None);
        }
        // Keep all existing explicit range calls on Bytes unchanged.
        if matches!(name, "find" | "count") && args.len() == 3 && byte_buffer_receiver(&receiver) {
            return Ok(None);
        }
        let byte = args.first().is_some_and(|arg| {
            matches!(self.byte_argument_hint(arg), Some(Ty::U8))
                || matches!(&arg.kind, Expression::Number(..))
        });
        let function = match name {
            "find" if byte => "find_byte",
            "rfind" if byte => "rfind_byte",
            "count" if byte => "count_byte",
            "startswith" => "starts_with",
            "endswith" => "ends_with",
            "isascii" => "is_ascii",
            "isspace" => "is_space",
            "isdigit" => "is_digit",
            "isalpha" => "is_alpha",
            "isalnum" => "is_alnum",
            "split" if args.is_empty() => "split_whitespace",
            other => other,
        };
        let mut inputs = Vec::with_capacity(args.len() + 1);
        inputs.push(self.byte_view_argument(base));
        for arg in args {
            inputs.push(
                if self
                    .byte_argument_hint(arg)
                    .as_ref()
                    .is_some_and(byte_needle)
                {
                    self.byte_view_argument(arg)
                } else {
                    arg.clone()
                },
            );
        }
        self.call_named(&format!("std.bytes.{function}"), &inputs, &base.at, ops)
            .map(Some)
    }

    pub(super) fn byte_membership(
        &mut self,
        op: &str,
        left: &Expr,
        right: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Type>> {
        if !self
            .byte_argument_hint(right)
            .as_ref()
            .is_some_and(byte_sequence)
        {
            return Ok(None);
        }
        let sequence = self
            .byte_argument_hint(left)
            .as_ref()
            .is_some_and(byte_needle);
        let mut loans = Vec::new();
        if sequence {
            let (_, loan) =
                self.view_argument(&self.byte_view_argument(left), false, false, ops)?;
            loans.push(loan);
        } else {
            let (actual, reads) = self.observe_expected(left, Some(Ty::U8), ops)?;
            self.same(Some(actual), Some(Ty::U8), &left.at)?;
            loans.extend(reads);
        }
        let (_, loan) = self.view_argument(&self.byte_view_argument(right), false, false, ops)?;
        loans.push(loan);
        // Evaluate the needle before the haystack, as other `in` expressions
        // do, then arrange the existing native scan's argument order.
        ops.push(Op::Swap);
        ops.push(Op::Collection(CollectionOp::Bytes(if sequence {
            BytesOp::FindBytes
        } else {
            BytesOp::Find
        })));
        Self::end_reads(loans, ops);
        ops.push(Op::PushInt(Value::I64(-1)));
        ops.push(if op == "not in" { Op::Eq } else { Op::Ne });
        Ok(Some(Some(Ty::Bool)))
    }
}
