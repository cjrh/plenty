//! The module as seen by a thread building one function's IR.
//!
//! Functions are built on several threads, but the object's symbol table must
//! grow in the same order for every thread count. A [`Symbols`] therefore
//! answers from a snapshot of the declarations made before any body is built,
//! and turns each symbol a body would have declared (type metadata, the
//! function's trap-site table, a runtime import) into a numbered request. Its IR names the request
//! in a namespace of its own. The thread placing compiled code in the object
//! declares the requests in function order and rewrites the code's relocations
//! to the real symbols ([`resolve`]).
use super::*;
use cranelift_codegen::ir::{self, GlobalValue};
use cranelift_codegen::isa::{OwnedTargetIsa, TargetFrontendConfig};
use cranelift_module::{ModuleReloc, ModuleRelocTarget};

/// Namespace of requested symbols in IR. Cranelift uses 0 for functions and 1
/// for data.
const REQUESTED: u32 = 2;

/// Requested functions and metadata are handed out as identifiers from here
/// up, far above any real declaration, so lowering code keeps passing plain
/// [`FuncId`]s and [`DataId`]s.
const FIRST_REQUEST: u32 = 1 << 31;

/// What [`Symbols`] needs from the declarations made before bodies are built.
pub(super) struct Declared {
    isa: OwnedTargetIsa,
    /// Signature and colocation by [`FuncId`].
    functions: Vec<(ir::Signature, bool)>,
    /// Colocation and thread-locality by [`DataId`].
    data: Vec<(bool, bool)>,
    names: HashMap<String, FuncId>,
}

impl Declared {
    pub(super) fn snapshot(module: &ObjectModule, isa: OwnedTargetIsa) -> Self {
        let declarations = module.declarations();
        let mut names = HashMap::new();
        let functions = declarations
            .get_functions()
            .map(|(id, declaration)| {
                if let Some(name) = &declaration.name {
                    names.insert(name.clone(), id);
                }
                (
                    declaration.signature.clone(),
                    declaration.linkage.is_final(),
                )
            })
            .collect();
        let data = declarations
            .get_data_objects()
            .map(|(_, declaration)| (declaration.linkage.is_final(), declaration.tls))
            .collect();
        Self {
            isa,
            functions,
            data,
            names,
        }
    }
}

/// A symbol declared on first use while lowering a body.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum Request {
    /// Type metadata, as [`metadata::declare`] defines it.
    Metadata(Ty),
    /// The function's trap-site table, holding these records.
    Sites(Vec<sites::Key>),
    /// An imported function.
    Import(String, ir::Signature),
    /// The function's coverage counters, one per block ([`coverage`]).
    Coverage(u32),
}

/// The module interface used while building one function's IR.
pub(super) struct Symbols {
    declared: Arc<Declared>,
    requests: Vec<Request>,
    numbers: HashMap<Request, u32>,
    /// The trap-site table's request number and global value, once used.
    site_table: Option<(u32, GlobalValue)>,
    sites: sites::Sites,
}

impl Symbols {
    pub(super) fn new(declared: Arc<Declared>) -> Self {
        Self {
            declared,
            requests: Vec::new(),
            numbers: HashMap::new(),
            site_table: None,
            sites: sites::Sites::default(),
        }
    }

    pub(super) fn into_requests(mut self) -> Vec<Request> {
        if let Some((number, _)) = self.site_table {
            self.requests[number as usize] = Request::Sites(self.sites.into_records());
        }
        self.requests
    }

    pub(super) fn make_signature(&self) -> ir::Signature {
        ir::Signature::new(self.declared.isa.default_call_conv())
    }

    pub(super) fn target_config(&self) -> TargetFrontendConfig {
        self.declared.isa.frontend_config()
    }

    fn request(&mut self, request: Request) -> u32 {
        if let Some(number) = self.numbers.get(&request) {
            return *number;
        }
        let number = self.requests.len() as u32;
        self.numbers.insert(request.clone(), number);
        self.requests.push(request);
        number
    }

    /// Only imports are declared while lowering. A name already declared
    /// keeps its declaration; the placing thread still checks the signature.
    pub(super) fn declare_function(
        &mut self,
        name: &str,
        linkage: Linkage,
        signature: &ir::Signature,
    ) -> Result<FuncId> {
        if linkage != Linkage::Import {
            return Err(format!("`{name}` must be declared before function bodies").into());
        }
        let number = self.request(Request::Import(name.to_string(), signature.clone()));
        Ok(FuncId::from_u32(FIRST_REQUEST + number))
    }

    /// Type metadata for `ty`.
    pub(super) fn metadata(&mut self, ty: &Ty) -> DataId {
        DataId::from_u32(FIRST_REQUEST + self.request(Request::Metadata(ty.clone())))
    }

    pub(super) fn declare_func_in_func(
        &mut self,
        id: FuncId,
        func: &mut ir::Function,
    ) -> ir::FuncRef {
        let (name, signature, colocated) = match id.as_u32().checked_sub(FIRST_REQUEST) {
            None => {
                let (signature, colocated) = &self.declared.functions[id.as_u32() as usize];
                (
                    ir::UserExternalName::new(0, id.as_u32()),
                    signature.clone(),
                    *colocated,
                )
            }
            Some(number) => {
                let Request::Import(name, signature) = &self.requests[number as usize] else {
                    unreachable!("function request {number} is not an import")
                };
                // The same test the module applies to the final linkage.
                let colocated = self
                    .declared
                    .names
                    .get(name)
                    .is_some_and(|id| self.declared.functions[id.as_u32() as usize].1);
                (
                    ir::UserExternalName::new(REQUESTED, number),
                    signature.clone(),
                    colocated,
                )
            }
        };
        let signature = func.import_signature(signature);
        let name = func.declare_imported_user_function(name);
        func.import_function(ir::ExtFuncData {
            name: ir::ExternalName::user(name),
            signature,
            colocated,
            patchable: false,
        })
    }

    pub(super) fn declare_data_in_func(&self, id: DataId, func: &mut ir::Function) -> GlobalValue {
        let (name, colocated, tls) = match id.as_u32().checked_sub(FIRST_REQUEST) {
            None => {
                let (colocated, tls) = self.declared.data[id.as_u32() as usize];
                (ir::UserExternalName::new(1, id.as_u32()), colocated, tls)
            }
            // Metadata is local to the object.
            Some(number) => (ir::UserExternalName::new(REQUESTED, number), true, false),
        };
        symbol(func, name, colocated, tls)
    }

    /// The function's `counters` coverage counters.
    pub(super) fn coverage(&mut self, counters: u32, func: &mut ir::Function) -> GlobalValue {
        let number = self.request(Request::Coverage(counters));
        symbol(
            func,
            ir::UserExternalName::new(REQUESTED, number),
            true,
            false,
        )
    }

    /// The function's trap-site table and the offset of `key`'s record in it.
    pub(super) fn site(&mut self, key: sites::Key, func: &mut ir::Function) -> (GlobalValue, i64) {
        let table = match self.site_table {
            Some((_, table)) => table,
            None => {
                // Filled in by `into_requests`, once every record is known.
                let number = self.requests.len() as u32;
                self.requests.push(Request::Sites(Vec::new()));
                let table = symbol(
                    func,
                    ir::UserExternalName::new(REQUESTED, number),
                    true,
                    false,
                );
                self.site_table = Some((number, table));
                table
            }
        };
        (table, self.sites.record(key))
    }
}

fn symbol(
    func: &mut ir::Function,
    name: ir::UserExternalName,
    colocated: bool,
    tls: bool,
) -> GlobalValue {
    let name = func.declare_imported_user_function(name);
    func.create_global_value(ir::GlobalValueData::Symbol {
        name: ir::ExternalName::user(name),
        offset: ir::immediates::Imm64::new(0),
        colocated,
        tls,
    })
}

/// Declare `requests` of `function` in `module` and point `relocs` at the
/// real symbols.
pub(super) fn resolve(
    module: &mut ObjectModule,
    runtime: &Runtime,
    function: FuncId,
    requests: &[Request],
    relocs: &mut [ModuleReloc],
) -> Result<()> {
    if requests.is_empty() {
        return Ok(());
    }
    let mut targets = Vec::with_capacity(requests.len());
    for request in requests {
        targets.push(match request {
            Request::Metadata(ty) => {
                let id = metadata::declare(module, runtime, ty)?;
                ModuleRelocTarget::user(1, id.as_u32())
            }
            Request::Sites(records) => {
                let id = sites::define(module, records)?;
                ModuleRelocTarget::user(1, id.as_u32())
            }
            Request::Import(name, signature) => {
                let id = module.declare_function(name, Linkage::Import, signature)?;
                ModuleRelocTarget::user(0, id.as_u32())
            }
            Request::Coverage(counters) => {
                let id = coverage::define(module, function, *counters)?;
                ModuleRelocTarget::user(1, id.as_u32())
            }
        });
    }
    for reloc in relocs {
        if let ModuleRelocTarget::User { namespace, index } = reloc.name {
            if namespace == REQUESTED {
                reloc.name = targets[index as usize].clone();
            }
        }
    }
    Ok(())
}
