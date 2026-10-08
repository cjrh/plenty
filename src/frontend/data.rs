//! Demand-driven concrete data types. Specializations share nominal identities.
use super::*;
use std::cell::RefCell;
use std::ops::{Deref, DerefMut};

#[derive(Clone, Default)]
pub(crate) struct TypeAliases {
    named: HashMap<String, Type>,
    pub(super) data: Rc<DataTypes>,
}

#[derive(Default)]
pub(super) struct DataTypes {
    enums: HashMap<String, enums::EnumDecl>,
    classes: HashMap<String, classes::ClassDecl>,
    instances: RefCell<HashMap<(String, Vec<Ty>), Ty>>,
    named: RefCell<HashMap<String, Ty>>,
    arguments: RefCell<HashMap<String, (String, Vec<Ty>)>>,
    pending: RefCell<Vec<classes::ClassDecl>>,
    active: RefCell<Vec<String>>,
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
        self.arguments.borrow().get(name).cloned()
    }

    pub(super) fn factories(&self) -> Vec<Function> {
        let mut classes: Vec<_> = self.classes.values().collect();
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
                    name: class.name.clone(),
                    type_params: class.type_params.clone(),
                    at: at.clone(),
                    inputs,
                    output: reference(
                        "Result".into(),
                        vec![
                            reference(class.name.clone(), types.clone()),
                            reference("AllocError".into(), vec![]),
                        ],
                    ),
                    doc: String::new(),
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
    pub(super) fn with_data(
        enums: &[enums::EnumDecl],
        classes: &[classes::ClassDecl],
    ) -> Result<Self> {
        let mut data = DataTypes::default();
        for e in enums.iter().filter(|e| !e.type_params.is_empty()) {
            validate_parameters(&e.name, &e.type_params, &e.at)?;
            data.enums.insert(e.name.clone(), e.clone());
        }
        for c in classes.iter().filter(|c| !c.type_params.is_empty()) {
            validate_parameters(&c.name, &c.type_params, &c.at)?;
            data.classes.insert(c.name.clone(), c.clone());
        }
        Ok(Self {
            named: HashMap::new(),
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
                if ty.restricted_storage() {
                    return Err(t.at.error(
                        "generic data arguments cannot contain references, generators, or closures",
                    ));
                }
                Ok(ty)
            })
            .collect::<Result<Vec<_>>>()?;
        let key = (name.clone(), actual.clone());
        for ((_, bound), ty) in params.iter().zip(&actual) {
            if bound.is_some() && !ty.is_int() {
                return Err(application
                    .at
                    .error(format!("{ty} does not satisfy IntType")));
            }
        }
        if let Some(ty) = self.data.instances.borrow().get(&key) {
            return Ok(ty.clone());
        }
        if self.data.instances.borrow().len() + self.data.active.borrow().len() >= 256 {
            return Err(application
                .at
                .error("generic data specialization limit of 256 exceeded"));
        }
        if self.data.active.borrow().len() >= 64 || self.data.active.borrow().contains(name) {
            return Err(application.at.error("recursive generic data type or type nesting exceeds the implementation limit of 64"));
        }
        let concrete_name = format!(
            "{name}[{}]",
            actual
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        if concrete_name.len() > 16_384 {
            return Err(application
                .at
                .error("concrete type name exceeds the implementation limit"));
        }
        let substitutions = params
            .iter()
            .map(|(n, _)| n.clone())
            .zip(
                actual
                    .iter()
                    .map(|t| generics::type_ref(t, &application.at)),
            )
            .collect();
        self.data.active.borrow_mut().push(name.clone());
        let result: Result<(Ty, Option<classes::ClassDecl>)> = (|| {
            if let Some(template) = self.data.enums.get(name) {
                let mut declaration = template.clone();
                declaration.name = concrete_name.clone();
                declaration.type_params.clear();
                for (_, fields) in &mut declaration.variants {
                    for field in fields {
                        generics::substitute(field, &substitutions)?;
                    }
                }
                Ok((self.resolve_enum(&declaration)?, None))
            } else {
                let mut declaration = self.data.classes[name].clone();
                declaration.name = concrete_name.clone();
                declaration.type_params.clear();
                for (_, field) in &mut declaration.fields {
                    generics::substitute(field, &substitutions)?;
                }
                for method in &mut declaration.methods {
                    for bound in method
                        .type_params
                        .iter_mut()
                        .filter_map(|(_, b)| b.as_mut())
                    {
                        generics::substitute(bound, &substitutions)?;
                    }
                    for (_, ty) in &mut method.inputs {
                        generics::substitute(ty, &substitutions)?;
                    }
                    generics::substitute(&mut method.output, &substitutions)?;
                    generics::substitute_block(&mut method.body, &substitutions)?;
                }
                Ok((declaration.resolve(self)?, Some(declaration)))
            }
        })();
        self.data.active.borrow_mut().pop();
        let (ty, class): (Ty, Option<classes::ClassDecl>) = result?;
        self.data
            .arguments
            .borrow_mut()
            .insert(concrete_name.clone(), key.clone());
        self.data.instances.borrow_mut().insert(key, ty.clone());
        self.data
            .named
            .borrow_mut()
            .insert(concrete_name, ty.clone());
        if let Some(class) = class {
            self.data.pending.borrow_mut().push(class);
        }
        Ok(ty)
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
                if fields.iter().any(Ty::restricted_storage) {
                    return Err(declaration.at.error(
                        "references, generators, and closures cannot be stored in enum payloads",
                    ));
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
        Ok(Ty::Enum(Rc::new(crate::sum::EnumType {
            name: declaration.name.clone(),
            depth,
            affine: variants.iter().flat_map(|v| &v.fields).any(Ty::affine),
            copyable: variants.iter().flat_map(|v| &v.fields).all(Ty::can_copy),
            has_destructor: variants
                .iter()
                .flat_map(|v| &v.fields)
                .any(Ty::has_destructor),
            variants,
            restricted_storage: false,
            managed: true,
            inline_range: false,
            payload_bytes: std::cell::OnceCell::new(),
        })))
    }
}

fn validate_parameters(name: &str, params: &[(String, Option<TypeRef>)], at: &Token) -> Result<()> {
    if params
        .iter()
        .any(|(n, _)| n == name.rsplit('.').next().unwrap())
    {
        return Err(at.error("a data parameter cannot shadow its declaration name"));
    }
    for bound in params.iter().filter_map(|(_, b)| b.as_ref()) {
        if bound.name.as_deref() != Some("IntType") || !bound.args.is_empty() {
            return Err(bound
                .at
                .error("generic data constraints currently require IntType"));
        }
    }
    Ok(())
}

impl generics::Engine {
    pub(super) fn expand_data(&mut self, aliases: &TypeAliases) -> Result<Vec<Function>> {
        let pending = std::mem::take(&mut *aliases.data.pending.borrow_mut());
        let functions = classes::expand(pending, aliases)?;
        for f in &functions {
            if !f.type_params.is_empty() {
                self.validate_template(f, aliases)?;
                self.templates.insert(f.name.clone(), Rc::new(f.clone()));
                continue;
            }
            self.methods
                .insert(f.name.clone(), protocols::signature(f, aliases)?);
            self.functions.insert(f.name.clone(), f.clone());
            self.pending.push_back(f.clone());
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
            let functions = self.generics.expand_data(self.aliases)?;
            if functions.is_empty() {
                break;
            }
            for f in functions {
                register_signature(&f, self.aliases, self.sigs, self.returned_fields)?;
            }
        }
        Ok(())
    }
}
