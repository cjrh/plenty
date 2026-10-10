//! The object under construction and the functions waiting to be compiled.
//!
//! Building a function's IR reads the frontend's tables and declares symbols
//! in the module, so it stays on the calling thread. Turning finished IR into
//! machine code needs only that IR and the target, so each batch of functions
//! is compiled on helper threads while the caller builds the next batch.
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa::{OwnedTargetIsa, TargetIsa};
use cranelift_codegen::Context;
use cranelift_frontend::FunctionBuilderContext;
use cranelift_module::{FuncId, Module, ModuleReloc};
use cranelift_object::{ObjectBuilder, ObjectModule};

use super::Result;

/// IR instructions queued before a batch is handed to the helpers. This
/// bounds the IR held at once. It must not depend on the worker count: batch
/// boundaries decide when definitions reach the module relative to later
/// declarations, and that sequence has to be the same on every machine for
/// the object to be.
const BATCH_INSTRUCTIONS: usize = 1 << 16;

/// Cranelift used to run on the main thread, whose stack is typically 8 MiB.
/// Rust's 2 MiB default for spawned threads would be a smaller limit.
const HELPER_STACK_BYTES: usize = 8 << 20;

pub(super) struct Output {
    pub module: ObjectModule,
    /// Scratch storage for building one function; `finalize` empties it for
    /// the next.
    pub builder: FunctionBuilderContext,
    isa: OwnedTargetIsa,
    pending: Vec<(FuncId, Function)>,
    pending_instructions: usize,
    compiling: Option<Batch>,
    workers: usize,
}

struct Code {
    alignment: u64,
    bytes: Vec<u8>,
    relocs: Vec<ModuleReloc>,
}

type Compiled = std::result::Result<Code, String>;
type Queue = Mutex<std::iter::Enumerate<std::vec::IntoIter<(FuncId, Function)>>>;

/// Functions being compiled by helper threads.
struct Batch {
    queue: Arc<Queue>,
    helpers: Vec<JoinHandle<Vec<(usize, FuncId, Compiled)>>>,
}

impl Output {
    pub(super) fn new(isa: OwnedTargetIsa, workers: usize) -> Result<Self> {
        let builder = ObjectBuilder::new(
            isa.clone(),
            "plenty",
            cranelift_module::default_libcall_names(),
        )?;
        Ok(Self {
            module: ObjectModule::new(builder),
            builder: FunctionBuilderContext::new(),
            isa,
            pending: Vec::new(),
            pending_instructions: 0,
            compiling: None,
            workers,
        })
    }

    /// Queue `func` as the body of the declared function `id`. A compilation
    /// error surfaces from a later call or from [`Output::finish`].
    pub(super) fn define(&mut self, id: FuncId, func: Function) -> Result<()> {
        self.pending_instructions += func.dfg.num_insts();
        self.pending.push((id, func));
        if self.pending_instructions >= BATCH_INSTRUCTIONS {
            self.start_batch()?;
        }
        Ok(())
    }

    /// Compile what is still queued and serialize the object.
    pub(super) fn finish(mut self) -> Result<Vec<u8>> {
        self.start_batch()?;
        self.define_compiled()?;
        Ok(self.module.finish().emit()?)
    }

    /// Hand the queued functions to helpers. The batch before them is defined
    /// first, so definitions happen at points fixed by the program alone.
    fn start_batch(&mut self) -> Result<()> {
        self.define_compiled()?;
        self.pending_instructions = 0;
        let functions = std::mem::take(&mut self.pending);
        self.compiling = Some(Batch::start(&self.isa, functions, self.workers));
        Ok(())
    }

    fn define_compiled(&mut self) -> Result<()> {
        let Some(batch) = self.compiling.take() else {
            return Ok(());
        };
        for (id, code) in batch.finish(&*self.isa) {
            let code = code.map_err(|error| {
                let declaration = self.module.declarations().get_function_decl(id);
                format!("compiling `{}`: {error}", declaration.linkage_name(id))
            })?;
            self.module
                .define_function_bytes(id, code.alignment, &code.bytes, &code.relocs)?;
        }
        Ok(())
    }
}

impl Batch {
    fn start(isa: &OwnedTargetIsa, functions: Vec<(FuncId, Function)>, workers: usize) -> Self {
        // The calling thread is the remaining worker: it drains the queue in
        // `finish`, so a helper that cannot be spawned only costs speed.
        let helpers = workers.min(functions.len()).saturating_sub(1);
        let queue = Arc::new(Mutex::new(functions.into_iter().enumerate()));
        let helpers = (0..helpers)
            .filter_map(|_| {
                let (queue, isa) = (Arc::clone(&queue), Arc::clone(isa));
                std::thread::Builder::new()
                    .stack_size(HELPER_STACK_BYTES)
                    .spawn(move || drain(&queue, &*isa))
                    .ok()
            })
            .collect();
        Self { queue, helpers }
    }

    /// Wait for every function, returning the results in the order the
    /// functions were queued whichever thread compiled them.
    fn finish(self, isa: &dyn TargetIsa) -> Vec<(FuncId, Compiled)> {
        let mut done = drain(&self.queue, isa);
        for helper in self.helpers {
            done.extend(
                helper
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
            );
        }
        done.sort_unstable_by_key(|(index, ..)| *index);
        done.into_iter().map(|(_, id, code)| (id, code)).collect()
    }
}

fn drain(queue: &Queue, isa: &dyn TargetIsa) -> Vec<(usize, FuncId, Compiled)> {
    let mut context = Context::new();
    let mut done = Vec::new();
    loop {
        // The guard is a temporary of this statement, so the lock is not held
        // while compiling.
        let next = queue.lock().unwrap().next();
        let Some((index, (id, func))) = next else {
            break done;
        };
        done.push((index, id, compile(&mut context, isa, id, func)));
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
    use crate::value::Heap;

    /// Enough declarations to span several batches of `BATCH_INSTRUCTIONS`.
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

    /// Helper threads finish in any order, and the frontend's maps iterate in
    /// a different order on every run. Neither may reach the object.
    #[test]
    fn object_does_not_depend_on_worker_count() {
        let mut source: String = (0..UNITS).map(unit).collect();
        source.push_str("def main() -> ():\n    pass\n");
        let mut heap = Heap::default();
        let program = crate::frontend::compile(&source, &mut heap).unwrap();
        let object = |workers| {
            super::super::object_bytes(&program.ops, &heap, Some(false), &[], None, workers)
                .unwrap()
        };
        let serial = object(1);
        assert!(serial == object(2), "two workers changed the object");
        assert!(serial == object(8), "eight workers changed the object");
    }
}
