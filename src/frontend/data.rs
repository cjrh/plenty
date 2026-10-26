//! Demand-driven concrete data types. Specializations share nominal identities.
use super::*;
use crate::ownership::Instantiation;
use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

/// Instances of one generic class or enum. Expansion that branches, such as
/// `Node[T]` holding both `Node[list[T]]` and `Node[Box[T]]`, reaches it
/// within the nesting limit of 64.
const MAX_DECLARATION_INSTANCES: usize = 16_384;
/// Generic data instances per compilation, a resource budget for expansion
/// that grows several declarations together.
const MAX_DATA_INSTANCES: usize = 65_536;

#[derive(Clone, Default)]
pub(crate) struct TypeAliases {
    named: HashMap<String, Type>,
    /// Checking state of one compilation's frontend; it never leaves its thread.
    pub(super) data: Rc<DataTypes>,
}

#[derive(Default)]
pub(super) struct DataTypes {
    enums: Arc<HashMap<String, enums::EnumDecl>>,
    classes: Arc<HashMap<String, classes::ClassDecl>>,
    instances: RefCell<HashMap<(String, Vec<Ty>), Ty>>,
    /// Instances with type arguments, which `MAX_DATA_INSTANCES` limits.
    generic_instances: Cell<usize>,
    /// Those instances by declaration, which `MAX_DECLARATION_INSTANCES` limits.
    declaration_instances: RefCell<HashMap<String, usize>>,
    named: RefCell<HashMap<String, Ty>>,
    arguments: RefCell<HashMap<String, (String, Vec<Ty>)>>,
    pending: RefCell<Vec<classes::ClassDecl>>,
    pending_enums: RefCell<Vec<enums::EnumDecl>>,
    definitions: Arc<crate::nominal::Types>,
    pending_types: RefCell<std::collections::VecDeque<DefinitionJob>>,
    /// Column eligibility depends on the complete class schema, including
    /// generic instances whose definitions are still in the queue.
    pending_clists: RefCell<Vec<(Ty, Token)>>,
    deferred: Cell<bool>,
    depth: Cell<usize>,
    /// The instance being defined, whose fields request further instances.
    requirer: RefCell<Option<Arc<Requirer>>>,
    /// What instances created now are attributed to. Without one, an
    /// instance is required by the type application that names it.
    requiring: RefCell<Option<Requiring>>,
    /// The use that first required each generic class instance, by concrete
    /// name, for the diagnostics of its methods.
    required: RefCell<HashMap<String, Arc<Instantiation>>>,
    /// Signature-shape probes use placeholder arguments, not real instances.
    shapes_only: bool,
    /// The shape-only tables, created by the first probe and shared by the rest.
    shapes: RefCell<Option<TypeAliases>>,
}

struct DefinitionJob {
    name: String,
    actual: Vec<Ty>,
    at: Token,
    depth: usize,
    requirer: Option<Arc<Requirer>>,
    required: Arc<Instantiation>,
}

/// The reason data instances are being created, when it is not the type
/// application itself: a type written in a generic declaration is at the
/// declaration, but the instance belongs to the use that supplied the type
/// arguments.
#[derive(Clone)]
pub(super) enum Requiring {
    /// Code or types of this generic instance.
    Instance(Arc<Instantiation>),
    /// A generic use in ordinary code whose instance is not known yet.
    Use(Token),
}

impl DataTypes {
    /// Replaces the current attribution and returns the previous one, which
    /// the caller restores.
    pub(super) fn attribute(&self, to: Option<Requiring>) -> Option<Requiring> {
        self.requiring.replace(to)
    }

    /// Like `attribute`, but an enclosing attribution keeps its claim: inside
    /// a generic instance, the use in ordinary code is the one to report.
    pub(super) fn attribute_within(&self, to: impl FnOnce() -> Requiring) -> Option<Requiring> {
        let outer = self.requiring.borrow().clone();
        if outer.is_none() {
            *self.requiring.borrow_mut() = Some(to());
        }
        outer
    }
}

/// One link in the chain of instances that required the current one. Walked
/// only to explain an expansion that reaches a specialization limit.
struct Requirer {
    declaration: String,
    instance: String,
    /// The type application that requested this instance.
    at: Token,
    parent: Option<Arc<Requirer>>,
}

impl Deref for TypeAliases {
    type Target = HashMap<String, Type>;
    fn deref(&self) -> &Self::Target {
        &self.named
    }
}
impl DerefMut for TypeAliases {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.named
    }
}
impl DataTypes {
    pub(super) fn is_class(&self, name: &str) -> bool {
        self.classes.contains_key(name)
    }
    pub(super) fn contains(&self, name: &str) -> bool {
        self.enums.contains_key(name) || self.classes.contains_key(name)
    }
    pub(super) fn lookup(&self, name: &str) -> Type {
        self.named.borrow().get(name).cloned()
    }
    pub(super) fn arguments(&self, ty: &Ty) -> Option<(String, Vec<Ty>)> {
        let name = match ty {
            Ty::Class(t) => &t.name,
            Ty::Enum(t) => &t.name,
            _ => return None,
        };
        self.arguments.borrow().get(&**name).cloned()
    }

    /// The reporting method of the toolchain's Context is available only when
    /// its cause can be reported. Nominal identity and source provenance both
    /// matter: a user type with the same short name has no special behavior.
    pub(super) fn report_context(
        &self,
        ty: &Ty,
        standard_sources: &HashSet<Arc<str>>,
    ) -> Option<(Ty, protocols::Protocol)> {
        let (name, arguments) = self.arguments(ty)?;
        if name != "std.error.Context" {
            return None;
        }
        let declaration = self.classes.get(&name)?;
        if !declaration
            .at
            .source()
            .as_ref()
            .is_some_and(|source| standard_sources.contains(source))
        {
            return None;
        }
        let [cause] = arguments.as_slice() else {
            return None;
        };
        let mut method = declaration
            .methods
            .iter()
            .find(|method| method.name == "format")?
            .clone();
        let name = "std.report.Formattable".to_owned();
        method.inputs[0].1 = TypeRef {
            concrete: None,
            at: declaration.at.clone(),
            name: Some("&".into()),
            args: vec![TypeRef {
                concrete: None,
                at: declaration.at.clone(),
                name: Some(name.clone()),
                args: vec![],
            }],
        };
        Some((
            cause.clone(),
            protocols::Protocol {
                name,
                at: declaration.at.clone(),
                type_params: vec![],
                methods: vec![method],
            },
        ))
    }

    pub(super) fn factories(&self) -> Vec<Function> {
        let mut classes: Vec<_> = self
            .classes
            .values()
            .filter(|c| !c.type_params.is_empty())
            .collect();
        classes.sort_by(|a, b| a.name.cmp(&b.name));
        classes
            .into_iter()
            .map(|class| {
                let at = &class.at;
                let reference = |name: String, args: Vec<TypeRef>| TypeRef {
                    concrete: None,
                    at: at.clone(),
                    name: Some(name),
                    args,
                };
                let types: Vec<_> = class
                    .type_params
                    .iter()
                    .map(|(n, _)| reference(n.clone(), vec![]))
                    .collect();
                let inputs = class
                    .methods
                    .iter()
                    .find(|m| m.name == "__init__")
                    .map(|m| m.inputs.iter().skip(1).cloned().collect())
                    .unwrap_or_else(|| class.fields.clone());
                let args = inputs
                    .iter()
                    .map(|(n, _)| Expr {
                        at: at.clone(),
                        kind: Expression::Name(n.clone()),
                    })
                    .collect();
                Function {
                    once: false,
                    captures: vec![],
                    foreign: None,
                    export: None,
                    defaults: class
                        .methods
                        .iter()
                        .find(|m| m.name == "__init__")
                        .map(|m| m.defaults.iter().skip(1).cloned().collect())
                        .unwrap_or_default(),
                    name: class.name.clone(),
                    type_params: class.type_params.clone(),
                    at: at.clone(),
                    inputs,
                    // Only a fallible initializer makes construction fail.
                    output: if class
                        .methods
                        .iter()
                        .any(|m| m.name == "__init__" && m.output.name.is_some())
                    {
                        reference(
                            "Result".into(),
                            vec![
                                reference(class.name.clone(), types.clone()),
                                reference("AllocError".into(), vec![]),
                            ],
                        )
                    } else {
                        reference(class.name.clone(), types.clone())
                    },
                    doc: String::new(),
                    doc_at: None,
                    body: vec![Stmt {
                        at: at.clone(),
                        kind: Statement::Expr(Expr {
                            at: at.clone(),
                            kind: Expression::GenericCall(class.name.clone(), types, args),
                        }),
                    }],
                }
            })
            .collect()
    }
}
impl TypeAliases {
    pub(super) fn validate_data_bounds(&self) -> Result<()> {
        let mut declarations: Vec<_> = self
            .data
            .enums
            .values()
            .map(|e| (&e.name, &e.type_params))
            .chain(
                self.data
                    .classes
                    .values()
                    .map(|c| (&c.name, &c.type_params)),
            )
            .collect();
        declarations.sort_by_key(|(name, _)| *name);
        for (_, params) in declarations {
            let bounds: Vec<_> = params
                .iter()
                .filter_map(|(_, b)| b.as_ref())
                .filter(|b| generics::callable_bound(b))
                .collect();
            if bounds.is_empty() {
                continue;
            }
            self.probe(params.iter().map(|(name, _)| name), |validation| {
                for bound in bounds {
                    generics::callable_pattern(bound).resolve(validation)?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }
    /// Runs a signature-shape check with `placeholders` naming arbitrary types.
    ///
    /// Shape checks must not enqueue placeholder data instances in the real
    /// program, so they resolve against a separate copy of the tables. Every
    /// probe shares one copy: a copy per declaration made checking quadratic
    /// in program size (issue #53). The copy is taken at the first probe, which
    /// must follow `resolve_types`; later real instances are not needed, since
    /// a probe only asks whether a signature resolves.
    pub(super) fn probe<'a, T>(
        &self,
        placeholders: impl IntoIterator<Item = &'a String>,
        check: impl FnOnce(&TypeAliases) -> Result<T>,
    ) -> Result<T> {
        self.probe_bound(placeholders.into_iter().map(|name| (name, Ty::I64)), check)
    }
    /// `probe` with a chosen type for each placeholder, so that signatures
    /// naming different placeholders resolve to different types.
    pub(super) fn probe_bound<'a, T>(
        &self,
        placeholders: impl IntoIterator<Item = (&'a String, Ty)>,
        check: impl FnOnce(&TypeAliases) -> Result<T>,
    ) -> Result<T> {
        let mut shapes = self.data.shapes.borrow_mut();
        let validation = shapes.get_or_insert_with(|| Self {
            named: self.named.clone(),
            data: Rc::new(DataTypes {
                enums: self.data.enums.clone(),
                classes: self.data.classes.clone(),
                instances: self.data.instances.clone(),
                named: self.data.named.clone(),
                arguments: self.data.arguments.clone(),
                shapes_only: true,
                ..DataTypes::default()
            }),
        });
        let shadowed: Vec<_> = placeholders
            .into_iter()
            .map(|(name, ty)| (name, validation.named.insert(name.clone(), Some(ty))))
            .collect();
        let result = check(validation);
        // Restore in reverse so a repeated placeholder gets its original binding back.
        for (name, previous) in shadowed.into_iter().rev() {
            match previous {
                Some(ty) => validation.named.insert(name.clone(), ty),
                None => validation.named.remove(name),
            };
        }
        // Placeholder specializations are never lowered.
        validation.data.pending.borrow_mut().clear();
        validation.data.pending_enums.borrow_mut().clear();
        result
    }
    pub(super) fn with_data(
        enums: &[enums::EnumDecl],
        classes: &[classes::ClassDecl],
    ) -> Result<Self> {
        let mut enum_declarations = HashMap::default();
        for e in enums {
            validate_parameters(&e.name, &e.type_params, &e.at)?;
            enum_declarations.insert(e.name.clone(), e.template());
        }
        let mut class_declarations = HashMap::default();
        for c in classes {
            validate_parameters(&c.name, &c.type_params, &c.at)?;
            class_declarations.insert(c.name.clone(), c.template());
        }
        let data = DataTypes {
            enums: Arc::new(enum_declarations),
            classes: Arc::new(class_declarations),
            ..DataTypes::default()
        };
        data.deferred.set(true);
        Ok(Self {
            named: HashMap::default(),
            data: Rc::new(data),
        })
    }

    pub(super) fn instantiate(&self, application: &TypeRef) -> Result<Ty> {
        let name = application.name.as_ref().unwrap();
        let params = if let Some(e) = self.data.enums.get(name) {
            &e.type_params
        } else {
            &self.data.classes[name].type_params
        };
        if application.args.len() != params.len() {
            return Err(application.at.error(format!(
                "generic type `{name}` requires {} type arguments",
                params.len()
            )));
        }
        let actual = application
            .args
            .iter()
            .map(|t| {
                let ty = t
                    .resolve(self)?
                    .ok_or_else(|| t.at.error("unit type arguments are not supported yet"))?;
                if let Some(barrier) = ty.storage_barrier() {
                    return Err(t.at.error(barrier.message("generic data")));
                }
                Ok(ty)
            })
            .collect::<Result<Vec<_>>>()?;
        let key = (name.clone(), actual.clone());
        if let Some(ty) = self.data.instances.borrow().get(&key) {
            return Ok(ty.clone());
        }
        // Placeholder instances do not count against the program's limits.
        // As for generic functions (issue #430), the definition depth below
        // bounds expansion, the per-declaration count bounds branching, and
        // the total is a resource budget.
        let counted = !actual.is_empty() && !self.data.shapes_only;
        if counted {
            let count = self
                .data
                .declaration_instances
                .borrow()
                .get(name)
                .copied()
                .unwrap_or(0);
            if count >= MAX_DECLARATION_INSTANCES {
                return Err(self.expansion().unwrap_or_else(|| {
                    application.at.error(format!(
                        "generic data specialization limit of {MAX_DECLARATION_INSTANCES} instances of `{name}` exceeded; check for expanding recursion"
                    ))
                }));
            }
            if self.data.generic_instances.get() >= MAX_DATA_INSTANCES {
                return Err(self.expansion().unwrap_or_else(|| {
                    application.at.error(format!(
                        "total generic data specialization budget of {MAX_DATA_INSTANCES} exceeded"
                    ))
                }));
            }
        }
        if self.data.depth.get() >= 64 {
            return Err(self.expansion().unwrap_or_else(|| {
                application.at.error("recursive generic data type or type nesting exceeds the implementation limit of 64")
            }));
        }
        let concrete_name = instance_name(name, &actual, Ty::identity_name);
        if concrete_name.len() > crate::op::MAX_IDENTITY_NAME_LEN {
            return Err(application
                .at
                .error("concrete type name exceeds the implementation limit"));
        }
        let display_name = instance_name(name, &actual, ToString::to_string);
        let ty = if self.data.enums.contains_key(name) {
            Ty::Enum(
                self.data
                    .definitions
                    .enumeration(&concrete_name)
                    .with_display_name(display_name),
            )
        } else {
            Ty::Class(
                self.data
                    .definitions
                    .class(&concrete_name)
                    .with_display_name(display_name),
            )
        };
        self.data
            .arguments
            .borrow_mut()
            .insert(concrete_name.clone(), key.clone());
        self.data.instances.borrow_mut().insert(key, ty.clone());
        if counted {
            self.data
                .generic_instances
                .set(self.data.generic_instances.get() + 1);
            *self
                .data
                .declaration_instances
                .borrow_mut()
                .entry(name.clone())
                .or_default() += 1;
        }
        self.data
            .named
            .borrow_mut()
            .insert(concrete_name, ty.clone());
        let instance: Arc<str> = ty.to_string().into();
        let required = Arc::new(match &*self.data.requiring.borrow() {
            Some(Requiring::Instance(outer)) => Instantiation {
                instance,
                ..(**outer).clone()
            },
            Some(Requiring::Use(at)) => Instantiation {
                at: at.location().into(),
                requested: instance.clone(),
                instance,
            },
            None => Instantiation {
                at: application.at.location().into(),
                requested: instance.clone(),
                instance,
            },
        });
        self.data
            .pending_types
            .borrow_mut()
            .push_back(DefinitionJob {
                name: name.clone(),
                actual,
                at: application.at.clone(),
                depth: self.data.depth.get() + 1,
                requirer: self.data.requirer.borrow().clone(),
                required,
            });
        if !self.data.deferred.get() {
            self.finish_types()?;
        }
        Ok(ty)
    }

    /// Explains a specialization limit reached by endless expansion: one
    /// declaration required again and again with growing type arguments.
    /// Finite nesting that is merely deep keeps the limit's own message.
    fn expansion(&self) -> Option<Box<dyn std::error::Error>> {
        let mut chain = Vec::new();
        let mut link = self.data.requirer.borrow().clone();
        while let Some(requirer) = link {
            link = requirer.parent.clone();
            chain.push(requirer);
        }
        chain.reverse();
        chain.iter().find_map(|first| {
            let instances: Vec<&Arc<Requirer>> = chain
                .iter()
                .filter(|r| r.declaration == first.declaration)
                .take(3)
                .collect();
            let [a, b, c] = instances[..] else {
                return None;
            };
            let (x, y, z) = (&a.instance, &b.instance, &c.instance);
            (x.len() < y.len() && y.len() < z.len()).then(|| {
                b.at.error(format!(
                    "generic type `{}` expands without end: `{x}` requires `{y}`, which requires `{z}`; a recursive use must not grow its type arguments",
                    first.declaration
                ))
            })
        })
    }

    pub(super) fn finish_types(&self) -> Result<()> {
        self.data.deferred.set(true);
        let attributed = self.data.requiring.borrow().clone();
        let result = (|| {
            // Instances defined by earlier calls were checked then, and their
            // layouts cannot change: rechecking every instance made checking
            // quadratic in the number of instances (issue #430).
            let mut defined = Vec::new();
            loop {
                let next = self.data.pending_types.borrow_mut().pop_front();
                let Some(DefinitionJob {
                    name,
                    actual,
                    at,
                    depth,
                    requirer,
                    required,
                }) = next
                else {
                    break;
                };
                self.data.depth.set(depth);
                // Instances named by this one's fields and methods have the same cause.
                *self.data.requiring.borrow_mut() = Some(Requiring::Instance(required.clone()));
                let ty = self.data.instances.borrow()[&(name.clone(), actual.clone())].clone();
                defined.push((name.clone(), ty.clone()));
                let concrete_name = ty.identity_name();
                // Diagnostics show the display name, not the linking identity.
                *self.data.requirer.borrow_mut() = Some(Arc::new(Requirer {
                    declaration: name.clone(),
                    instance: ty.to_string(),
                    at: at.clone(),
                    parent: requirer,
                }));
                let params = if let Some(e) = self.data.enums.get(&name) {
                    &e.type_params
                } else {
                    &self.data.classes[&name].type_params
                };
                let substitutions = params
                    .iter()
                    .map(|(n, _)| n.clone())
                    .zip(actual.iter().map(|t| generics::type_ref(t, &at)))
                    .collect();
                // Resolve constraints after aliases and nominal identities exist.
                // Nested applications join this bounded work queue instead of
                // recursively rechecking their own declaration's constraints.
                if !self.data.shapes_only {
                    for ((_, bound), ty) in params.iter().zip(&actual) {
                        if let Some(bound) = bound {
                            if generics::callable_bound(bound) {
                                generics::check_callable_bound(
                                    bound,
                                    ty,
                                    &substitutions,
                                    self,
                                    &at,
                                )?;
                            } else if generics::numeric_bound(bound)
                                .is_some_and(|satisfies| !satisfies(ty))
                            {
                                return Err(at.error(format!(
                                    "{ty} does not satisfy {}",
                                    bound.name.as_deref().unwrap()
                                )));
                            }
                        }
                    }
                }
                let result: Result<(Ty, Option<classes::ClassDecl>)> = (|| {
                    // A concrete declaration has nothing to substitute, so it
                    // is resolved in place rather than copied.
                    if actual.is_empty() {
                        return Ok((
                            match self.data.enums.get(&name) {
                                Some(declaration) => self.resolve_enum(declaration)?,
                                None => self.data.classes[&name].resolve(self)?,
                            },
                            None,
                        ));
                    }
                    if let Some(template) = self.data.enums.get(&name) {
                        let mut declaration = template.clone();
                        declaration.name = concrete_name.clone();
                        declaration.type_params.clear();
                        for (_, fields) in &mut declaration.variants {
                            for field in fields {
                                generics::substitute(field, &substitutions)?;
                            }
                        }
                        for method in &mut declaration.methods {
                            substitute_method(method, &substitutions)?;
                        }
                        let resolved = self.resolve_enum(&declaration)?;
                        self.data
                            .required
                            .borrow_mut()
                            .insert(concrete_name.clone(), required.clone());
                        self.data.pending_enums.borrow_mut().push(declaration);
                        Ok((resolved, None))
                    } else {
                        let mut declaration = self.data.classes[&name].clone();
                        declaration.name = concrete_name.clone();
                        declaration.type_params.clear();
                        for (_, field) in &mut declaration.fields {
                            generics::substitute(field, &substitutions)?;
                        }
                        for method in &mut declaration.methods {
                            substitute_method(method, &substitutions)?;
                        }
                        Ok((declaration.resolve(self)?, Some(declaration)))
                    }
                })();
                let (definition, class): (Ty, Option<classes::ClassDecl>) = result?;
                match (&ty, definition) {
                    (Ty::Class(t), Ty::Class(value)) => t.define((*value.get()).clone()),
                    (Ty::Enum(t), Ty::Enum(value)) => t.define((*value.get()).clone()),
                    _ => unreachable!(),
                }
                if let Some(class) = class.filter(|_| !params.is_empty()) {
                    self.data
                        .required
                        .borrow_mut()
                        .insert(class.name.clone(), required);
                    self.data.pending.borrow_mut().push(class);
                }
            }
            for (row, at) in std::mem::take(&mut *self.data.pending_clists.borrow_mut()) {
                super::clists::check_row(&row, &at)?;
            }
            for (name, ty) in &defined {
                let at = || {
                    self.data
                        .enums
                        .get(name)
                        .map(|d| &d.at)
                        .unwrap_or_else(|| &self.data.classes[name].at)
                };
                if ty.facts().infinite {
                    return Err(at().error(format!(
                        "`{ty}` contains itself and would have infinite size; store the recursive field in a `Box`, list, set, or dict"
                    )));
                }
                if ty.layout_depth() > 64 {
                    return Err(at().error("type nesting exceeds the implementation limit of 64"));
                }
            }
            Ok(())
        })();
        self.data.deferred.set(false);
        self.data.depth.set(0);
        *self.data.requirer.borrow_mut() = None;
        *self.data.requiring.borrow_mut() = attributed;
        result
    }

    pub(super) fn clist_type(&self, row: Ty, at: &Token) -> Result<Ty> {
        if !self.data.shapes_only {
            if matches!(&row, Ty::Class(class) if class.try_get().is_none()) {
                self.data
                    .pending_clists
                    .borrow_mut()
                    .push((row.clone(), at.clone()));
            } else {
                super::clists::check_row(&row, at)?;
            }
        }
        Ok(Ty::Clist(Arc::new(row)))
    }

    fn resolve_enum(&self, declaration: &enums::EnumDecl) -> Result<Ty> {
        let variants = declaration
            .variants
            .iter()
            .map(|(name, fields)| {
                let fields = fields
                    .iter()
                    .map(|t| Ok(t.resolve(self)?.unwrap_or(Ty::Unit)))
                    .collect::<Result<Vec<_>>>()?;
                if let Some(barrier) = fields.iter().find_map(Ty::storage_barrier) {
                    return Err(declaration.at.error(barrier.message("enum payloads")));
                }
                Ok(crate::sum::Variant {
                    name: name.clone(),
                    fields,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let depth = 1 + variants
            .iter()
            .flat_map(|v| &v.fields)
            .map(Ty::layout_depth)
            .max()
            .unwrap_or(0);
        if depth > 64 {
            return Err(declaration
                .at
                .error("type nesting exceeds the implementation limit of 64"));
        }
        Ok(Ty::Enum(crate::nominal::Nominal::new(
            crate::sum::EnumType {
                name: declaration.name.clone(),
                facts: std::sync::OnceLock::new(),
                variants,
                restricted_storage: false,
                managed: true,
                inline_range: false,
                payload_bytes: std::sync::OnceLock::new(),
                by_name: std::sync::OnceLock::new(),
                storage: std::sync::OnceLock::new(),
                managed_fields: std::sync::OnceLock::new(),
            },
        )))
    }
}

/// Formats `name[arg, ...]`, or `name` alone for a non-generic type. Callers pick
/// the mapper: `Ty::identity_name` for the identity key, `Display` for messages.
fn instance_name(name: &str, args: &[Ty], f: impl Fn(&Ty) -> String) -> String {
    if args.is_empty() {
        return name.to_string();
    }
    let args = args.iter().map(f).collect::<Vec<_>>().join(", ");
    format!("{name}[{args}]")
}

fn substitute_method(method: &mut Function, substitutions: &generics::Substitution) -> Result<()> {
    for bound in method
        .type_params
        .iter_mut()
        .filter_map(|(_, b)| b.as_mut())
    {
        generics::substitute(bound, substitutions)?;
    }
    for (_, ty) in &mut method.inputs {
        generics::substitute(ty, substitutions)?;
    }
    generics::substitute(&mut method.output, substitutions)?;
    for default in method.defaults.iter_mut().flatten() {
        generics::substitute_expr(default, substitutions)?;
    }
    generics::substitute_block(&mut method.body, substitutions)
}

fn validate_parameters(name: &str, params: &[(String, Option<TypeRef>)], at: &Token) -> Result<()> {
    if params
        .iter()
        .any(|(n, _)| n == name.rsplit('.').next().unwrap())
    {
        return Err(at.error("a data parameter cannot shadow its declaration name"));
    }
    for bound in params.iter().filter_map(|(_, b)| b.as_ref()) {
        if !(generics::numeric_bound(bound).is_some() && bound.args.is_empty()
            || generics::callable_bound(bound))
        {
            return Err(bound.at.error(
                "generic data constraints currently require IntType, FloatType, Callable, or OnceCallable",
            ));
        }
    }
    Ok(())
}

impl generics::Engine {
    #[cfg_attr(feature = "profile", hotpath::measure(label = "generics::expand_data"))]
    pub(super) fn expand_data(
        &mut self,
        aliases: &TypeAliases,
        table: &mut functions::Functions,
    ) -> Result<Vec<Function>> {
        let pending = std::mem::take(&mut *aliases.data.pending.borrow_mut());
        let mut functions = Vec::new();
        let pending_enums = std::mem::take(&mut *aliases.data.pending_enums.borrow_mut());
        for declaration in pending_enums {
            let required = aliases
                .data
                .required
                .borrow()
                .get(&declaration.name)
                .cloned();
            let methods = enums::expand(vec![declaration], aliases)?;
            if let Some(required) = required {
                for method in &methods {
                    self.required
                        .insert(table.intern(&method.name), required.clone());
                }
            }
            functions.extend(methods);
        }
        for class in pending {
            let required = aliases.data.required.borrow().get(&class.name).cloned();
            let methods = classes::expand(vec![class], aliases)?;
            if let Some(required) = required {
                for method in &methods {
                    self.required
                        .insert(table.intern(&method.name), required.clone());
                }
            }
            functions.extend(methods);
        }
        for f in &functions {
            if !f.type_params.is_empty() {
                self.validate_template(f, aliases)?;
                self.templates.insert(f.name.clone(), Arc::new(f.clone()));
                self.specializable += 1;
                continue;
            }
            let signature = protocols::signature(f, aliases)?;
            if crate::record::is_method(&f.name) {
                self.methods.insert(f.name.clone(), signature.clone());
            }
            let conditional = f.name.ends_with(".format")
                && signature.0.first().is_some_and(|receiver| {
                    matches!(receiver, Ty::Ref(owner, false)
                        if aliases.data.report_context(owner, &self.standard_sources).is_some())
                });
            if conditional {
                self.defer_reporting_method(f.clone(), table);
            } else if f.name.ends_with(".__del__") {
                // Constructing an instance arms its destructor, so a body that
                // the type arguments do not support is rejected with the type.
                self.declare(f.clone(), table);
            } else {
                let (owner, method) = crate::record::method_display(&f.name)
                    .rsplit_once('.')
                    .expect("a data method is named by its owner");
                let display = match aliases.data.lookup(owner) {
                    Some(owner) => format!("{owner}.{method}").into(),
                    None => crate::record::method_display(&f.name).into(),
                };
                self.defer_method(f.clone(), &signature, display, table);
            }
        }
        Ok(functions
            .into_iter()
            .filter(|f| f.type_params.is_empty())
            .collect())
    }
}
impl Lower<'_> {
    pub(super) fn sync_data(&mut self) -> Result<()> {
        loop {
            let functions = self.generics.expand_data(self.aliases, self.sigs)?;
            if functions.is_empty() {
                break;
            }
            for f in functions {
                register_signature(&f, self.aliases, self.sigs)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_data_method_calls_share_native_specializations() {
        let mut source = String::from(
            "class Cell[T]:\n    value: T\n    def keep[U](self, value: U) -> U:\n        value\ntype Byte = u8\ntype ByteCell = Cell[Byte]\ndef main() -> ():\n    cell = ByteCell(7)\n",
        );
        for _ in 0..100 {
            source.push_str("    cell.keep(1u8)\n    cell.keep[u8](1)\n    cell.keep[Byte](1)\n");
        }
        source.push_str("    pass\n");
        let start = std::time::Instant::now();
        let program = compile(&source, &mut Heap::default()).unwrap();
        let names: Vec<_> = program
            .ops
            .iter()
            .filter_map(|op| {
                if let Op::DefineFn(name, _) = op {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            names
                .iter()
                .filter(|name| name.starts_with("__plenty_generic_"))
                .count(),
            1
        );
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "__plenty_class_Cell[u8].new")
                .count(),
            1
        );
        eprintln!(
            "300 generic method calls: {} native functions, frontend {:?}",
            names.len(),
            start.elapsed()
        );
    }
}
