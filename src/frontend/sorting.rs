//! Sorting helpers are ordinary Plenty functions, specialized through the same
//! callable and loan checks as user code. Only total key comparison is intrinsic.
use super::*;

const KEY_OF: &str = include_str!("sorting.plenty");

impl Lower<'_> {
    /// Register `source`'s one function as template `name`; callers specialize it
    /// with ordinary generic calls.
    pub(super) fn plenty_helper(&mut self, name: &str, source: &str) -> Result<()> {
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
        stable: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let method = if stable {
            "sort_stable_by_key"
        } else {
            "sort_by_key"
        };
        if !(1..=2).contains(&args.len()) {
            return Err(at.error(format!(
                "{method} takes a key callback and an optional descending bool"
            )));
        }
        let list = classes::unbox(self.place_type(base).expect("list place"));
        let Ty::List(element) = list else {
            unreachable!()
        };
        // A closure literal is lowered into a hidden local first, so it
        // passes the same way as a stored closure.
        let mut args = args.to_vec();
        if let Expression::Anonymous(function) = &ungroup(&args[0]).kind {
            let ty = self.closure_value(function, &args[0].at, ops)?;
            let slot = self.slot(ty.clone(), &args[0].at)?;
            ops.push(Op::StoreLocal(slot));
            let name = format!("<sort key {slot}>");
            self.bind(
                name.clone(),
                Local {
                    slot,
                    ty,
                    mutable: false,
                },
            );
            args[0] = Expr {
                at: args[0].at.clone(),
                kind: Expression::Name(name),
            };
        }
        let args = &args[..];
        let mut callback = self
            .expression_type_hint(&args[0])
            .or_else(|| self.place_type(&args[0]))
            .ok_or_else(|| {
                args[0].at.error(format!(
                    "{method} requires a key callback: a function or closure"
                ))
            })?;
        while let Ty::Ref(inner, _) = callback {
            callback = (*inner).clone();
        }
        if let Ty::Closure(closure) = &callback
            && closure.mutable
        {
            return Err(args[0].at.error(format!(
                "{method} requires a shared-call key; this closure is mutable because it changes a `mut` capture"
            )));
        }
        let sig = callables::signature(&callback).ok_or_else(|| {
            args[0]
                .at
                .error(format!("{method} requires a key callback"))
        })?;
        if sig.inputs != [Ty::Ref(element.clone(), false)] {
            return Err(args[0]
                .at
                .error(format!("{method} callback must take &{element}")));
        }
        let output = sig.output.clone().ok_or_else(|| {
            args[0].at.error(format!(
                "{method} callback must return an ordered key or Result[key, E]"
            ))
        })?;
        let (key, error) = match &output {
            Ty::Enum(result) if result.is_result() => (
                result.get().variants[0].fields[0].clone(),
                Some(result.get().variants[1].fields[0].clone()),
            ),
            _ => (output.clone(), None),
        };
        if !key.is_orderable() {
            return Err(args[0].at.error(format!("{method} callback must return an integer, float, str, or tuple of those, optionally in Result[key, E]")));
        }
        self.plenty_helper("__plenty_key_of", KEY_OF)?;
        let key_of = self.specialize(
            "__plenty_key_of",
            vec![
                Argument::Type((*element).clone()),
                Argument::Type(output),
                Argument::Type(callback),
            ],
            at,
        )?;
        // The runtime sorts the rows and calls back into the generated
        // adapter, so elements move without per-access calls or refcounting.
        let sort_name = if stable {
            format!("{key_of}_stable_sort")
        } else {
            format!("{key_of}_sort")
        };
        if !self.sigs.contains_key(&sort_name) {
            let callback = self.sigs[&key_of].inputs[0].1.clone();
            let list = Ty::List(element.clone());
            let sig = Arc::new(crate::op::FnSig {
                inputs: vec![
                    ("items".into(), Ty::Ref(Arc::new(list.clone()), true)),
                    ("key".into(), callback.clone()),
                    ("descending".into(), Ty::Bool),
                ],
                outputs: crate::collection::key_sort_output(stable, error.as_ref())
                    .into_iter()
                    .collect(),
            });
            let sort = crate::collection::KeySort {
                adapter: format!("{key_of}_order"),
                key_of,
                list,
                callback,
                key,
                error: error.clone(),
                stable,
            };
            self.sigs.insert(sort_name.clone(), sig.clone());
            let mut body = vec![
                Op::LoadLocal(0),
                Op::LoadLocal(1),
                Op::LoadLocal(2),
                Op::Collection(CollectionOp::ListSortBy(Arc::new(sort))),
            ];
            if !stable && error.is_none() {
                body.push(Op::Drop);
            }
            body.push(Op::Return);
            self.generics.compiled.push(Op::DefineFn(
                sort_name.clone(),
                Box::new(CompiledFn {
                    location: None,
                    origins: Arc::default(),
                    generator: None,
                    sig,
                    doc: "".into(),
                    locals: Arc::default(),
                    body: body.into(),
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
        self.key_sort_call(base, args, &base.at, false, ops)
    }

    pub(super) fn list_sort_stable_by_key(
        &mut self,
        base: &Expr,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        self.key_sort_call(base, args, &base.at, true, ops)
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
        self.plenty_helper("__plenty_sorted", "def sorteditems[T](items: &list[T], descending: bool) -> Result[list[T], AllocError]:\n    mut result = copy(items)?\n    result.sort(descending)\n    Ok(result)\n")?;
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
