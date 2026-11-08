//! Text builtins whose checked range form accepts substring views directly.
use super::*;
use crate::text::TextViewOp;

impl Lower<'_> {
    pub(super) fn text_method_output_hint(&self, base: &Expr, name: &str) -> Type {
        if self.slice_view_type(base, false) != Some(text_view()) {
            return None;
        }
        let result = |ty| crate::sum::result(ty, crate::sum::alloc_error());
        Some(match name {
            "concat" | "repeat" | "replace" | "removeprefix" | "removesuffix" | "lower"
            | "upper" | "strip" | "lstrip" | "rstrip" | "slice" | "join" => result(Ty::Str),
            "split" | "splitlines" => result(Ty::List(Arc::new(Ty::Str))),
            "find" | "rfind" => crate::sum::option(Ty::I64),
            "get" => crate::sum::option(Ty::Str),
            "count" | "byte_len" => Ty::I64,
            "startswith" | "endswith" | "isascii" | "isspace" | "isdigit" | "isalpha"
            | "isalnum" => Ty::Bool,
            _ => return None,
        })
    }

    /// Select the range backend only when an operand needs it. Ordinary str
    /// calls keep their existing cached-count and native-helper paths.
    pub(super) fn text_range_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Option<Result<Type>> {
        let receiver_view = self.is_text_view(base);
        let receiver_text = receiver_view || self.slice_view_type(base, false) == Some(text_view());
        if !receiver_text {
            return None;
        }
        let needs_range = receiver_view
            || args.iter().any(|arg| self.is_text_view(arg))
            || name == "join"
                && args.first().is_some_and(|arg| {
                    matches!(self.expression_type_hint(arg), Some(Ty::View(_, _)))
                });
        if !needs_range {
            return None;
        }
        let operation = match name {
            "get" => CollectionOp::TextGet,
            "concat" => CollectionOp::TextTryConcat,
            "repeat" => CollectionOp::TextTryRepeat,
            "replace" => CollectionOp::TextTryReplace,
            "startswith" => CollectionOp::TextStartsWith,
            "endswith" => CollectionOp::TextEndsWith,
            "find" => CollectionOp::TextFind,
            "rfind" => CollectionOp::TextRFind,
            "count" => CollectionOp::TextCount,
            "removeprefix" => CollectionOp::TextTryRemovePrefix,
            "removesuffix" => CollectionOp::TextTryRemoveSuffix,
            "isascii" => CollectionOp::TextIsAscii,
            "isspace" => CollectionOp::TextIsSpace,
            "isdigit" => CollectionOp::TextIsDigit,
            "isalpha" => CollectionOp::TextIsAlpha,
            "isalnum" => CollectionOp::TextIsAlnum,
            "lower" => CollectionOp::TextTryLower,
            "upper" => CollectionOp::TextTryUpper,
            "strip" => CollectionOp::TextTryStrip,
            "lstrip" => CollectionOp::TextTryLStrip,
            "rstrip" => CollectionOp::TextTryRStrip,
            "slice" => CollectionOp::TextTrySlice,
            "split" if args.is_empty() => CollectionOp::TextTrySplitWhitespace,
            "split" => CollectionOp::TextTrySplit,
            "splitlines" => CollectionOp::TextTrySplitLines,
            "join" => return Some(self.range_join(base, args, ops)),
            _ => return None,
        };
        Some((|| {
            let mut arguments = vec![base.clone()];
            arguments.extend_from_slice(args);
            if name == "splitlines" && args.is_empty() {
                arguments.push(Expr {
                    at: base.at.clone(),
                    kind: Expression::Bool(false),
                });
            }
            if name == "split"
                && matches!(args.first().map(|arg| &ungroup(arg).kind), Some(Expression::Text(t)) if t.is_empty())
            {
                return Err(args[0].at.error(
                    "split requires a nonempty separator; split on whitespace with `split()`",
                ));
            }
            self.system_call(TextViewOp::apply(operation), &arguments, &base.at, ops)
        })())
    }

    fn range_join(&mut self, base: &Expr, args: &[Expr], ops: &mut Vec<Op>) -> Result<Type> {
        let [source] = args else {
            return Err(base.at.error("join requires one argument"));
        };
        let (_, loan) = self.view_argument(base, false, true, ops)?;
        let result = self.join_text(source, true, ops)?;
        Self::end_reads(vec![loan], ops);
        Ok(Some(result))
    }
}
