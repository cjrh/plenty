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
    /// Record each operation's source line in the IR, for `--dump`. Off
    /// otherwise: source locations are part of the compiled-function cache key.
    pub(super) lines: bool,
}

impl Declared {
    pub(super) fn snapshot(module: &ObjectModule, isa: OwnedTargetIsa) -> Self {
        let declarations = module.declarations();
        let mut names = HashMap::default();
        let functions = declarations
            .get_functions()
            .map(|(id, declaration)| {
                if let Some(name) = &declaration.name {
                    names.insert(name.clone(), id);
                }
                (
                    declaration.signature.clone(),
                    runtime_colocated(declaration.linkage),
                )
            })
            .collect();
        let data = declarations
            .get_data_objects()
            .map(|(_, declaration)| (data_colocated(declaration.linkage), declaration.tls))
            .collect();
        Self {
            isa,
            functions,
            data,
            names,
            lines: false,
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
    /// A runtime helper.
    Import(String, ir::Signature),
    /// A C function from a native library ([`super::foreign`]).
    Foreign(String, ir::Signature),
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
    checks: traps::Checks,
}

impl Symbols {
    pub(super) fn new(declared: Arc<Declared>) -> Self {
        Self {
            declared,
            requests: Vec::new(),
            numbers: HashMap::default(),
            site_table: None,
            sites: sites::Sites::default(),
            checks: traps::Checks::default(),
        }
    }

    /// The requests, and the checks that trap through the runtime's handler.
    pub(super) fn finish(mut self) -> (Vec<Request>, traps::Checks) {
        if let Some((number, _)) = self.site_table {
            self.requests[number as usize] = Request::Sites(self.sites.into_records());
        }
        (self.requests, self.checks)
    }

    /// A trap code for a check of `kind` at the site record `site`
    /// ([`traps::Checks::code`]).
    pub(super) fn check_code(&mut self, kind: u32, site: Option<i64>) -> Option<ir::TrapCode> {
        self.checks.code(kind, site)
    }

    /// Whether to record source lines in the IR ([`Declared::lines`]).
    pub(super) fn lines(&self) -> bool {
        self.declared.lines
    }

    /// Whether Cranelift's optimizer runs on the bodies built here.
    pub(super) fn optimizes(&self) -> bool {
        self.declared.isa.flags().opt_level() != cranelift_codegen::settings::OptLevel::None
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

    /// Only runtime imports are declared while lowering; foreign functions
    /// use [`Self::declare_foreign`]. A name already declared keeps its
    /// declaration; the placing thread still checks the signature.
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

    /// Import the native library function `name`.
    pub(super) fn declare_foreign(&mut self, name: &str, signature: &ir::Signature) -> FuncId {
        let number = self.request(Request::Foreign(name.to_string(), signature.clone()));
        FuncId::from_u32(FIRST_REQUEST + number)
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
                let (signature, colocated) = match &self.requests[number as usize] {
                    Request::Import(_, signature) => (signature, true),
                    // A native library may be shared, so the call goes through
                    // the GOT unless the name is the program's own.
                    Request::Foreign(name, signature) => (
                        signature,
                        self.declared
                            .names
                            .get(name)
                            .is_some_and(|id| self.declared.functions[id.as_u32() as usize].1),
                    ),
                    _ => unreachable!("function request {number} is not an import"),
                };
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
                // Filled in by `finish`, once every record is known.
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

/// Whether calls to a function declared with `linkage` before bodies are
/// built are direct (`call rel32`, `R_X86_64_PLT32`) rather than through the
/// GOT. Every function imported then is a runtime helper. The runtime archive
/// is linked into every executable and shared library built from the object,
/// and a shared library's version script makes its symbols local, so the
/// linker resolves the call without a PLT entry. The module applies
/// [`Linkage::is_final`] alone, which treats the runtime like any shared
/// library (issue #372).
pub(super) fn runtime_colocated(linkage: Linkage) -> bool {
    linkage.is_final() || linkage == Linkage::Import
}

/// Whether data declared with `linkage` is addressed PC-relatively
/// ([`super::relax`]): only a definition no other module can preempt, since a
/// shared library cannot use `R_X86_64_PC32` against an exported symbol.
pub(super) fn data_colocated(linkage: Linkage) -> bool {
    matches!(linkage, Linkage::Local | Linkage::Hidden)
}

/// Apply [`runtime_colocated`] and [`data_colocated`] to `func`, built against
/// `module` on the placing thread, whose own declarations use
/// [`Linkage::is_final`].
pub(super) fn colocate(module: &ObjectModule, func: &mut ir::Function) {
    let declarations = module.declarations();
    let names = func.params.user_named_funcs().clone();
    let id = |name: &ir::ExternalName| match name {
        ir::ExternalName::User(name) => Some(&names[*name]),
        _ => None,
    };
    for data in func.dfg.ext_funcs.values_mut() {
        if let Some(name) = id(&data.name)
            && name.namespace == 0
        {
            let linkage = declarations
                .get_function_decl(FuncId::from_u32(name.index))
                .linkage;
            data.colocated = runtime_colocated(linkage);
        }
    }
    for data in func.global_values.values_mut() {
        if let ir::GlobalValueData::Symbol {
            name, colocated, ..
        } = data
            && let Some(name) = id(name)
            && name.namespace == 1
        {
            let linkage = declarations
                .get_data_decl(DataId::from_u32(name.index))
                .linkage;
            *colocated = data_colocated(linkage);
        }
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
/// real symbols. The function's trap-site records join the object's one
/// table (`sites`); their relocations gain the function's base offset in it.
pub(super) fn resolve(
    module: &mut ObjectModule,
    sites: &mut sites::Table,
    runtime: &Runtime,
    function: FuncId,
    requests: &[Request],
    relocs: &mut [ModuleReloc],
) -> Result<Vec<ModuleRelocTarget>> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let mut targets = Vec::with_capacity(requests.len());
    let mut addends = vec![0; requests.len()];
    for (number, request) in requests.iter().enumerate() {
        targets.push(match request {
            Request::Metadata(ty) => {
                let id = metadata::declare(module, runtime, ty)?;
                ModuleRelocTarget::user(1, id.as_u32())
            }
            Request::Sites(records) => {
                let (id, base) = sites.append(module, records)?;
                addends[number] = base;
                ModuleRelocTarget::user(1, id.as_u32())
            }
            Request::Import(name, signature) | Request::Foreign(name, signature) => {
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
        if let ModuleRelocTarget::User { namespace, index } = reloc.name
            && namespace == REQUESTED
        {
            reloc.name = targets[index as usize].clone();
            reloc.addend += addends[index as usize];
        }
    }
    Ok(targets)
}
