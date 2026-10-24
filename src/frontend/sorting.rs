//! Sorting helpers are ordinary Plenty functions, specialized through the same
//! callable and loan checks as user code. Only total key comparison is intrinsic.
use super::*;

const KEY_OF: &str = include_str!("sorting.plenty");

impl Lower<'_> {
    fn sorting_helper(&mut self, name: &str, source: &str) -> Result<()> {
        if !self.generics.templates.contains_key(name) {
            let mut function = modules::generated_interface(source)?.functions.remove(0);
            function.name = name.into();
            self.generics.validate_template(&function, self.aliases)?;
            self.generics.retention.keep(&function.body);
            self.generics
                .templates
                .insert(name.into(), Arc::new(function));
        }
        Ok(())
    }

    fn reverse_argument(args: &[Expr], at: &Token) -> Expr {
        args.first().cloned().unwrap_or_else(|| Expr {
            at: at.clone(),
            kind: Expression::Bool(false),
        })
    }

    pub(super) fn list_sort(
        &mut self,
        base: &Expr,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if args.len() > 1 {
            return Err(base
                .at
                .error("sort takes no arguments or one descending bool"));
        }
        let descending = Self::reverse_argument(args, &base.at);
        let actual = self.value(&descending, ops)?;
        self.same(Some(actual), Some(Ty::Bool), &descending.at)?;
        let slot = self.slot(Ty::Bool, &descending.at)?;
        ops.push(Op::StoreLocal(slot));
        let ty = classes::unbox(self.place_type(base).expect("list place"));
        let Ty::List(element) = &ty else {
            unreachable!()
        };
        if !element.is_orderable() {
            return Err(base.at.error(format!(
                "sort does not order {element} elements; it orders integers, floats, str, and tuples of those"
            )));
        }
        let loan = self.mutation_place(base, ops)?;
        ops.push(Op::MoveLocal(slot));
        ops.push(Op::Collection(CollectionOp::ListSort(ty)));
        ops.push(Op::Drop);
        ops.push(Op::UseLoan(loan));
        Ok(None)
    }

    fn key_sort_call(
        &mut self,
        base: &Expr,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if !(1..=2).contains(&args.len()) {
            return Err(
                at.error("sort_by_key takes a key callback and an optional descending bool")
            );
        }
        let list = classes::unbox(self.place_type(base).expect("list place"));
        let Ty::List(element) = list else {
            unreachable!()
        };
        let mut callback = self
            .expression_type_hint(&args[0])
            .or_else(|| self.place_type(&args[0]))
            .ok_or_else(|| {
                args[0]
                    .at
                    .error("sort_by_key requires a named function or reusable closure")
            })?;
        while let Ty::Ref(inner, _) = callback {
            callback = (*inner).clone();
        }
        let sig = callables::signature(&callback)
            .ok_or_else(|| args[0].at.error("sort_by_key requires a key callback"))?;
        if sig.inputs != [Ty::Ref(element.clone(), false)] {
            return Err(args[0]
                .at
                .error(format!("sort_by_key callback must take &{element}")));
        }
        let key = sig.output.clone().filter(Ty::is_orderable).ok_or_else(|| {
            args[0]
                .at
                .error("sort_by_key callback must return an integer, float, str, or tuple of those")
        })?;
        self.sorting_helper("__plenty_key_of", KEY_OF)?;
        let key_of = self.specialize(
            "__plenty_key_of",
            vec![(*element).clone(), key.clone(), callback],
            at,
        )?;
        // The runtime sorts the rows and calls back into the generated
        // adapter, so elements move without per-access calls or refcounting.
        let sort_name = format!("{key_of}_sort");
        if !self.sigs.contains_key(&sort_name) {
            let callback = self.sigs[&key_of].inputs[0].1.clone();
            let list = Ty::List(element.clone());
            let sig = Arc::new(crate::op::FnSig {
                inputs: vec![
                    ("items".into(), Ty::Ref(Arc::new(list.clone()), true)),
                    ("key".into(), callback.clone()),
                    ("descending".into(), Ty::Bool),
                ],
                outputs: vec![],
            });
            let sort = crate::collection::KeySort {
                adapter: format!("{key_of}_order"),
                key_of,
                list,
                callback,
                key,
            };
            self.sigs.insert(sort_name.clone(), sig.clone());
            self.generics.compiled.push(Op::DefineFn(
                sort_name.clone(),
                Box::new(CompiledFn {
                    location: None,
                    origins: Arc::default(),
                    generator: None,
                    sig,
                    doc: "".into(),
                    locals: Arc::default(),
                    body: vec![
                        Op::LoadLocal(0),
                        Op::LoadLocal(1),
                        Op::LoadLocal(2),
                        Op::Collection(CollectionOp::ListSortBy(Arc::new(sort))),
                        Op::Drop,
                        Op::Return,
                    ]
                    .into(),
                }),
            ));
        }
        let descending = Self::reverse_argument(&args[1..], at);
        let receiver = Expr {
            at: base.at.clone(),
            kind: Expression::Unary("&mut".into(), Box::new(base.clone())),
        };
        self.call_named(
            &sort_name,
            &[receiver, args[0].clone(), descending],
            at,
            ops,
        )
    }

    pub(super) fn list_sort_by_key(
        &mut self,
        base: &Expr,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        self.key_sort_call(base, args, &base.at, ops)
    }

    pub(super) fn sorted(&mut self, args: &[Expr], at: &Token, ops: &mut Vec<Op>) -> Result<Type> {
        if !(1..=2).contains(&args.len()) {
            return Err(at.error("sorted takes a list and an optional descending bool; use sort_by_key on the copied list for key ordering"));
        }
        // Reject non-orderable elements here; otherwise the helper's body
        // reports them as a rejected instantiation of an internal name.
        let mut list = match &ungroup(&args[0]).kind {
            Expression::Unary(op, place) if op == "&" || op == "&mut" => self
                .place_type(place)
                .or_else(|| self.expression_type_hint(place)),
            _ => self.expression_type_hint(&args[0]),
        };
        while let Some(Ty::Ref(inner, _)) = list {
            list = Some((*inner).clone());
        }
        if let Some(Ty::List(element)) = list.map(classes::unbox)
            && !element.is_orderable()
        {
            return Err(at.error(format!(
                    "sorted does not order {element} elements; it orders integers, floats, str, and tuples of those"
                )));
        }
        self.sorting_helper("__plenty_sorted", "def sorteditems[T](items: &list[T], descending: bool) -> Result[list[T], AllocError]:\n    mut result = copy(items)?\n    result.sort(descending)\n    Ok(result)\n")?;
        let descending = Self::reverse_argument(&args[1..], at);
        self.generic_call(
            "__plenty_sorted",
            None,
            &[args[0].clone(), descending],
            at,
            ops,
        )
        .map_err(|error| {
            at.error(format!(
                "sorted requires a list and an optional descending bool: {}",
                error.to_string().replace("__plenty_sorted", "sorted")
            ))
        })
    }
}
