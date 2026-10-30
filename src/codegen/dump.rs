//! `plenty --dump`: each function's operations, Cranelift IR before and after
//! compilation, and machine code, as text for `grep` and `diff` (issue #360).
//! See book/src/design/toolchain/02-architecture/dump.md.
//!
//! Workers capture a selected function's IR while compiling it. The placing
//! thread renders the capture after resolving the symbols the body requested,
//! so every name in the text is the object's, and sections come out in
//! placement order whatever the worker count.
use std::borrow::Cow;
use std::fmt::{self, Write as _};

use cranelift_codegen::entity::SecondaryMap;
use cranelift_codegen::ir::entities::AnyEntity;
use cranelift_codegen::ir::{self, ExternalName, Function, GlobalValueData, UserExternalName};
use cranelift_codegen::write::{FuncWriter, PlainWriter, decorate_function, write_block_header};
use cranelift_module::{DataId, FuncId, Linkage, Module, ModuleReloc, ModuleRelocTarget};
use cranelift_object::ObjectModule;
use iced_x86::{Decoder, DecoderOptions, Formatter, GasFormatter, Instruction};

use super::UserFn;
use super::symbols::Request;
use crate::hash::HashMap;
use crate::op::{CompiledFn, Op};
use crate::value::Heap;
use crate::value::StrId;

/// One kind of text `--dump` writes, each to its own file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DumpStage {
    /// The typed operations the code generator lowers.
    Ops,
    /// Cranelift IR as the code generator built it.
    Clif,
    /// Cranelift IR after Cranelift's own passes: optimized with `--release`.
    ClifOpt,
    /// Machine code, with resolved symbol names.
    Asm,
}

impl DumpStage {
    pub const ALL: [DumpStage; 4] = [Self::Ops, Self::Clif, Self::ClifOpt, Self::Asm];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ops => "ops",
            Self::Clif => "clif",
            Self::ClifOpt => "clif-opt",
            Self::Asm => "asm",
        }
    }

    /// `all`, or a comma-separated list of stage names, in canonical order.
    pub fn parse_list(text: &str) -> std::result::Result<Vec<DumpStage>, String> {
        let mut stages = Vec::new();
        for word in text.split(',') {
            match word {
                "all" => stages.extend(Self::ALL),
                _ => stages.push(
                    Self::ALL
                        .into_iter()
                        .find(|stage| stage.name() == word)
                        .ok_or_else(|| {
                            format!(
                                "unknown dump stage `{word}`; expected `all` or a comma-separated \
                                 list of `ops`, `clif`, `clif-opt`, `asm`"
                            )
                        })?,
                ),
            }
        }
        stages.sort();
        stages.dedup();
        Ok(stages)
    }
}

/// What `--dump` writes.
#[derive(Clone, Debug, Default)]
pub struct DumpOptions {
    pub stages: Vec<DumpStage>,
    /// Include functions compiled from `std` modules.
    pub standard_library: bool,
    /// Keep only functions whose name or symbol contains this text.
    pub filter: Option<String>,
}

impl DumpOptions {
    fn has(&self, stage: DumpStage) -> bool {
        self.stages.contains(&stage)
    }
}

/// A finished dump: `index.txt`, then one file per requested stage.
pub struct Dump {
    pub files: Vec<(String, String)>,
}

/// What the dump knows about a function before it is built.
struct Subject {
    key: String,
    kind: &'static str,
    standard_library: bool,
    source: String,
    selected: bool,
    ops: Option<String>,
    op_count: Option<usize>,
}

/// The functions to capture, decided before any is built.
pub(super) struct Plan {
    options: DumpOptions,
    optimize: bool,
    subjects: HashMap<FuncId, Subject>,
    /// A preview of each string literal's text, by its data symbol.
    strings: HashMap<DataId, String>,
    /// Absolute source paths and their root-relative forms, longest first:
    /// type names can mention where a type was defined, and the dump shows
    /// such files as `source` does, so it does not depend on the checkout.
    paths: Vec<(String, String)>,
}

impl Plan {
    /// `text` with absolute source paths shown relative to the source root.
    fn relative(&self, mut text: String) -> String {
        for (absolute, relative) in &self.paths {
            if text.contains(absolute.as_str()) {
                text = text.replace(absolute.as_str(), relative);
            }
        }
        text
    }

    /// Describe every function declared so far. `ops` is the whole program,
    /// before top-level definitions are removed from it.
    pub(super) fn new(
        options: &DumpOptions,
        optimize: bool,
        ops: &[Op],
        user_fns: &HashMap<String, UserFn>,
        module: &ObjectModule,
        heap: &Heap,
        str_data: &super::StrData,
    ) -> Self {
        let mut plan = Self {
            options: options.clone(),
            optimize,
            subjects: HashMap::default(),
            strings: str_data
                .literals
                .iter()
                .map(|(id, data)| (*data, preview(heap, *id)))
                .collect(),
            paths: Vec::new(),
        };
        let mut defined = Vec::new();
        definitions(ops, &mut defined);
        let mut paths: Vec<(String, String)> = defined
            .iter()
            .filter_map(|(_, f)| {
                Some((
                    f.origins.source.clone()?.to_string(),
                    f.origins.trap_file.clone()?.to_string(),
                ))
            })
            .filter(|(absolute, relative)| absolute != relative)
            .collect();
        paths.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.cmp(b)));
        paths.dedup();
        plan.paths = paths;
        let mut keys = HashMap::default();
        for (name, f) in defined {
            let decl = &user_fns[name];
            let key = unique(&mut keys, plan.relative(display_name(name, f)));
            let source = source(f);
            let std = decl.standard_library;
            let ops = plan
                .matches(&key, &format!("__plenty_fn_{name}"), std)
                .then(|| render_function(f, heap));
            plan.add(decl.id, &key, "function", std, &source, module, ops);
            if let Some(id) = decl.drop_callback {
                plan.add(
                    id,
                    &format!("{key}@drop"),
                    "drop",
                    std,
                    &source,
                    module,
                    None,
                );
            }
            if let Some(id) = decl.resume {
                let key = format!("{key}@resume");
                plan.add(id, &key, "resume", std, &source, module, None);
            }
        }
        for (id, declaration) in module.declarations().get_functions() {
            if declaration.linkage == Linkage::Import || plan.subjects.contains_key(&id) {
                continue;
            }
            let symbol = declaration.linkage_name(id);
            let kind = if symbol == "plenty_main" {
                "entry"
            } else {
                "adapter"
            };
            plan.add(id, &symbol, kind, false, "-", module, None);
        }
        plan
    }

    fn matches(&self, key: &str, symbol: &str, standard_library: bool) -> bool {
        (self.options.standard_library || !standard_library)
            && self
                .options
                .filter
                .as_deref()
                .is_none_or(|text| key.contains(text) || symbol.contains(text))
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        id: FuncId,
        key: &str,
        kind: &'static str,
        standard_library: bool,
        source: &str,
        module: &ObjectModule,
        ops: Option<(String, usize)>,
    ) {
        let symbol = module.declarations().get_function_decl(id).linkage_name(id);
        let selected = self.matches(key, &symbol, standard_library);
        let op_count = ops.as_ref().map(|(_, count)| *count);
        let ops = ops
            .filter(|_| self.options.has(DumpStage::Ops))
            .map(|(text, _)| text);
        self.subjects.insert(
            id,
            Subject {
                key: key.to_string(),
                kind,
                standard_library,
                source: source.to_string(),
                selected,
                ops,
                op_count,
            },
        );
    }

    /// Record `plenty_main`'s operations: the program without its top-level
    /// definitions.
    pub(super) fn set_entry_ops(&mut self, id: FuncId, ops: &[Op], heap: &Heap) {
        if let Some(subject) = self.subjects.get_mut(&id)
            && subject.selected
        {
            let mut text = String::new();
            subject.op_count = Some(render_ops(ops, heap, 0, &mut text));
            if self.options.has(DumpStage::Ops) {
                subject.ops = Some(text);
            }
        }
    }

    /// Whether to capture `id`'s code. Functions declared after the plan,
    /// such as a library's exports, are judged by their symbol.
    pub(super) fn wants(&self, id: FuncId, module: Option<&ObjectModule>) -> bool {
        match (self.subjects.get(&id), module) {
            (Some(subject), _) => subject.selected,
            (None, Some(module)) => {
                let symbol = module.declarations().get_function_decl(id).linkage_name(id);
                self.matches(&symbol, &symbol, false)
            }
            (None, None) => false,
        }
    }

    /// Text for a placed function. `relocs` are resolved; `requested` pairs
    /// the body's requested symbols with what they became.
    pub(super) fn entry(
        &self,
        module: &ObjectModule,
        id: FuncId,
        capture: Capture,
        (bytes, relocs, traps): (&[u8], &[ModuleReloc], &[(u32, u8)]),
        (requests, targets, checks): (&[Request], &[ModuleRelocTarget], &super::traps::Checks),
    ) -> Entry {
        let symbol = module
            .declarations()
            .get_function_decl(id)
            .linkage_name(id)
            .into_owned();
        let (key, kind, standard_library, source, ops, op_count) = match self.subjects.get(&id) {
            Some(subject) => (
                subject.key.clone(),
                subject.kind,
                subject.standard_library,
                subject.source.clone(),
                subject.ops.clone(),
                subject.op_count,
            ),
            None => (symbol.clone(), "export", false, "-".to_string(), None, None),
        };
        let names = Names {
            module,
            plan: self,
            requests,
            targets,
            checks,
        };
        let clif = |func: &Function| {
            let mut text = String::new();
            decorate_function(&mut ClifWriter::new(&names), &mut text, func)
                .expect("writing to a string");
            text
        };
        let options = &self.options;
        Entry {
            key,
            symbol,
            kind,
            standard_library,
            source,
            ops,
            op_count,
            clif: options.has(DumpStage::Clif).then(|| clif(&capture.before)),
            clif_opt: options
                .has(DumpStage::ClifOpt)
                .then(|| capture.after.as_ref().map(clif))
                .flatten(),
            asm: options
                .has(DumpStage::Asm)
                .then(|| asm(bytes, (relocs, traps), &capture.srclocs, &names)),
            bytes: bytes.len(),
            instructions: capture.instructions,
            optimized_instructions: capture.after.as_ref().map(instructions).unwrap_or(0),
        }
    }

    /// The finished dump of `entries`, in placement order.
    pub(super) fn finish(&self, entries: Vec<Entry>) -> Dump {
        let options = &self.options;
        let mut files = Vec::new();
        let mut index = String::new();
        let _ = writeln!(index, "# plenty {} --dump", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(index, "# target {}", crate::native_target());
        let _ = writeln!(
            index,
            "# optimize {}",
            if self.optimize { "speed" } else { "none" }
        );
        let stages: Vec<_> = options.stages.iter().map(|stage| stage.name()).collect();
        let _ = writeln!(index, "# stages {}", stages.join(","));
        let _ = writeln!(
            index,
            "# scope {}",
            if options.standard_library {
                "user+std"
            } else {
                "user"
            }
        );
        if let Some(filter) = &options.filter {
            let _ = writeln!(index, "# filter {filter:?}");
        }
        let _ = writeln!(index, "# functions {}", entries.len());
        let _ = writeln!(index, "# sorted by machine-code bytes, largest first");
        let _ = writeln!(
            index,
            "#{:>7} {:>6} {:>6} {:>6} {:<8} {:<4} {:<32} symbol key",
            "bytes", "ops", "clif", "opt", "kind", "lib", "source"
        );
        let mut order: Vec<&Entry> = entries.iter().collect();
        // Stable: equal sizes keep placement order.
        order.sort_by_key(|entry| std::cmp::Reverse(entry.bytes));
        for entry in order {
            let _ = writeln!(
                index,
                "{:>8} {:>6} {:>6} {:>6} {:<8} {:<4} {:<32} {} {}",
                entry.bytes,
                entry
                    .op_count
                    .map_or_else(|| "-".to_string(), |count| count.to_string()),
                entry.instructions,
                entry.optimized_instructions,
                entry.kind,
                if entry.standard_library {
                    "std"
                } else {
                    "user"
                },
                entry.source,
                entry.symbol,
                entry.key
            );
        }
        files.push(("index.txt".to_string(), index));
        for stage in &options.stages {
            let mut text = String::new();
            for entry in &entries {
                let body = match stage {
                    DumpStage::Ops => entry.ops.as_deref(),
                    DumpStage::Clif => entry.clif.as_deref(),
                    DumpStage::ClifOpt => entry.clif_opt.as_deref(),
                    DumpStage::Asm => entry.asm.as_deref(),
                };
                if let Some(body) = body {
                    let _ = writeln!(
                        text,
                        "==== {}  symbol={}  kind={}  source={}",
                        entry.key, entry.symbol, entry.kind, entry.source
                    );
                    text.push_str(body);
                    text.push('\n');
                }
            }
            files.push((format!("{}.txt", stage.name()), text));
        }
        Dump { files }
    }
}

/// What a worker keeps of a selected function's compilation.
pub(super) struct Capture {
    pub before: Function,
    pub after: Option<Function>,
    /// Machine-code byte ranges and the source line each came from.
    pub srclocs: Vec<(u32, u32, u32)>,
    pub instructions: usize,
}

impl Capture {
    pub(super) fn new(func: &Function) -> Self {
        Self {
            before: func.clone(),
            after: None,
            srclocs: Vec::new(),
            instructions: instructions(func),
        }
    }
}

/// One placed function's dump.
pub(super) struct Entry {
    key: String,
    symbol: String,
    kind: &'static str,
    standard_library: bool,
    source: String,
    ops: Option<String>,
    op_count: Option<usize>,
    clif: Option<String>,
    clif_opt: Option<String>,
    asm: Option<String>,
    bytes: usize,
    instructions: usize,
    optimized_instructions: usize,
}

fn instructions(func: &Function) -> usize {
    func.layout
        .blocks()
        .map(|block| func.layout.block_insts(block).count())
        .sum()
}

/// Every `DefineFn` in `ops`, where [`super::collect_user_fns`] finds them.
fn definitions<'a>(ops: &'a [Op], out: &mut Vec<(&'a str, &'a CompiledFn)>) {
    for op in ops {
        match op {
            Op::DefineFn(name, f) => {
                out.push((name, f));
                definitions(&f.body, out);
            }
            Op::Match(arms) => {
                for arm in arms.iter() {
                    definitions(&arm.body, out);
                }
            }
            Op::CheckedBlock { body, .. } => definitions(body, out),

            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                definitions(then, out);
                for arm in elifs.iter() {
                    definitions(&arm.condition, out);
                    definitions(&arm.body, out);
                }
                definitions(otherwise, out);
            }
            _ => {}
        }
    }
}

/// The name a reader knows a function by: methods without the class prefix,
/// generic function instances by their instance. A generic class's methods
/// already carry the instance in their names.
fn display_name(name: &str, f: &CompiledFn) -> String {
    if name.starts_with("__plenty_generic_")
        && let Some(required) = &f.origins.required
    {
        return required.instance.to_string();
    }
    if let Some(rest) = name.strip_prefix("__plenty_closure_") {
        let mut parts = rest.rsplitn(3, '_');
        if let (Some(column), Some(line), Some(enclosing)) =
            (parts.next(), parts.next(), parts.next())
        {
            let enclosing = enclosing
                .strip_prefix("__plenty_class_")
                .unwrap_or(enclosing);
            return format!("{enclosing}.<closure@{line}:{column}>");
        }
    }
    name.strip_prefix("__plenty_class_")
        .unwrap_or(name)
        .to_string()
}

/// `key`, made unique by a `#N` suffix.
fn unique(seen: &mut HashMap<String, usize>, key: String) -> String {
    let count = seen.entry(key.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        key
    } else {
        format!("{key}#{count}")
    }
}

/// `file:line`, with the file relative to the source root so the dump does
/// not depend on where the tree is checked out.
fn source(f: &CompiledFn) -> String {
    let line = f.location.as_deref().and_then(|location| {
        let mut parts = location.rsplitn(3, ':');
        let _column = parts.next()?;
        let line = parts.next()?;
        line.parse::<u32>().ok()
    });
    match (&f.origins.trap_file, line) {
        (Some(file), Some(line)) => format!("{file}:{line}"),
        (Some(file), None) => file.to_string(),
        (None, _) => "-".to_string(),
    }
}

const PREVIEW_CHARS: usize = 40;

fn preview(heap: &Heap, id: StrId) -> String {
    let text = String::from_utf8_lossy(heap.data(id).0);
    let mut short: String = text.chars().take(PREVIEW_CHARS).collect();
    if text.chars().count() > PREVIEW_CHARS {
        short.push('…');
    }
    format!("{short:?}")
}

/// A function's local slots and then its operations.
fn render_function(f: &CompiledFn, heap: &Heap) -> (String, usize) {
    let mut text = String::new();
    for (slot, ty) in f.locals.iter().enumerate() {
        let _ = writeln!(text, "local {slot}: {ty}");
    }
    let count = render_ops(&f.body, heap, 0, &mut text);
    (text, count)
}

/// One operation per line, nested bodies indented under a label. Returns the
/// number of operations.
fn render_ops(ops: &[Op], heap: &Heap, depth: usize, out: &mut String) -> usize {
    let pad = |out: &mut String, depth: usize| {
        for _ in 0..depth {
            out.push_str("  ");
        }
    };
    let label = |out: &mut String, depth: usize, text: &str| {
        pad(out, depth);
        out.push_str(text);
        out.push('\n');
    };
    let mut count = ops.len();
    for op in ops {
        match op {
            Op::Loop { condition, body } => {
                label(out, depth, "Loop");
                label(out, depth + 1, "condition:");
                count += render_ops(condition, heap, depth + 2, out);
                label(out, depth + 1, "body:");
                count += render_ops(body, heap, depth + 2, out);
            }
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                label(out, depth, "If");
                label(out, depth + 1, "then:");
                count += render_ops(then, heap, depth + 2, out);
                for arm in elifs.iter() {
                    label(out, depth + 1, "elif condition:");
                    count += render_ops(&arm.condition, heap, depth + 2, out);
                    label(out, depth + 1, "elif body:");
                    count += render_ops(&arm.body, heap, depth + 2, out);
                }
                if !otherwise.is_empty() {
                    label(out, depth + 1, "else:");
                    count += render_ops(otherwise, heap, depth + 2, out);
                }
            }
            Op::Match(arms) => {
                label(out, depth, "Match");
                for arm in arms.iter() {
                    label(out, depth + 1, &format!("arm {:?}:", arm.pattern));
                    count += render_ops(&arm.body, heap, depth + 2, out);
                }
            }
            Op::Try {
                source,
                target,
                cleanup,
            } => {
                label(out, depth, &format!("Try {source:?} -> {target:?}"));
                if !cleanup.is_empty() {
                    label(out, depth + 1, "cleanup:");
                    count += render_ops(cleanup, heap, depth + 2, out);
                }
            }
            Op::CheckedBlock { body, result } => {
                label(out, depth, &format!("CheckedBlock -> {result:?}"));
                count += render_ops(body, heap, depth + 1, out);
            }
            Op::CheckedArith {
                operation,
                ty,
                cleanup,
            } => {
                label(out, depth, &format!("CheckedArith {operation:?} {ty:?}"));
                if !cleanup.is_empty() {
                    label(out, depth + 1, "cleanup:");
                    count += render_ops(cleanup, heap, depth + 2, out);
                }
            }
            // The body has its own section.
            Op::DefineFn(name, _) => label(out, depth, &format!("DefineFn {name}")),

            Op::Site(site) => label(
                out,
                depth,
                &format!(
                    "Site {}:{}{}",
                    site.line,
                    site.column,
                    if site.scope_end { " scope_end" } else { "" }
                ),
            ),
            Op::PushStr(id) => label(out, depth, &format!("PushStr {}", preview(heap, *id))),
            Op::PushBytes(id) => label(out, depth, &format!("PushBytes {}", preview(heap, *id))),
            _ => label(out, depth, &format!("{op:?}")),
        }
    }
    count
}

/// Resolves the external names in one placed function.
struct Names<'a> {
    module: &'a ObjectModule,
    plan: &'a Plan,
    requests: &'a [Request],
    targets: &'a [ModuleRelocTarget],
    checks: &'a super::traps::Checks,
}

impl Names<'_> {
    fn user(&self, name: &UserExternalName) -> String {
        match name.namespace {
            0 => self.function(FuncId::from_u32(name.index)).into_owned(),
            1 => self.data(DataId::from_u32(name.index)),
            _ => {
                let index = name.index as usize;
                let resolved = self
                    .targets
                    .get(index)
                    .map(|target| self.target(target))
                    .unwrap_or_else(|| format!("request {index}"));
                match self.requests.get(index) {
                    Some(Request::Metadata(ty)) => {
                        self.plan.relative(format!("{resolved} (metadata {ty})"))
                    }
                    Some(Request::Sites(records)) => {
                        format!("{resolved} (trap sites: {})", records.len())
                    }
                    Some(Request::Coverage(counters)) => {
                        format!("{resolved} (coverage counters: {counters})")
                    }
                    Some(Request::Import(..) | Request::Foreign(..)) | None => resolved,
                }
            }
        }
    }

    fn function(&self, id: FuncId) -> Cow<'_, str> {
        self.module
            .declarations()
            .get_function_decl(id)
            .linkage_name(id)
    }

    fn data(&self, id: DataId) -> String {
        let name = self
            .module
            .declarations()
            .get_data_decl(id)
            .linkage_name(id);
        match self.plan.strings.get(&id) {
            Some(text) => format!("{name} {text}"),
            None => name.into_owned(),
        }
    }

    fn target(&self, target: &ModuleRelocTarget) -> String {
        match target {
            ModuleRelocTarget::User { namespace, index } => self.user(&UserExternalName {
                namespace: *namespace,
                index: *index,
            }),
            ModuleRelocTarget::LibCall(call) => format!("%{call}"),
            ModuleRelocTarget::KnownSymbol(symbol) => format!("%{symbol}"),
            ModuleRelocTarget::FunctionOffset(id, offset) => {
                format!("{}+{offset}", self.function(*id))
            }
        }
    }

    /// The name of a relocation target, labelled with what a requested data
    /// symbol holds.
    fn reloc(&self, target: &ModuleRelocTarget) -> String {
        if let ModuleRelocTarget::User {
            namespace: 1,
            index,
        } = target
            && let Some(position) = self.targets.iter().position(|requested| {
                matches!(requested, ModuleRelocTarget::User { namespace: 1, index: i } if i == index)
            })
        {
            return self.user(&UserExternalName::new(2, position as u32));
        }
        self.target(target)
    }
}

/// CLIF text with names on external references and source lines as comments
/// in place of Cranelift's `@offset` prefixes.
struct ClifWriter<'a> {
    names: &'a Names<'a>,
    line: Option<u32>,
}

impl<'a> ClifWriter<'a> {
    fn new(names: &'a Names<'a>) -> Self {
        Self { names, line: None }
    }
}

impl FuncWriter for ClifWriter<'_> {
    fn write_block_header(
        &mut self,
        w: &mut dyn fmt::Write,
        func: &Function,
        block: ir::Block,
        _indent: usize,
    ) -> fmt::Result {
        write_block_header(w, func, block, 4)
    }

    fn write_instruction(
        &mut self,
        w: &mut dyn fmt::Write,
        func: &Function,
        aliases: &SecondaryMap<ir::Value, Vec<ir::Value>>,
        inst: ir::Inst,
        _indent: usize,
    ) -> fmt::Result {
        let srcloc = func.srcloc(inst);
        if !srcloc.is_default() && self.line != Some(srcloc.bits()) {
            self.line = Some(srcloc.bits());
            writeln!(w, "    ; line {}", srcloc.bits())?;
        }
        let mut text = String::new();
        PlainWriter.write_instruction(&mut text, func, aliases, inst, 4)?;
        for line in text.lines() {
            let line = line.trim_start();
            let line = match line.strip_prefix('@') {
                Some(rest) => rest.split_once(' ').map_or(rest, |(_, rest)| rest),
                None => line,
            };
            write!(w, "    {}", line.trim_start())?;
            if let ir::InstructionData::Trap { code, .. }
            | ir::InstructionData::CondTrap { code, .. } = func.dfg.insts[inst]
                && let Some(helper) = self.names.checks.helper(code.as_raw().get())
            {
                write!(w, "  ; {helper}")?;
            }
            writeln!(w)?;
        }
        Ok(())
    }

    fn write_entity_definition(
        &mut self,
        w: &mut dyn fmt::Write,
        func: &Function,
        entity: AnyEntity,
        value: &dyn fmt::Display,
    ) -> fmt::Result {
        let reference = match entity {
            AnyEntity::GlobalValue(gv) => match &func.global_values[gv] {
                GlobalValueData::Symbol {
                    name: ExternalName::User(reference),
                    ..
                } => Some(*reference),
                _ => None,
            },
            AnyEntity::FuncRef(f) => match &func.dfg.ext_funcs[f].name {
                ExternalName::User(reference) => Some(*reference),
                _ => None,
            },
            _ => None,
        };
        match reference {
            Some(reference) => {
                let name = self.names.user(&func.params.user_named_funcs()[reference]);
                writeln!(w, "    {entity} = {value} ; {name}")
            }
            None => writeln!(w, "    {entity} = {value}"),
        }
    }
}

/// Machine code as AT&T assembly: the byte offset of each instruction,
/// relocated operands' symbols as comments, and source lines where they
/// change. The constants Cranelift places after the code are shown as bytes.
/// `traps` are the `(offset, code)` of the checks' `ud2`s.
fn asm(
    bytes: &[u8],
    (relocs, traps): (&[ModuleReloc], &[(u32, u8)]),
    srclocs: &[(u32, u32, u32)],
    names: &Names,
) -> String {
    if names.module.isa().triple().architecture
        == target_lexicon::Architecture::Aarch64(target_lexicon::Aarch64Architecture::Aarch64)
    {
        return asm_arm64(bytes, relocs, traps, srclocs, names);
    }
    let code = &bytes[..code_length(bytes, relocs)];
    let mut decoder = Decoder::with_ip(64, code, 0, DecoderOptions::NONE);
    let mut formatter = GasFormatter::new();
    formatter.options_mut().set_first_operand_char_index(8);
    let mut out = String::new();
    let mut instruction = Instruction::default();
    let mut text = String::new();
    let mut line = None;
    let mut srcloc = srclocs.iter().peekable();
    // Zeros at the end of `code` align the constants; Cranelift never emits
    // `add %al,(%rax)`.
    while decoder.can_decode() && code[decoder.ip() as usize..].iter().any(|byte| *byte != 0) {
        decoder.decode_out(&mut instruction);
        let start = instruction.ip() as u32;
        let end = start + instruction.len() as u32;
        while srcloc.next_if(|(_, until, _)| *until <= start).is_some() {}
        if let Some((from, _, source)) = srcloc.peek()
            && *from <= start
            && line != Some(*source)
        {
            line = Some(*source);
            let _ = writeln!(out, "    # line {source}");
        }
        text.clear();
        formatter.format(&instruction, &mut text);
        let _ = write!(out, "{start:6x}:  {text}");
        let mut first = true;
        for reloc in relocs
            .iter()
            .filter(|reloc| (start..end).contains(&reloc.offset))
        {
            let _ = write!(
                out,
                "{}{}",
                if first { "  # " } else { ", " },
                names.reloc(&reloc.name)
            );
            if !matches!(reloc.addend, 0 | -4) {
                let _ = write!(out, "{:+}", reloc.addend);
            }
            first = false;
        }
        if let Some(helper) = traps
            .iter()
            .find(|(offset, _)| *offset == start)
            .and_then(|(_, code)| names.checks.helper(*code))
        {
            let _ = write!(out, "  # {helper}");
        }
        out.push('\n');
    }
    let mut offset = decoder.ip() as usize;
    for row in bytes[offset..].chunks(16) {
        let row: Vec<String> = row.iter().map(|byte| format!("0x{byte:02x}")).collect();
        let _ = writeln!(out, "{offset:6x}:  .byte   {}", row.join(","));
        offset += row.len();
    }
    out
}

/// Where the code in `bytes` ends: the lowest address in the function that an
/// instruction reads RIP-relatively, past that instruction. Cranelift emits
/// constants after the last instruction and only reaches them this way;
/// operands with a relocation refer elsewhere.
fn code_length(bytes: &[u8], relocs: &[ModuleReloc]) -> usize {
    let mut decoder = Decoder::with_ip(64, bytes, 0, DecoderOptions::NONE);
    let mut instruction = Instruction::default();
    let mut end = bytes.len();
    while decoder.can_decode() && (decoder.ip() as usize) < end {
        decoder.decode_out(&mut instruction);
        let next = instruction.next_ip();
        if instruction.is_ip_rel_memory_operand()
            && !relocs
                .iter()
                .any(|reloc| (instruction.ip()..next).contains(&u64::from(reloc.offset)))
        {
            let target = instruction.ip_rel_memory_address();
            if target >= next && target < end as u64 {
                end = target as usize;
            }
        }
    }
    end
}

/// Decode final A64 words, including UDF checks. A zero word is an instruction
/// on ARM, so the x86 trailing-zero padding heuristic must not be used here.
fn asm_arm64(
    bytes: &[u8],
    relocs: &[ModuleReloc],
    traps: &[(u32, u8)],
    srclocs: &[(u32, u32, u32)],
    names: &Names,
) -> String {
    let mut out = String::new();
    let mut line = None;
    let pools = arm64_literal_pools(bytes, relocs);
    let mut offset = 0;
    while offset + 4 <= bytes.len() {
        if let Some(pool) = pools.iter().find(|pool| pool.contains(&offset)) {
            for row in bytes[offset..pool.end].chunks(16) {
                let data: Vec<_> = row.iter().map(|byte| format!("0x{byte:02x}")).collect();
                let _ = writeln!(out, "{offset:6x}:  .byte   {}", data.join(","));
                offset += row.len();
            }
            continue;
        }
        let start = offset as u32;
        if let Some((_, _, source)) = srclocs
            .iter()
            .find(|(from, until, _)| (*from..*until).contains(&start))
            && line != Some(*source)
        {
            line = Some(*source);
            let _ = writeln!(out, "    # line {source}");
        }
        let text = decode_arm64(&bytes[offset..offset + 4]);
        let _ = write!(out, "{offset:6x}:  {text}");
        for reloc in relocs
            .iter()
            .filter(|r| (start..start + 4).contains(&r.offset))
        {
            let _ = write!(out, "  # {}", names.reloc(&reloc.name));
            if reloc.addend != 0 {
                let _ = write!(out, "{:+}", reloc.addend);
            }
        }
        if let Some(helper) = traps
            .iter()
            .find(|(at, _)| *at == start)
            .and_then(|(_, code)| names.checks.helper(*code))
        {
            let _ = write!(out, "  # {helper}");
        }
        out.push('\n');
        offset += 4;
    }
    for row in bytes[offset..].chunks(16) {
        let data: Vec<_> = row.iter().map(|byte| format!("0x{byte:02x}")).collect();
        let _ = writeln!(out, "{offset:6x}:  .byte   {}", data.join(","));
        offset += row.len();
    }
    out
}

fn decode_arm64(bytes: &[u8]) -> String {
    use yaxpeax_arch::{Decoder as _, U8Reader};
    match yaxpeax_arm::armv8::a64::InstDecoder::default().decode(&mut U8Reader::new(bytes)) {
        Ok(instruction) => instruction.to_string(),
        Err(_) => format!(
            ".inst 0x{:08x}",
            u32::from_le_bytes(bytes.try_into().expect("A64 word"))
        ),
    }
}

/// Literal loads identify their own payload, not the end of a function.
/// Cranelift can branch around a pool island and continue emitting code after
/// it. Skip known payload while finding additional loads, so constant bytes are
/// never themselves interpreted as instructions pointing at another pool.
fn arm64_literal_pools(bytes: &[u8], relocs: &[ModuleReloc]) -> Vec<std::ops::Range<usize>> {
    let mut pools: Vec<std::ops::Range<usize>> = Vec::new();
    let mut offset = 0;
    while offset + 4 <= bytes.len() {
        if let Some(end) = pools
            .iter()
            .filter(|pool| pool.contains(&offset))
            .map(|pool| pool.end)
            .max()
        {
            offset = end;
            continue;
        }
        let word = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if word & 0x3b00_0000 == 0x1800_0000
            && !relocs
                .iter()
                .any(|reloc| (offset..offset + 4).contains(&(reloc.offset as usize)))
        {
            let vector = word & (1 << 26) != 0;
            let width = match (vector, word >> 30) {
                (_, 0) | (false, 2) => 4,
                (_, 1) => 8,
                (true, 2) => 16,
                // PRFM literals name a prefetch address, not a constant value.
                _ => 0,
            };
            let displacement = ((word << 8) as i32 >> 13) * 4;
            let target = offset as i64 + i64::from(displacement);
            if width != 0 && target >= 0 {
                let start = target as usize;
                if start.is_multiple_of(4)
                    && start + width <= bytes.len()
                    && !(start..start + width).contains(&offset)
                {
                    pools.push(start..start + width);
                }
            }
        }
        offset += 4;
    }
    pools.sort_by_key(|pool| pool.start);
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for pool in pools {
        if let Some(previous) = merged.last_mut()
            && pool.start <= previous.end
        {
            previous.end = previous.end.max(pool.end);
        } else {
            merged.push(pool);
        }
    }
    merged
}

#[cfg(test)]
mod arm64_tests {
    use super::*;

    #[test]
    fn final_words_decode_as_arm_including_zero_trap() {
        assert_eq!(decode_arm64(&0xd65f03c0_u32.to_le_bytes()), "ret");
        assert!(decode_arm64(&0x91000400_u32.to_le_bytes()).starts_with("add x0, x0"));
        assert!(decode_arm64(&0_u32.to_le_bytes()).starts_with("udf"));
    }

    #[test]
    fn literal_pool_and_relocation_boundaries() {
        // ldr x0, pc+8; ret; an eight-byte constant.
        let bytes: Vec<_> = [0x58000040_u32, 0xd65f03c0, 42, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(arm64_literal_pools(&bytes, &[]), vec![8..16]);
        assert!(arm64_literal_pools(&0_u32.to_le_bytes(), &[]).is_empty());
    }
    #[test]
    fn instructions_after_a_middle_literal_island_remain_disassembled() {
        // ldr x0, pc+8; b pc+12; eight-byte constant; ret.
        let bytes: Vec<_> = [0x58000040_u32, 0x14000003, 42, 0, 0xd65f03c0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let isa = crate::codegen::baseline_isa(
            "aarch64-unknown-linux-gnu",
            "backtracking",
            "none",
            false,
        )
        .unwrap();
        let module = ObjectModule::new(
            cranelift_object::ObjectBuilder::new(
                isa,
                "island-dump",
                cranelift_module::default_libcall_names(),
            )
            .unwrap(),
        );
        let plan = Plan {
            options: DumpOptions::default(),
            optimize: false,
            subjects: HashMap::default(),
            strings: HashMap::default(),
            paths: Vec::new(),
        };
        let checks = super::super::traps::Checks::default();
        let names = Names {
            module: &module,
            plan: &plan,
            requests: &[],
            targets: &[],
            checks: &checks,
        };
        let listing = asm_arm64(&bytes, &[], &[], &[], &names);
        assert!(listing.contains("     8:  .byte"), "{listing}");
        assert!(listing.contains("    10:  ret"), "{listing}");
    }
}
