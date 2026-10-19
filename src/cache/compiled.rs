//! The adapter to Cranelift 0.131's incremental-cache protocol. Keep the key
//! parity tests when upgrading Cranelift: its Hash byte stream is not an API.
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::incremental_cache::{
    serialize_compiled, try_finish_recompile, CacheKvStore,
};
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::{CompiledCode, Context, FinalizedMachReloc};
use sha2::{Digest, Sha256};

use super::Cache;

/// Only fields consumed by Plenty's object emitter. Relocations still refer
/// to stencil indices: resolve them against the current function, never the
/// function that populated the cache. Source locations and other parameterized
/// Cranelift metadata deliberately do not survive here.
pub(crate) struct MachineCode {
    pub alignment: u64,
    pub bytes: Vec<u8>,
    pub relocs: Vec<FinalizedMachReloc>,
}

impl From<&CompiledCode> for MachineCode {
    fn from(code: &CompiledCode) -> Self {
        Self {
            alignment: code.buffer.alignment as u64,
            bytes: code.buffer.data().to_vec(),
            relocs: code.buffer.relocs().to_vec(),
        }
    }
}

struct Entry {
    code: Arc<MachineCode>,
    compile_time: Duration,
}

type Slot = Arc<OnceLock<Result<Entry, String>>>;
#[derive(Default)]
pub(super) struct Entries(Mutex<HashMap<[u8; 32], Slot>>);

/// Concatenation preserves every write, including Hash's length and delimiter
/// writes, while avoiding SHA-256's streaming overhead on each tiny fragment.
#[derive(Default)]
struct Bytes(Vec<u8>);

impl Hasher for Bytes {
    fn finish(&self) -> u64 {
        unreachable!("cache hashing uses the complete SHA-256 digest")
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
}

fn key(isa: &dyn TargetIsa, func: &Function) -> [u8; 32] {
    let mut bytes = Bytes::default();
    func.stencil.hash(&mut bytes);
    isa.name().hash(&mut bytes);
    isa.triple().hash(&mut bytes);
    isa.flags().hash(&mut bytes);
    isa.isa_flags_hash_key().hash(&mut bytes);
    let key = Sha256::digest(&bytes.0).into();
    #[cfg(test)]
    assert_eq!(
        format!("CacheKeyHash:{key:?}"),
        cranelift_codegen::incremental_cache::compute_cache_key(isa, func).to_string(),
        "buffered key must match the pinned Cranelift Hash protocol"
    );
    key
}

impl Cache {
    pub(crate) fn compile(
        &self,
        context: &mut Context,
        isa: &dyn TargetIsa,
    ) -> Result<Arc<MachineCode>, String> {
        // Preserve the upstream debugging facility, including its comparison
        // of metadata which the normal Plenty emitter does not consume.
        if isa.flags().enable_incremental_compilation_cache_checks() {
            let mut store = self.store();
            let (compiled, hit) = context
                .compile_with_cache(isa, &mut store, &mut ControlPlane::default())
                .map_err(|error| format!("{:?}", error.inner))?;
            let code = Arc::new(MachineCode::from(compiled));
            store.finish(hit);
            return Ok(code);
        }

        let started = Instant::now();
        let key = key(isa, &context.func);
        let slot = self
            .compiled
            .0
            .lock()
            .unwrap()
            .entry(key)
            .or_default()
            .clone();
        let mut initialized = false;
        let entry = slot.get_or_init(|| {
            initialized = true;
            let mut store = self.store();
            if let Some(blob) = store.get(&key) {
                if let Ok(compiled) = try_finish_recompile(&context.func, &blob) {
                    let compile_time = store.read.get().unwrap_or_default();
                    let code = Arc::new(MachineCode::from(&compiled));
                    store.finish(true);
                    return Ok(Entry { code, compile_time });
                }
            }
            let stencil = context
                .compile_stencil(isa, &mut ControlPlane::default())
                .map_err(|error| format!("{error:?}"))?;
            let (stencil, serialized) = serialize_compiled(stencil);
            if let Ok(blob) = serialized {
                store.insert(&key, blob);
            }
            let compile_time = store.started.elapsed();
            let compiled = stencil.apply_params(&context.func.params);
            let code = Arc::new(MachineCode::from(&compiled));
            store.finish(false);
            Ok(Entry { code, compile_time })
        });
        match entry {
            Ok(entry) => {
                if !initialized {
                    let store = self.store();
                    store.read.set(Some(entry.compile_time));
                    // Include time waiting for another worker's initialization.
                    super::Store { started, ..store }.finish(true);
                }
                Ok(Arc::clone(&entry.code))
            }
            Err(error) => Err(error.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheMode;
    use cranelift_codegen::ir::{types, AbiParam, InstBuilder};
    use cranelift_codegen::settings::Configurable;
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    #[test]
    fn undecodable_payload_is_replaced_and_then_reused() {
        let isa = cranelift_native::builder()
            .unwrap()
            .finish(cranelift_codegen::settings::Flags::new(
                cranelift_codegen::settings::builder(),
            ))
            .unwrap();
        let mut function = Function::new();
        function.signature.call_conv = isa.default_call_conv();
        function.signature.returns.push(AbiParam::new(types::I64));
        let mut builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut builder_context);
        let entry = builder.create_block();
        builder.switch_to_block(entry);
        let value = builder.ins().iconst(types::I64, 42);
        builder.ins().return_(&[value]);
        builder.seal_all_blocks();
        builder.finalize();

        // `key` checks upstream parity in test builds, including changes to
        // settings that cannot share a compiled stencil.
        for (name, value) in [
            ("opt_level", "speed"),
            ("regalloc_algorithm", "single_pass"),
            ("enable_verifier", "false"),
            ("preserve_frame_pointers", "true"),
            ("enable_probestack", "true"),
        ] {
            let mut flags = cranelift_codegen::settings::builder();
            flags.set(name, value).unwrap();
            let changed = cranelift_native::builder()
                .unwrap()
                .finish(cranelift_codegen::settings::Flags::new(flags))
                .unwrap();
            key(&*changed, &function);
        }

        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(&CacheMode::Directory(dir.path().to_path_buf())).unwrap();
        let key = key(&*isa, &function);
        // An intact Plenty envelope with an invalid Cranelift payload must
        // still miss. The checksum alone is not a format validator.
        cache.write(&key, Duration::ZERO, b"not a Cranelift entry");
        let mut context = Context::new();
        context.func = function.clone();
        let code = cache.compile(&mut context, &*isa).unwrap();
        assert_eq!(cache.stats().misses, 1);
        let (_, bytes) = cache.read(&key).unwrap();
        let decoded = try_finish_recompile(&function, &bytes).unwrap();
        assert_eq!(code.bytes, decoded.code_buffer());
        context.clear();
        context.func = function;
        assert!(Arc::ptr_eq(
            &code,
            &cache.compile(&mut context, &*isa).unwrap()
        ));
        assert_eq!(cache.stats().hits, 1);
    }
}
