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
    instances: RefCell<HashMap<(String, Vec<Ty>), Ty>>,
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
    pub(super) fn contains(&self, name: &str) -> bool {
        self.enums.contains_key(name)
    }
}
impl TypeAliases {
    pub(super) fn with_enums(enums: &[enums::EnumDecl]) -> Result<Self> {
        let mut data = DataTypes::default();
        for e in enums.iter().filter(|e| !e.type_params.is_empty()) {
            if e.type_params.iter().any(|(_, b)| b.is_some()) {
                return Err(e.at.error("generic data type bounds are not supported yet"));
            }
            data.enums.insert(e.name.clone(), e.clone());
        }
        Ok(Self {
            named: HashMap::new(),
            data: Rc::new(data),
        })
    }

    pub(super) fn instantiate(&self, application: &TypeRef) -> Result<Ty> {
        let name = application.name.as_ref().unwrap();
        let template = &self.data.enums[name];
        if application.args.len() != template.type_params.len() {
            return Err(application.at.error(format!(
                "generic type `{name}` requires {} type arguments",
                template.type_params.len()
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
        let substitutions = template
            .type_params
            .iter()
            .map(|(n, _)| n.clone())
            .zip(
                actual
                    .iter()
                    .map(|t| generics::type_ref(t, &application.at)),
            )
            .collect();
        let mut declaration = template.clone();
        declaration.name = concrete_name;
        declaration.type_params.clear();
        for (_, fields) in &mut declaration.variants {
            for field in fields {
                generics::substitute(field, &substitutions)?;
            }
        }
        self.data.active.borrow_mut().push(name.clone());
        let result = self.resolve_enum(&declaration);
        self.data.active.borrow_mut().pop();
        let ty = result?;
        self.data.instances.borrow_mut().insert(key, ty.clone());
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
