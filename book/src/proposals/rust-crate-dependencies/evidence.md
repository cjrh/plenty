# Rust rlib research and probe evidence

Companion to [Rust crate dependencies](../rust-crate-dependencies.md).
Checked on 2026-10-23 against Plenty `d7332b6c` and primary Rust documentation.
Compiler-private documentation follows nightly and is not a stable API promise.

## Evidence

| Finding | Source and limit |
| --- | --- |
| rlib carries metadata and code | [Compiler guide](https://rustc-dev-guide.rust-lang.org/backend/libs-and-metadata.html) and archive probe. Member names are implementation details. |
| rustc can link Rust dependencies and foreign objects | [Linkage reference](https://doc.rust-lang.org/reference/linkage.html#mixed-rust-and-foreign-codebases). Passing an object does not establish call ABI. |
| Generic calls require compiler participation | [Monomorphization](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html) and downstream generic probe. |
| Rust ABI requires version discipline | [ABI reference](https://doc.rust-lang.org/reference/items/external-blocks.html#abi) and [FnAbi](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/struct.FnAbi.html); Cranelift integration remains unproven. |
| Plenty's embedded runtime is not an rlib dependency | `build.rs::build_runtime`, `src/codegen.rs::link_object`, `src/toolchain.rs::link`, and [runtime reference](../../design/toolchain/02-architecture/runtime.md). |

## Executed probes

Environment: `rustc 1.99.0 (b940084d7 2026-09-28)`, full commit
`b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`, LLVM 23.1.1,
`x86_64-unknown-linux-gnu`. These equivalent sources/commands reproduce the
experiment. Research supplied source through rustc stdin and used temporary
outputs; no repository Rust implementation was changed.

`probe.rs`:

```text
pub fn id<T>(x: T) -> T { x }
#[unsafe(no_mangle)]
pub fn probe_add(x: u64) -> u64 { x + 1 }
```

```sh
rustc --edition=2024 --crate-name=probe --crate-type=rlib \
  -C panic=abort -C opt-level=0 -o libprobe.rlib probe.rs
ar t libprobe.rlib
nm --defined-only --demangle libprobe.rlib
```

Observed members: `lib.rmeta`, `lib.rmeta-link` and one `.rcgu.o`. Defined symbols
included `probe_add`, but no `probe::id`: the library requests no concrete generic
instance. Do not hard-code this archive structure.

`consumer.rs`:

```text
extern crate probe;
fn main() {
    assert_eq!(probe::id(String::from("ok")), "ok");
    assert_eq!(probe::probe_add(41), 42);
}
```

```sh
rustc --edition=2024 --crate-name=consumer --extern probe=libprobe.rlib \
  -C panic=abort -o consumer consumer.rs
./consumer
```

Compilation/linking and both assertions passed. The downstream Rust compilation
instantiated the generic with owning `String` and its ordinary Rust cleanup.

`direct.rs`:

```text
unsafe extern "Rust" { fn probe_add(x: u64) -> u64; }
fn main() { assert_eq!(unsafe { probe_add(41) }, 42); }
```

```sh
rustc --edition=2024 --crate-name=direct -C panic=abort \
  -C link-arg=libprobe.rlib -o direct direct.rs
./direct
```

This also passed without `--extern`; the linker extracted an ordinary symbol.
**rustc generated both sides of this call.** It proves neither compatibility
with C/Plenty callers nor general raw-rlib linker support, generic instantiation
or dependency closure. Rust's documented unsupported status still applies to
general foreign-linker consumption.

## Limits

No compiler helper, Cranelift-to-Rust adapter, Cargo project integration, runtime
rebuild, third-party crate qualification or cross-version matrix was built.
There are no Plenty ownership/unwind or performance results here. The next
decisive probe must use a real Plenty/Cranelift caller and compare its lowering
against the pinned rustc for scalars, pointers, returns and output slots at
debug/release optimization. Then test one complete runtime/dependency graph.
