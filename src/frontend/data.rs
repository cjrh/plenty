//! Demand-driven concrete data types. Specializations share nominal identities.
use super::*;
use std::cell::{Cell, RefCell};
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
    definitions: Rc<crate::nominal::Types>,
    pending_types: RefCell<std::collections::VecDeque<DefinitionJob>>,
    deferred: Cell<bool>,
    depth: Cell<usize>,
}

struct DefinitionJob {
    name: String,
    actual: Vec<Ty>,
    at: Token,
    depth: usize,
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
    /// Shape checks must not enqueue placeholder data instances in the real program.
    pub(super) fn validation(&self) -> Self {
        Self {
            named: self.named.clone(),
            data: Rc::new(DataTypes {
                enums: self.data.enums.clone(),
                classes: self.data.classes.clone(),
                instances: self.data.instances.clone(),
                named: self.data.named.clone(),
                arguments: self.data.arguments.clone(),
                ..DataTypes::default()
            }),
        }
    }
    pub(super) fn with_data(
        enums: &[enums::EnumDecl],
        classes: &[classes::ClassDecl],
    ) -> Result<Self> {
        let mut data = DataTypes::default();
        data.deferred.set(true);
        for e in enums {
            validate_parameters(&e.name, &e.type_params, &e.at)?;
            data.enums.insert(e.name.clone(), e.clone());
        }
        for c in classes {
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
                if !ty.heap_storable() {
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
        if !actual.is_empty()
            && self
                .data
                .instances
                .borrow()
                .keys()
                .filter(|(_, args)| !args.is_empty())
                .count()
                >= 256
        {
            return Err(application
                .at
                .error("generic data specialization limit of 256 exceeded"));
        }
        if self.data.depth.get() >= 64 {
            return Err(application.at.error("recursive generic data type or type nesting exceeds the implementation limit of 64"));
        }
        let concrete_name = if actual.is_empty() {
            name.clone()
        } else {
            format!(
                "{name}[{}]",
                actual
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        if concrete_name.len() > 16_384 {
            return Err(application
                .at
                .error("concrete type name exceeds the implementation limit"));
        }
        let ty = if self.data.enums.contains_key(name) {
            Ty::Enum(self.data.definitions.enumeration(&concrete_name))
        } else {
            Ty::Class(self.data.definitions.class(&concrete_name))
        };
        self.data
            .arguments
            .borrow_mut()
            .insert(concrete_name.clone(), key.clone());
        self.data.instances.borrow_mut().insert(key, ty.clone());
        self.data
            .named
            .borrow_mut()
            .insert(concrete_name, ty.clone());
        self.data
            .pending_types
            .borrow_mut()
            .push_back(DefinitionJob {
                name: name.clone(),
                actual,
                at: application.at.clone(),
                depth: self.data.depth.get() + 1,
            });
        if !self.data.deferred.get() {
            self.finish_types()?;
        }
        Ok(ty)
    }

    pub(super) fn finish_types(&self) -> Result<()> {
        self.data.deferred.set(true);
        let result = (|| {
            loop {
                let next = self.data.pending_types.borrow_mut().pop_front();
                let Some(DefinitionJob {
                    name,
                    actual,
                    at,
                    depth,
                }) = next
                else {
                    break;
                };
                self.data.depth.set(depth);
                let ty = self.data.instances.borrow()[&(name.clone(), actual.clone())].clone();
                let concrete_name = ty.to_string();
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
                let result: Result<(Ty, Option<classes::ClassDecl>)> = (|| {
                    if let Some(template) = self.data.enums.get(&name) {
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
                        let mut declaration = self.data.classes[&name].clone();
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
                let (definition, class): (Ty, Option<classes::ClassDecl>) = result?;
                match (&ty, definition) {
                    (Ty::Class(t), Ty::Class(value)) => t.define((*value.get()).clone()),
                    (Ty::Enum(t), Ty::Enum(value)) => t.define((*value.get()).clone()),
                    _ => unreachable!(),
                }
                if let Some(class) = class.filter(|_| !params.is_empty()) {
                    self.data.pending.borrow_mut().push(class);
                }
            }
            for ((name, _), ty) in self.data.instances.borrow().iter() {
                if ty.layout_depth() > 64 {
                    let at = self
                        .data
                        .enums
                        .get(name)
                        .map(|d| &d.at)
                        .unwrap_or_else(|| &self.data.classes[name].at);
                    return Err(at.error("type nesting exceeds the implementation limit of 64"));
                }
            }
            Ok(())
        })();
        self.data.deferred.set(false);
        self.data.depth.set(0);
        result
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
                if fields.iter().any(|t| !t.heap_storable()) {
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
        Ok(Ty::Enum(crate::nominal::Nominal::new(
            crate::sum::EnumType {
                name: declaration.name.clone(),
                facts: std::cell::OnceCell::new(),
                variants,
                restricted_storage: false,
                managed: true,
                inline_range: false,
                payload_bytes: std::cell::OnceCell::new(),
            },
        )))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_data_method_calls_share_native_specializations() {
        let mut source = String::from("class Cell[T]:\n    value: T\n    def keep[U](self, value: U) -> U:\n        value\ntype Byte = u8\ntype ByteCell = Cell[Byte]\ndef main() -> ():\n    cell = ByteCell(7).unwrap()\n");
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
