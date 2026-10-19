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
use crate::cache::Cache;

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
    cache: Option<Arc<Cache>>,
    /// Instrument functions built on workers for coverage ([`coverage`]).
    pub coverage: bool,
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

// Amortize channel wakeups without retaining a worker's entire output or
// assigning chunks whose expensive functions could leave other workers idle.
const RESULT_BATCH_SIZE: usize = 32;
type Batch = Vec<(usize, Finished)>;

impl Output {
    pub(super) fn new(targets: Targets, cache: Option<Arc<Cache>>) -> Result<Self> {
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
            cache,
            coverage: false,
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
            self.cache.as_deref(),
        );
        self.place(id, code, |_, _| Ok(()))
    }

    #[cfg_attr(feature = "profile", hotpath::measure)]
    pub(super) fn finish(self) -> cranelift_object::ObjectProduct {
        use cranelift_object::object::{elf, FileFlags};
        if let Some(cache) = &self.cache {
            cache.finish();
        }
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
    #[cfg_attr(feature = "profile", hotpath::measure)]
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
        let (finished, results) = mpsc::channel::<Batch>();
        let targets = self.targets.clone();
        let cache = self.cache.clone();
        let coverage = self.coverage;
        std::thread::scope(|scope| {
            let shared = (&queue, &stop, &build, &targets, &declared, &cache);
            let mut helpers = 0;
            for _ in 0..workers.min(total) {
                let finished = finished.clone();
                let spawned = std::thread::Builder::new()
                    .stack_size(crate::parallel::HELPER_STACK_BYTES)
                    .spawn_scoped(scope, move || {
                        let (queue, stop, build, targets, declared, cache) = shared;
                        let mut worker = Worker::new(
                            targets.clone(),
                            Arc::clone(declared),
                            cache.clone(),
                            coverage,
                        );
                        work(queue, stop, build, &mut worker, &finished);
                    });
                // A helper that cannot be spawned only costs speed.
                if spawned.is_ok() {
                    helpers += 1;
                }
            }
            if helpers == 0 {
                let mut worker = Worker::new(
                    targets.clone(),
                    Arc::clone(&declared),
                    cache.clone(),
                    coverage,
                );
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
        results: &mpsc::Receiver<Batch>,
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
                let Ok(batch) = results.recv() else {
                    return Err("a code generation thread stopped".into());
                };
                early.extend(batch);
            };
            for built in finished? {
                self.place(built.id, built.code, |module, relocs| {
                    symbols::resolve(module, runtime, built.id, &built.requests, relocs)
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
    finished: &mpsc::Sender<Batch>,
) {
    let mut batch = Vec::with_capacity(RESULT_BATCH_SIZE);
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
        batch.push((index, result.map(|()| built)));
        if batch.len() == RESULT_BATCH_SIZE {
            if finished.send(batch).is_err() {
                return;
            }
            batch = Vec::with_capacity(RESULT_BATCH_SIZE);
        }
    }
    // A later job can fail on another worker while this worker owns earlier
    // results. Flush those results even after cancellation so placement can
    // reach the first error in declaration order.
    if !batch.is_empty() {
        let _ = finished.send(batch);
    }
}

/// One thread's state for building and compiling functions.
pub(super) struct Worker {
    pub builder: FunctionBuilderContext,
    context: Context,
    targets: Targets,
    declared: Arc<Declared>,
    cache: Option<Arc<Cache>>,
    coverage: bool,
    built: Vec<Built>,
}

impl Worker {
    fn new(
        targets: Targets,
        declared: Arc<Declared>,
        cache: Option<Arc<Cache>>,
        coverage: bool,
    ) -> Self {
        Self {
            builder: FunctionBuilderContext::new(),
            context: Context::new(),
            targets,
            declared,
            cache,
            coverage,
            built: Vec::new(),
        }
    }

    /// The module view for building one function.
    pub(super) fn symbols(&self) -> Symbols {
        Symbols::new(Arc::clone(&self.declared))
    }

    /// Compile `func`, built against `symbols`, as the body of `id`.
    pub(super) fn define(&mut self, id: FuncId, mut func: Function, mut symbols: Symbols) {
        if self.coverage {
            super::coverage::instrument(&mut func, &mut symbols);
        }
        let isa = self.targets.for_function(&func);
        let code = compile(&mut self.context, isa, id, func, self.cache.as_deref());
        self.built.push(Built {
            id,
            code,
            requests: symbols.into_requests(),
        });
    }
}

/// Compile `func`, or take its code from `cache`. A hit yields the code the
/// compilation would have: Cranelift keys on the function without its
/// external names and applies this function's names to the cached result,
/// and the IR's requested symbols were rebuilt with it ([`symbols`]).
#[cfg_attr(feature = "profile", hotpath::measure)]
fn compile(
    context: &mut Context,
    isa: &dyn TargetIsa,
    id: FuncId,
    func: Function,
    cache: Option<&Cache>,
) -> Compiled {
    context.func = func;
    let compiled = crate::profile_block!(
        "output::cranelift_compile",
        match cache {
            Some(cache) => {
                let mut store = cache.store();
                let hit = context
                    .compile_with_cache(isa, &mut store, &mut ControlPlane::default())
                    .map(|(_, hit)| hit)
                    .map_err(|error| format!("{:?}", error.inner));
                hit.map(|hit| store.finish(hit))
            }
            None => context
                .compile(isa, &mut ControlPlane::default())
                .map(|_| ())
                .map_err(|error| format!("{:?}", error.inner)),
        }
    );
    let code = compiled.map(|()| {
        crate::profile_block!("output::collect_code", {
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
        })
    });
    crate::profile_block!("output::clear_context", context.clear());
    code
}

#[cfg(test)]
mod tests {
    use super::super::Build;
    use super::*;
    use crate::cache::CacheMode;
    use crate::value::Heap;
    use cranelift_codegen::ir::condcodes::IntCC;
    use cranelift_codegen::ir::{self, types, AbiParam, InstBuilder, Signature, UserFuncName};
    use cranelift_frontend::FunctionBuilder;

    fn targets() -> Targets {
        Targets {
            ordinary: super::super::host_isa().unwrap(),
            single_pass: super::super::target_isa("single_pass").unwrap(),
        }
    }

    #[test]
    fn result_batches_flush_on_completion_failure_and_cancellation() {
        for (count, fail, cancel, expected) in [
            (0, None, None, vec![]),
            (67, None, None, vec![32, 32, 3]),
            (67, Some(35), None, vec![32, 4]),
            (67, None, Some(5), vec![6]),
        ] {
            let targets = targets();
            let output = Output::new(targets.clone(), None).unwrap();
            let declared = Declared::snapshot(&output.module, targets.ordinary.clone());
            let mut worker = Worker::new(targets, Arc::new(declared), None, false);
            let queue = Mutex::new((0..count).collect::<Vec<_>>().into_iter().enumerate());
            let stop = AtomicBool::new(false);
            let (sender, receiver) = mpsc::channel();
            work(
                &queue,
                &stop,
                &|job, _| {
                    if Some(job) == cancel {
                        stop.store(true, Ordering::Relaxed);
                    }
                    if Some(job) == fail {
                        Err("failed".into())
                    } else {
                        Ok(())
                    }
                },
                &mut worker,
                &sender,
            );
            drop(sender);
            let batches: Vec<_> = receiver.into_iter().collect();
            assert_eq!(batches.iter().map(Vec::len).collect::<Vec<_>>(), expected);
            for (expected_index, (index, result)) in batches.into_iter().flatten().enumerate() {
                assert_eq!(index, expected_index);
                assert_eq!(result.is_err(), Some(index) == fail);
            }
        }
    }

    #[test]
    fn batched_build_reports_first_error_in_job_order() {
        for workers in [0, 1, 2, 8] {
            let mut output = Output::new(targets(), None).unwrap();
            let runtime = super::super::declare_runtime(&mut output.module, false).unwrap();
            let error = output
                .build((0..100).collect(), workers, &runtime, |job, _| {
                    if job == 35 || job == 36 {
                        Err(format!("job {job}").into())
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
            assert_eq!(error.to_string(), "job 35");
        }

        // Keep the first job in flight until a second worker starts a later
        // failure. Both errors are partial batches and job order must win.
        let mut output = Output::new(targets(), None).unwrap();
        let runtime = super::super::declare_runtime(&mut output.module, false).unwrap();
        let (sender, receiver) = mpsc::channel();
        let receiver = Mutex::new(receiver);
        let error = output
            .build(vec![0, 1], 2, &runtime, |job, _| {
                if job == 0 {
                    receiver.lock().unwrap().recv().unwrap();
                } else {
                    sender.send(()).unwrap();
                }
                Err(format!("job {job}").into())
            })
            .unwrap_err();
        assert_eq!(error.to_string(), "job 0");
    }

    #[test]
    fn worker_panic_disconnects_partial_batches_and_propagates() {
        let mut output = Output::new(targets(), None).unwrap();
        let runtime = super::super::declare_runtime(&mut output.module, false).unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            output.build((0..100).collect(), 2, &runtime, |job, _| {
                assert_ne!(job, 5, "worker panic with buffered results");
                Ok(())
            })
        }));
        assert!(panic.is_err());
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
        compile(
            &mut Context::new(),
            isa,
            FuncId::from_u32(0),
            func.clone(),
            None,
        )
        .unwrap()
        .bytes
    }

    /// What a worker produces for `func`, through the same path as a program.
    fn worker_machine_code(targets: &Targets, func: &Function) -> Vec<u8> {
        let output = Output::new(targets.clone(), None).unwrap();
        let declared = Declared::snapshot(&output.module, targets.ordinary.clone());
        let mut worker = Worker::new(targets.clone(), Arc::new(declared), None, false);
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

    /// Coverage counters are requested symbols like any other: the
    /// instrumented object is the same for every worker count and cache
    /// state, including a cache holding the plain build's entries.
    #[test]
    fn coverage_object_does_not_depend_on_worker_count() {
        let mut source: String = (0..UNITS).map(unit).collect();
        source.push_str("def main() -> ():\n    pass\n");
        let coverage = Build {
            coverage: true,
            ..Build::default()
        };
        let serial = object(&source, 1, coverage.clone());
        assert!(
            serial == object(&source, 8, coverage),
            "workers changed the object"
        );

        let dir = tempfile::tempdir().unwrap();
        let plain = cached(dir.path(), false);
        assert!(
            serial != object(&source, 1, plain),
            "coverage changed nothing"
        );
        let cached = Build {
            coverage: true,
            ..cached(dir.path(), false)
        };
        // Equal bytes also show no plain entry was taken for an instrumented
        // function.
        assert!(
            serial == object(&source, 2, cached.clone()),
            "cache changed the object"
        );
        assert!(
            serial == object(&source, 8, cached),
            "warm cache changed the object"
        );
    }

    /// The object for `source` with `build`, as bytes.
    fn object(source: &str, workers: usize, build: Build) -> Vec<u8> {
        let mut heap = Heap::default();
        let program = crate::frontend::compile(source, &mut heap).unwrap();
        super::super::object_bytes(program.ops, &heap, Some(false), &[], None, workers, build)
            .unwrap()
    }

    /// A build using the cache in `dir`. With `check`, Cranelift compiles
    /// every hit again and asserts the cached code matches.
    fn cached(dir: &std::path::Path, check: bool) -> Build {
        Build {
            cache: Cache::open(&CacheMode::Directory(dir.to_path_buf())),
            check_cache_hits: check,
            ..Build::default()
        }
    }

    /// Workers, and the cache in any state, must not change the object
    /// (issues #58 and #144).
    fn assert_same_object_for_any_worker_count(mut source: String) {
        source.push_str("def main() -> ():\n    pass\n");
        let serial = object(&source, 1, Build::default());
        assert!(
            serial == object(&source, 0, Build::default()),
            "caller-thread fallback changed the object"
        );
        assert!(
            serial == object(&source, 2, Build::default()),
            "two workers changed the object"
        );
        assert!(
            serial == object(&source, 8, Build::default()),
            "eight workers changed the object"
        );

        let dir = tempfile::tempdir().unwrap();
        let checked = cached(dir.path(), true);
        assert!(
            serial == object(&source, 1, checked.clone()),
            "cold cache changed the object"
        );
        let hits = checked.cache.as_ref().unwrap().stats().hits;
        assert!(
            serial == object(&source, 8, checked),
            "warm cache changed the object"
        );
        let warm = cached(dir.path(), true);
        assert!(
            serial == object(&source, 2, warm.clone()),
            "warm cache changed the object"
        );
        let stats = warm.cache.unwrap().stats();
        assert_eq!(stats.misses, 0, "a warm cache missed: {stats:?}");
        assert!(stats.hits > hits, "{stats:?}");

        // The same without Cranelift's hit checks, which are an ISA flag of
        // their own: the entries plain builds use.
        let dir = tempfile::tempdir().unwrap();
        assert!(serial == object(&source, 3, cached(dir.path(), false)));
        assert!(serial == object(&source, 1, cached(dir.path(), false)));
    }

    /// Two programs whose functions build the same stencil but request
    /// different symbols (type metadata, trap tables for other source
    /// positions) must each get their own symbols from a cached entry.
    #[test]
    fn cached_code_resolves_each_programs_own_symbols() {
        let shows = |ty: &str, lead: &str| {
            format!(
                "{lead}def show(items: {ty}) -> Result[(), IoError]:
    print(items)?
    print(items[0])?
    Ok(())

def main() -> ():
    pass
"
            )
        };
        let ints = shows("list[i64]", "");
        let strs = shows("list[str]", "\n\n# shifted\n");
        let dir = tempfile::tempdir().unwrap();
        let cold = cached(dir.path(), true);
        assert!(object(&ints, 2, Build::default()) == object(&ints, 2, cold));
        let warm = cached(dir.path(), true);
        assert!(object(&strs, 2, Build::default()) == object(&strs, 2, warm.clone()));
        assert!(warm.cache.unwrap().stats().hits > 0);
    }

    /// The build's ISA with `flags` changed, as the cache key sees it.
    fn isa_with(flags: &[(&str, &str)]) -> OwnedTargetIsa {
        use cranelift_codegen::settings::{self, Configurable};
        let mut builder = settings::builder();
        for (name, value) in [("is_pic", "true"), ("enable_llvm_abi_extensions", "true")]
            .iter()
            .chain(flags)
        {
            builder.set(name, value).unwrap();
        }
        cranelift_native::builder_with_options(false)
            .unwrap()
            .finish(settings::Flags::new(builder))
            .unwrap()
    }

    /// `return value` with the given type, calling the imported function
    /// `callee` first when given: the body, signature, and callee identity
    /// are the stencil inputs the key must and must not react to.
    fn constant(ty: cranelift_codegen::ir::Type, value: i64, callee: Option<u32>) -> Function {
        let mut signature = Signature::new(targets().ordinary.default_call_conv());
        signature.returns.push(AbiParam::new(ty));
        let mut func = Function::with_name_signature(UserFuncName::default(), signature.clone());
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut context);
        let entry = builder.create_block();
        builder.switch_to_block(entry);
        if let Some(index) = callee {
            let signature = builder.import_signature(signature);
            let name = builder
                .func
                .declare_imported_user_function(ir::UserExternalName::new(0, index));
            let callee = builder.import_function(ir::ExtFuncData {
                name: ir::ExternalName::user(name),
                signature,
                colocated: true,
                patchable: false,
            });
            builder.ins().call(callee, &[]);
        }
        let result = builder.ins().iconst(ty, value);
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize();
        func
    }

    /// Every input the key covers, changed one at a time, is a miss, and
    /// per-program function identities are not an input: that is what lets
    /// one program's `std` code serve another.
    #[test]
    fn key_covers_the_stencil_and_isa_flags_but_not_symbol_identities() {
        // `CacheKeyHash` has no `Debug`; its `Display` is the whole hash.
        let compute_cache_key = |isa: &dyn TargetIsa, func: &Function| {
            cranelift_codegen::incremental_cache::compute_cache_key(isa, func).to_string()
        };
        let targets = targets();
        let isa = &*targets.ordinary;
        let key = compute_cache_key(isa, &constant(types::I64, 1, Some(3)));
        assert_eq!(
            key,
            compute_cache_key(isa, &constant(types::I64, 1, Some(3)))
        );
        assert_eq!(
            key,
            compute_cache_key(isa, &constant(types::I64, 1, Some(7))),
            "the callee's identity is applied after compilation"
        );
        assert_ne!(
            key,
            compute_cache_key(isa, &constant(types::I64, 2, Some(3))),
            "body"
        );
        assert_ne!(
            key,
            compute_cache_key(isa, &constant(types::I32, 1, Some(3))),
            "signature"
        );
        assert_ne!(
            key,
            compute_cache_key(isa, &constant(types::I64, 1, None)),
            "callees"
        );
        assert_ne!(
            key,
            compute_cache_key(&*targets.single_pass, &constant(types::I64, 1, Some(3))),
            "register allocator"
        );
        let function = constant(types::I64, 1, Some(3));
        let base = compute_cache_key(&*isa_with(&[]), &function);
        for (name, value) in [
            ("opt_level", "speed"),
            ("enable_verifier", "false"),
            ("enable_probestack", "true"),
            ("preserve_frame_pointers", "true"),
            ("enable_incremental_compilation_cache_checks", "true"),
        ] {
            assert_ne!(
                base,
                compute_cache_key(&*isa_with(&[(name, value)]), &function),
                "{name}"
            );
        }
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
