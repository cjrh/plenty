//! The object under construction and the threads building its functions.
//!
//! Each function's IR is built and compiled to machine code on a worker
//! thread. The calling thread places the results in the object strictly in
//! the order the functions were queued, declaring the symbols their bodies
//! requested as it goes ([`symbols`]), so the object is the same for every
//! worker count.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa::{OwnedTargetIsa, TargetIsa};
use cranelift_codegen::Context;
use cranelift_frontend::FunctionBuilderContext;
use cranelift_module::{FuncId, Module, ModuleReloc};
use cranelift_object::{ObjectBuilder, ObjectModule};

use super::symbols::{self, Declared, Request, Symbols};
use super::{Result, Runtime};

/// Branch arguments in one function above which Cranelift's single-pass
/// register allocator compiles it. The backtracking allocator re-sorts a block
/// parameter's whole live-range list for each branch argument merged into it,
/// and rescans the branch arguments for each value live into a block, so its
/// time grows with the square of this count (issue #56). The single-pass
/// allocator is linear but emits larger, slower code, so the limit is far above
/// what hand-written functions reach. The count is a property of the function
/// alone: the choice cannot depend on the worker count.
const SINGLE_PASS_BRANCH_ARGUMENTS: usize = 2048;

/// One target, once per register allocator.
#[derive(Clone)]
pub(super) struct Targets {
    /// Backtracking allocator, used for the module and ordinary functions.
    pub ordinary: OwnedTargetIsa,
    /// Single-pass allocator, for functions over
    /// [`SINGLE_PASS_BRANCH_ARGUMENTS`].
    pub single_pass: OwnedTargetIsa,
}

impl Targets {
    fn for_function(&self, func: &Function) -> &dyn TargetIsa {
        if branch_arguments(func) > SINGLE_PASS_BRANCH_ARGUMENTS {
            &*self.single_pass
        } else {
            &*self.ordinary
        }
    }
}

/// Values passed to block parameters, summed over every branch in `func`.
fn branch_arguments(func: &Function) -> usize {
    func.layout
        .blocks()
        .filter_map(|block| func.layout.last_inst(block))
        .flat_map(|branch| {
            func.dfg.insts[branch]
                .branch_destination(&func.dfg.jump_tables, &func.dfg.exception_tables)
        })
        .map(|destination| destination.len(&func.dfg.value_lists))
        .sum()
}

pub(super) struct Output {
    pub module: ObjectModule,
    /// Scratch storage for building a function on the calling thread;
    /// `finalize` empties it for the next.
    pub builder: FunctionBuilderContext,
    targets: Targets,
    context: Context,
}

struct Code {
    alignment: u64,
    bytes: Vec<u8>,
    relocs: Vec<ModuleReloc>,
}

type Compiled = std::result::Result<Code, String>;

/// A function compiled by a worker, waiting to be placed in the object.
struct Built {
    id: FuncId,
    code: Compiled,
    requests: Vec<Request>,
}

/// A job's functions, or why the job failed. Errors cross threads as text.
type Finished = std::result::Result<Vec<Built>, String>;

impl Output {
    pub(super) fn new(targets: Targets) -> Result<Self> {
        let mut builder = ObjectBuilder::new(
            targets.ordinary.clone(),
            "plenty",
            cranelift_module::default_libcall_names(),
        )?;
        // Every std function of an imported module is compiled, and some call
        // runtime entries such as `plenty_tls` or `plenty_sqlite`. One section
        // per symbol lets the linker's `--gc-sections` drop the unused ones,
        // so a program links those runtime stacks only when it reaches them
        // (issue #127).
        builder
            .per_function_section(true)
            .per_data_object_section(true);
        Ok(Self {
            module: ObjectModule::new(builder),
            builder: FunctionBuilderContext::new(),
            targets,
            context: Context::new(),
        })
    }

    /// Compile `func` on this thread as the body of the declared function
    /// `id`. Its IR must name only symbols declared in the module.
    pub(super) fn define(&mut self, id: FuncId, func: Function) -> Result<()> {
        let code = compile(
            &mut self.context,
            self.targets.for_function(&func),
            id,
            func,
        );
        self.place(id, code, |_, _| Ok(()))
    }

    pub(super) fn finish(self) -> cranelift_object::ObjectProduct {
        use cranelift_object::object::{elf, FileFlags};
        let mut product = self.module.finish();
        // GNU ld honours SHF_GNU_RETAIN, which keeps a library's interface
        // section through `--gc-sections`, only in GNU OSABI objects, as GNU
        // as marks them. Cranelift writes the System V OSABI.
        if let FileFlags::None = product.object.flags {
            product.object.flags = FileFlags::Elf {
                os_abi: elf::ELFOSABI_GNU,
                abi_version: 0,
                e_flags: 0,
            };
        }
        product
    }

    /// Run `build` on each job across `workers` threads, placing the
    /// functions each job defines in job order. The first error in job order
    /// is returned, whichever thread hit an error first.
    pub(super) fn build<J: Send>(
        &mut self,
        jobs: Vec<J>,
        workers: usize,
        runtime: &Runtime,
        build: impl Fn(J, &mut Worker) -> Result<()> + Sync,
    ) -> Result<()> {
        let declared = Arc::new(Declared::snapshot(
            &self.module,
            self.targets.ordinary.clone(),
        ));
        let total = jobs.len();
        let queue = Mutex::new(jobs.into_iter().enumerate());
        let stop = AtomicBool::new(false);
        let (finished, results) = mpsc::channel::<(usize, Finished)>();
        let targets = self.targets.clone();
        std::thread::scope(|scope| {
            let shared = (&queue, &stop, &build, &targets, &declared);
            let mut helpers = 0;
            for _ in 0..workers.min(total) {
                let finished = finished.clone();
                let spawned = std::thread::Builder::new()
                    .stack_size(crate::parallel::HELPER_STACK_BYTES)
                    .spawn_scoped(scope, move || {
                        let (queue, stop, build, targets, declared) = shared;
                        let mut worker = Worker::new(targets.clone(), Arc::clone(declared));
                        work(queue, stop, build, &mut worker, &finished);
                    });
                // A helper that cannot be spawned only costs speed.
                if spawned.is_ok() {
                    helpers += 1;
                }
            }
            if helpers == 0 {
                let mut worker = Worker::new(targets.clone(), Arc::clone(&declared));
                work(&queue, &stop, &build, &mut worker, &finished);
            }
            drop(finished);
            let placed = self.place_in_order(total, &results, runtime);
            if placed.is_err() {
                stop.store(true, Ordering::Relaxed);
            }
            placed
        })
    }

    fn place_in_order(
        &mut self,
        total: usize,
        results: &mpsc::Receiver<(usize, Finished)>,
        runtime: &Runtime,
    ) -> Result<()> {
        let mut early = HashMap::new();
        for index in 0..total {
            let finished = loop {
                if let Some(finished) = early.remove(&index) {
                    break finished;
                }
                // Every sender is gone only if a worker panicked; the scope
                // raises that panic once this returns.
                let Ok((done, finished)) = results.recv() else {
                    return Err("a code generation thread stopped".into());
                };
                early.insert(done, finished);
            };
            for built in finished? {
                self.place(built.id, built.code, |module, relocs| {
                    symbols::resolve(module, runtime, &built.requests, relocs)
                })?;
            }
        }
        Ok(())
    }

    fn place(
        &mut self,
        id: FuncId,
        code: Compiled,
        relocate: impl FnOnce(&mut ObjectModule, &mut [ModuleReloc]) -> Result<()>,
    ) -> Result<()> {
        let mut code = code.map_err(|error| {
            let declaration = self.module.declarations().get_function_decl(id);
            format!("compiling `{}`: {error}", declaration.linkage_name(id))
        })?;
        relocate(&mut self.module, &mut code.relocs)?;
        self.module
            .define_function_bytes(id, code.alignment, &code.bytes, &code.relocs)?;
        Ok(())
    }
}

/// Take jobs in queue order until none remain or one fails. Jobs are claimed
/// in order, so every job before a failed one still finishes.
fn work<J>(
    queue: &Mutex<std::iter::Enumerate<std::vec::IntoIter<J>>>,
    stop: &AtomicBool,
    build: &impl Fn(J, &mut Worker) -> Result<()>,
    worker: &mut Worker,
    finished: &mpsc::Sender<(usize, Finished)>,
) {
    while !stop.load(Ordering::Relaxed) {
        // The guard is a temporary of this statement, so the lock is not held
        // while building.
        let next = queue.lock().unwrap().next();
        let Some((index, job)) = next else {
            break;
        };
        let result = build(job, worker).map_err(|error| error.to_string());
        let built = std::mem::take(&mut worker.built);
        if result.is_err() {
            stop.store(true, Ordering::Relaxed);
        }
        if finished.send((index, result.map(|()| built))).is_err() {
            break;
        }
    }
}

/// One thread's state for building and compiling functions.
pub(super) struct Worker {
    pub builder: FunctionBuilderContext,
    context: Context,
    targets: Targets,
    declared: Arc<Declared>,
    built: Vec<Built>,
}

impl Worker {
    fn new(targets: Targets, declared: Arc<Declared>) -> Self {
        Self {
            builder: FunctionBuilderContext::new(),
            context: Context::new(),
            targets,
            declared,
            built: Vec::new(),
        }
    }

    /// The module view for building one function.
    pub(super) fn symbols(&self) -> Symbols {
        Symbols::new(Arc::clone(&self.declared))
    }

    /// Compile `func`, built against `symbols`, as the body of `id`.
    pub(super) fn define(&mut self, id: FuncId, func: Function, symbols: Symbols) {
        let isa = self.targets.for_function(&func);
        let code = compile(&mut self.context, isa, id, func);
        self.built.push(Built {
            id,
            code,
            requests: symbols.into_requests(),
        });
    }
}

fn compile(context: &mut Context, isa: &dyn TargetIsa, id: FuncId, func: Function) -> Compiled {
    context.func = func;
    let code = context
        .compile(isa, &mut ControlPlane::default())
        .map(|_| ())
        .map_err(|error| format!("{:?}", error.inner))
        .map(|()| {
            let buffer = &context.compiled_code().unwrap().buffer;
            Code {
                alignment: buffer.alignment as u64,
                bytes: buffer.data().to_vec(),
                relocs: buffer
                    .relocs()
                    .iter()
                    .map(|reloc| ModuleReloc::from_mach_reloc(reloc, &context.func, id))
                    .collect(),
            }
        });
    context.clear();
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Heap;
    use cranelift_codegen::ir::condcodes::IntCC;
    use cranelift_codegen::ir::{types, AbiParam, InstBuilder, Signature, UserFuncName};
    use cranelift_frontend::FunctionBuilder;

    fn targets() -> Targets {
        Targets {
            ordinary: super::super::host_isa().unwrap(),
            single_pass: super::super::target_isa("single_pass").unwrap(),
        }
    }

    /// `y = x`, then `count` times `if y > i: y = y + 3`. Each conditional
    /// passes `y` to its merge block from both arms: two branch arguments.
    fn conditional_assignments(targets: &Targets, count: usize) -> Function {
        let mut signature = Signature::new(targets.ordinary.default_call_conv());
        signature.params.push(AbiParam::new(types::I64));
        signature.returns.push(AbiParam::new(types::I64));
        let mut func = Function::with_name_signature(UserFuncName::default(), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let mut y = builder.block_params(entry)[0];
        for i in 0..count {
            let (assign, merge) = (builder.create_block(), builder.create_block());
            let merged = builder.append_block_param(merge, types::I64);
            let above = builder
                .ins()
                .icmp_imm(IntCC::SignedGreaterThan, y, i as i64);
            builder.ins().brif(above, assign, &[], merge, &[y.into()]);
            builder.switch_to_block(assign);
            let assigned = builder.ins().iadd_imm(y, 3);
            builder.ins().jump(merge, &[assigned.into()]);
            builder.switch_to_block(merge);
            y = merged;
        }
        builder.ins().return_(&[y]);
        builder.seal_all_blocks();
        builder.finalize();
        func
    }

    fn machine_code(isa: &dyn TargetIsa, func: &Function) -> Vec<u8> {
        compile(&mut Context::new(), isa, FuncId::from_u32(0), func.clone())
            .unwrap()
            .bytes
    }

    /// What a worker produces for `func`, through the same path as a program.
    fn worker_machine_code(targets: &Targets, func: &Function) -> Vec<u8> {
        let output = Output::new(targets.clone()).unwrap();
        let declared = Declared::snapshot(&output.module, targets.ordinary.clone());
        let mut worker = Worker::new(targets.clone(), Arc::new(declared));
        let symbols = worker.symbols();
        worker.define(FuncId::from_u32(0), func.clone(), symbols);
        worker.built.pop().unwrap().code.unwrap().bytes
    }

    /// The backtracking allocator's time is quadratic in a function's branch
    /// arguments (issue #56), so a function over the limit must not reach it,
    /// and a function at the limit must keep the better code.
    #[test]
    fn branch_arguments_choose_the_register_allocator() {
        let targets = targets();
        let at_limit = conditional_assignments(&targets, SINGLE_PASS_BRANCH_ARGUMENTS / 2);
        assert_eq!(branch_arguments(&at_limit), SINGLE_PASS_BRANCH_ARGUMENTS);
        let ordinary = machine_code(&*targets.ordinary, &at_limit);
        assert!(ordinary != machine_code(&*targets.single_pass, &at_limit));
        assert!(worker_machine_code(&targets, &at_limit) == ordinary);

        let over_limit = conditional_assignments(&targets, SINGLE_PASS_BRANCH_ARGUMENTS / 2 + 1);
        let single_pass = machine_code(&*targets.single_pass, &over_limit);
        assert!(single_pass != machine_code(&*targets.ordinary, &over_limit));
        assert!(worker_machine_code(&targets, &over_limit) == single_pass);
    }

    /// Enough functions that workers finish them out of order.
    const UNITS: usize = 400;

    fn unit(i: usize) -> String {
        format!(
            "class Point{i}:
    x: i64
    y: i64

    def squared_length(self) -> i64:
        self.x * self.x + self.y * self.y + {i}

def label{i}(point: &Point{i}) -> Result[(), IoError]:
    if point.squared_length() > {i}:
        print(\"far {i}\")?
    Ok(())

def squares{i}(limit: i64) -> Result[list[i64], AllocError]:
    [n * n + {i} for n in range(limit) if n % 2 == 0]

"
        )
    }

    /// Workers finish in any order, and the frontend's maps iterate in a
    /// different order on every run. Neither may reach the object.
    #[test]
    fn object_does_not_depend_on_worker_count() {
        let mut source: String = (0..UNITS).map(unit).collect();
        // One function over `SINGLE_PASS_BRANCH_ARGUMENTS`, so both register
        // allocators contribute to the object.
        source.push_str("def large(x: i64) -> i64:\n    mut y = x\n");
        for i in 0..SINGLE_PASS_BRANCH_ARGUMENTS / 2 + 100 {
            source.push_str(&format!("    if y > {i}:\n        y = y + 3\n"));
        }
        source.push_str("    y\n\n");
        assert_same_object_for_any_worker_count(source);
    }

    fn assert_same_object_for_any_worker_count(mut source: String) {
        source.push_str("def main() -> ():\n    pass\n");
        let mut heap = Heap::default();
        let program = crate::frontend::compile(&source, &mut heap).unwrap();
        let object = |workers| {
            super::super::object_bytes(program.ops.clone(), &heap, Some(false), &[], None, workers)
                .unwrap()
        };
        let serial = object(1);
        assert!(serial == object(2), "two workers changed the object");
        assert!(serial == object(8), "eight workers changed the object");
    }

    /// Symbols a body declares on first use: type metadata, trap-site tables,
    /// thread and executor adapters, generator frames, runtime imports.
    fn declaring_unit(i: usize) -> String {
        format!(
            "def count{i}(start: i64) -> Generator[i64]:
    mut remaining = start
    while remaining > 0:
        yield remaining // {i} + 1
        remaining = remaining - 1

def table{i}(n: i64) -> Result[dict[str, list[i64]], AllocError]:
    mut table: dict[str, list[i64]] = {{}}?
    table[\"k{i}\"] = [n, n + {i}]?
    Ok(table)

def worker{i}(n: i64) -> Result[list[i64], AllocError]:
    [x * {i} for x in range(n)]

def scaled{i}(n: i64) -> i64:
    n * {i} + 1

def run{i}(n: i64) -> Result[(), Failure]:
    for value in count{i}(n):
        print(value)?
    print(table{i}(n)?[\"k{i}\"][1] // n)?
    with spawn(worker{i}, n)? as task:
        print(task.join()?)?
    with ThreadPoolExecutor(2, 2)? as pool:
        print(pool.map(scaled{i}, range(n))?)?
    Ok(())

"
        )
    }

    #[test]
    fn declarations_do_not_depend_on_worker_count() {
        assert_same_object_for_any_worker_count((0..UNITS / 4).map(declaring_unit).collect());
    }
}
