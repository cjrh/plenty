# GCC backend (`plenty_codegen_gcc`)

> Active tasks and priorities live in the [backlog](../backlog.md). Staging below
> records design dependencies, not scheduled work; some syntax and assumptions
> are historical. See the [implementation status](../design/04-implementation-status.md)
> and [reference](../design/index.md) for current behavior.

Status: planning proposal, 2026-10-07. Nothing in this document is implemented.
API facts were checked against the `gccjit` 7.1.0 crate source and the
libgccjit 17 documentation on 2026-10-07. Line numbers refer to the tree on
that date.

## Goal

Add a second AOT backend that uses GCC, through `libgccjit`, to make native
code. Cranelift stays the default backend. The model is
[`rustc_codegen_gcc`](https://github.com/rust-lang/rustc_codegen_gcc).

```
                              ┌── codegen_cranelift ──> CLIF ──────┐
Op ── op::check ── codegen_ssa┤                                    ├──> .o ──cc──> executable
                              └── codegen_gcc ──> libgccjit trees ─┘
```

## Why

- GCC's optimizer can make faster code, and with `-Os`, smaller code.
- GCC supports more CPU targets than Cranelift.
- It does not change runtime memory use. That comes from the value and heap
  design, not the backend.

## Cost

- **System dependency.** `libgccjit.so` must be installed to use the backend.
  Use the crate's `dlopen` feature (see below) so the default build and the
  default binary do not need it.
- **One target per install.** One `libgccjit` build makes code only for the
  target GCC was built for. To cross-compile, you need a separate libgccjit for
  each target.
- **Slower compiles, more compiler RAM** than Cranelift.
- **Two backends to maintain.** Every new `Op` needs two lowerings, unless you
  first add a shared lowering layer (see Phase 1).
- **License.** The `gccjit` crate and libgccjit are GPL-3.0. See "License".

## License

Not legal advice. This is the common reading.

Plan: license Plenty as `MIT OR Apache-2.0`. The repo has no license file now.

**What works:**

- All of Plenty's own source can be `MIT OR Apache-2.0`. Both licenses are
  compatible with GPL-3.0, so GPL code can sit beside them.
- **User programs are not affected.** The runtime that is linked into compiled
  Plenty programs is Plenty's own MIT/Apache code. Machine code that GCC makes
  is not covered by GCC's license.

**What does not work:**

- A distributed `plenty` binary built with `gcc-backend` includes the `gccjit`
  crate (GPL-3.0). That binary must be distributed under GPL-3.0 terms,
  including an offer of its source.
- `dlopen` does not fix this. It removes the link-time need for
  `libgccjit.so`, but the crate's Rust code is still compiled into the binary.

**Options:**

1. **Keep `gcc-backend` off by default (recommended).** Default binaries stay
   pure MIT/Apache. Only builds with the GCC backend carry the GPL. Say this in
   the README.
2. **Write your own bindings** for the about 40 libgccjit functions you need,
   instead of using the GPL crate. libgccjit itself is still GPL-3.0, and the
   FSF treats dynamic loading as a combined work. This option is weaker than it
   looks.
3. **Emit C and run `gcc` as a separate program.** This avoids the question
   completely, because Plenty does not link GCC. It is not the
   `rustc_codegen_gcc` model.

**Setup:**

- Set `license = "MIT OR Apache-2.0"` in `Cargo.toml` and
  `plenty-runtime/Cargo.toml`.
- Add `LICENSE-MIT` and `LICENSE-APACHE` at the repo root.

## How `rustc_codegen_gcc` does it (the model)

- `rustc_codegen_ssa` holds the shared lowering. It calls builder **traits**
  (`BuilderMethods`, `ConstMethods`, ...).
- Each backend (`_llvm`, `_cranelift`, `_gcc`) implements those traits.
- The `_gcc` crate uses the `gccjit` Rust crate (bindings to `libgccjit`).
- It uses a patched GCC fork. Plenty does **not** need the fork. See "The
  `gccjit` crate" below.

## The `gccjit` crate: facts that save time

Crate: `gccjit = "7.1.0"` (depends on `gccjit_sys` 4.1.0). Repo:
<https://github.com/rust-lang/gccjit.rs>. Docs on docs.rs are built with the
`master` feature, so docs.rs shows functions that you cannot use.

**Features:**

| Feature | Use it? | Why |
|---|---|---|
| `master` | **No** | Needs the `rustc_codegen_gcc` GCC fork. Gates try/catch, attributes, `get_target_info`, `new_temp`, `global_set_readonly`, vector ops, `get_error_count`. Plenty needs none of these |
| `dlopen` | **Yes** | Loads libgccjit at run time with `gccjit::load(c"libgccjit.so.0")`. Without it, the crate links `-lgccjit`, and every build of `plenty` needs libgccjit installed |

**Every API this plan needs is available without `master`:**
`Context::compile_to_file(OutputKind::ObjectFile, ..)`,
`RValue::set_require_tail_call`, `Block::end_with_switch`, `Context::new_case`,
`Function::get_address`, `Context::get_builtin_function`,
`LValue::global_set_initializer` (bytes), `LValue::global_set_initializer_rvalue`,
`Context::new_struct_constructor`, `LValue::set_alignment`,
`Context::add_command_line_option`, `Context::dump_reproducer_to_file`.

**Linkage maps directly** (`gccjit::FunctionType`):

| Cranelift `Linkage` | Used for | `FunctionType` |
|---|---|---|
| `Export` | `plenty_main` | `Exported` |
| `Local` | user functions, resume bodies | `Internal` |
| `Import` | runtime helpers | `Extern` |

**Error handling gotchas:**

- In **debug builds** (this includes `cargo test`), the crate panics after any
  call if `get_last_error()` returns text. Without `master`, it cannot read the
  error count, so it **panics on warnings too**. Do not emit code that makes
  GCC warn. If you must, wrap the GCC backend in `std::panic::catch_unwind` and
  turn the panic into a `Result` error.
- In **release builds**, it does not panic. Check `ctx.get_first_error()` after
  `compile_to_file`, or a failed compile is silent.
- Any call that returns NULL panics in all builds (`panic_on_null`).

**libgccjit rejects unreachable blocks by default.** The Cranelift lowering
makes blocks that have no predecessor (for example the block after a
`TailCall` or after a trap). Call `ctx.set_allow_unreachable_blocks(true)`, or
do not create those blocks.

**Optimization levels:** `OptimizationLevel` has `None`, `Limited`, `Standard`,
`Aggressive` (`-O0` to `-O3`). There is no `-Os` variant. Use
`ctx.add_command_line_option("-Os")`.

**Concurrency:** libgccjit holds a global lock while it compiles. Parallel
tests that use the GCC backend run their GCC compiles one at a time. They are
correct, but slower.

**Assembler:** libgccjit writes assembly and runs the assembler. `as`
(binutils) must be on `PATH`. Plenty already needs `cc`, so this is normally
true.

### Minimum libgccjit version

The libgccjit docs list features by ABI tag, not by GCC version. The tags this
plan needs:

| ABI tag | Adds | Needed for |
|---|---|---|
| `LIBGCCJIT_ABI_3` | `end_with_switch`, `new_case` | Generator resume dispatch |
| `LIBGCCJIT_ABI_6` | `set_bool_require_tail_call` | Mandatory TCO |
| `LIBGCCJIT_ABI_9` | `function_get_address` | Resume and drop callbacks |
| `LIBGCCJIT_ABI_14` | `global_set_initializer` (bytes) | String literals |
| `LIBGCCJIT_ABI_19` | `global_set_initializer_rvalue`, struct/array constructors | Metadata blobs with pointers |
| `LIBGCCJIT_ABI_20` | `INT128_T` / `UINT128_T`, `type_get_size` | Inline sums (`I128`) |
| `LIBGCCJIT_ABI_24` | `lvalue_set_alignment` | `set_align(8)` on data |

So the minimum is **`LIBGCCJIT_ABI_24`**. With `dlopen`, `gccjit::load` fails
if the library lacks **any** symbol that the non-`master` crate binds, so the
crate may need a newer tag than this. Check what is installed:

```sh
objdump -T /usr/lib64/libgccjit.so.0 | grep -o 'LIBGCCJIT_ABI_[0-9]*' | sort -uV | tail -1
```

## Current state (what the GCC backend must match)

The Cranelift backend is about 2,840 lines:

| File | Lines | Contents |
|---|---|---|
| `src/codegen.rs` | 2029 | Driver, runtime declarations, functions, `Lowerer` |
| `src/codegen/enums.rs` | 313 | Sum types |
| `src/codegen/generators.rs` | 231 | Resumable bodies |
| `src/codegen/collections.rs` | 134 | Collection ops |
| `src/codegen/metadata.rs` | 133 | Static type metadata blobs |

Where to look first:

| What | Location |
|---|---|
| Entry point for all AOT builds | `compile_ops_to_executable`, `src/codegen.rs:156` |
| Object emission, passes 1–3 | `compile_to_object`, `src/codegen.rs:212` |
| ISA flags | `host_isa`, `src/codegen.rs:273` |
| Runtime helper table (34 imports) | `Runtime` / `declare_runtime`, `src/codegen.rs:293`, `:346` |
| User function signature | `user_fn_signature`, `src/codegen.rs:528` |
| Plenty type to machine type | `clif_type`, `src/codegen.rs:836` |
| Trap sequence | `trap_if`, `src/codegen.rs:1477` |
| Tail call | `lower_tail_call`, `src/codegen.rs:1858` |
| Resume signature, `func_addr` | `src/codegen/generators.rs:12`, `:72` |
| Data with pointers | `define`, `src/codegen/metadata.rs:9` |

Features it uses, and the libgccjit equivalent:

| Cranelift feature | libgccjit equivalent | Notes |
|---|---|---|
| Object file output | `compile_to_file(OutputKind::ObjectFile, ..)` | Link step with `cc` stays the same |
| Runtime calls (SystemV) | `new_function(.., FunctionType::Extern, ..)` | Same `plenty-runtime` archive |
| Compile-time stack of SSA values | Store each pushed value in a **local** | See "rvalues are trees" |
| `CallConv::Tail` + `return_call` | `new_call(..)` + `set_require_tail_call(true)`, then `end_with_return` | **Highest risk.** See Risks |
| **Multiple return values** (`sig.outputs` is a list) | One struct type per distinct output list; return the struct | C has one return value. Cache struct types by output list |
| `Variable` | `Function::new_local` | Direct match |
| Block params at `match` join | One local per param; assign before `end_with_jump` | GCC's SSA pass removes them |
| `brif`, `jump` | `end_with_conditional`, `end_with_jump` | Direct match |
| Generator resume dispatch | `end_with_switch` + `new_case` | Continuation index becomes a case value |
| `func_addr` (resume, `drop_callback`) | `Function::get_address` | |
| `*_overflow` checked arithmetic | `get_builtin_function("__builtin_add_overflow")` etc. | Type-generic. If it fails, see Risks |
| `trap` after trap helper | Call helper, then `__builtin_unreachable` | Helper does not return |
| `select` | Two blocks and a local | No ternary rvalue without `master` |
| `fcvt_to_sint_sat` / `_uint_sat` | Explicit NaN check, compares, and clamps | A plain C cast is undefined when out of range |
| `I128` (inline sums) | `new_c_type(CType::UInt128t)` | Must match Rust `u128` ABI in the runtime |
| Stack slots | Local array or struct + `LValue::get_address` | |
| Static string data | `u8` array global + `global_set_initializer` | |
| Data that points to other data | Struct global + `new_struct_constructor` + `global_set_initializer_rvalue` | See "Metadata blobs" |
| `set_align(8)` | `LValue::set_alignment(8)` | |
| `is_pic`, `preserve_frame_pointers` | `add_command_line_option("-fPIC")` | Frame pointers are a Cranelift tail-call need only. Confirm in Phase 0 that the object links as a PIE |

### rvalues are trees, not SSA values

In Cranelift, a `Value` is computed once. You can use it many times.

In libgccjit, an `rvalue` is an expression tree. If you use the same rvalue
twice, GCC computes it twice, and side effects (such as runtime calls) happen
twice. Rule for the GCC backend: **assign each pushed value to a new local, and
push the local.** This is how `rustc_codegen_gcc` handles it too. GCC's
optimizer removes the extra locals.

### Metadata blobs

`metadata.rs` makes each blob as bytes plus a list of `(offset, DataId)`
pointer slots. To build the same blob in libgccjit:

1. Sort the pointer offsets.
2. Make a struct type. Bytes between pointer slots become `u8[N]` fields.
   Each pointer slot becomes a `void *` field.
3. Build a `new_struct_constructor` with array constructors for the byte
   fields (one rvalue per byte: a string literal cannot fill a `char` array)
   and `get_address` of the target global for each pointer field.
4. Set alignment 8.

Phase 1 should make the bytes-plus-slots form the backend-neutral output of
`metadata.rs`, so both backends use the same layout code.

### Undefined behaviour

GCC optimizes on C undefined behaviour. Cranelift does not. Rules:

- Do wrapping arithmetic on unsigned types, then cast back.
- Check shift amounts before you shift.
- Add `-fno-strict-aliasing` with `add_command_line_option`. The runtime and
  the generated code read the same memory through different types.

## Phases

### Phase 0: spike (small)

Goal: prove the toolchain works before you change the design.

1. `sudo dnf install libgccjit libgccjit-devel`. Neither is installed now.
   System GCC is 16.2. Run the `objdump` command above.
2. In a scratch crate with `gccjit = { version = "7.1", features = ["dlopen"] }`:
   - write an object file with one exported `plenty_main`,
   - call one runtime helper (for example `plenty_print_str`),
   - link it with `libplenty_runtime.a` through `cc`. Copy the archive from
     `target/debug/build/plenty-*/out/`.
3. Test mandatory tail calls, at `-O0` and `-O2`:
   - self tail call, mutual tail call,
   - a callee with **more** arguments than the caller (more than 6 integer
     arguments on x86-64, so some go on the stack),
   - a callee that returns a struct larger than 16 bytes (returned in memory),
   - a callee that takes or returns `unsigned __int128`.
4. Test `__builtin_add_overflow` through `get_builtin_function` on `u8`, `i32`,
   and `i64`.
5. Test a global whose initializer holds the address of another global.
6. Build in **debug** mode and confirm that no step panics on a warning.

Exit: a short note on which features work, and which need a fallback.

### Phase 1: backend seam (large, no behaviour change)

Goal: split the shared lowering from the Cranelift-specific code, like
`rustc_codegen_ssa`.

1. Move backend-neutral work out of `codegen.rs`:
   - `collect_user_fns`, `collect_str_ids`, `needs_local_addresses`,
   - the runtime helper table (names and signatures as plain data),
   - metadata blob layout (bytes plus pointer slots).
2. Define builder traits for what `Lowerer` needs. Start from the feature table:
   constants, arithmetic, checked arithmetic, compares, casts, calls, tail
   calls, blocks, branches, switch, locals, memory load/store, data addresses.
3. Make `Lowerer` generic over the builder. Implement the traits for Cranelift.
4. All existing tests must pass with no changes.

Design notes:

- gccjit handles carry a `'ctx` lifetime (`RValue<'ctx>`, `Block<'ctx>`). Give
  the traits a lifetime parameter, as `rustc_codegen_gcc` does with
  `CodegenCx<'gcc, 'tcx>`. Do this in the first version of the traits, or you
  must change every signature later.
- Model "value" in the trait as something that a backend can read many times
  with no side effects. Cranelift returns its SSA `Value`. GCC returns a local.
- Model block params as "declare join values, assign, jump". Cranelift maps
  this to block params; GCC maps it to locals.

This is the largest phase. It is also useful on its own: it makes new `Op`
lowerings easier to write and review.

Alternative: skip Phase 1 and copy `Lowerer` into a GCC version. This is faster
to start, but every later `Op` change needs two edits that can drift apart. Use
this only if you want a throwaway experiment.

### Phase 2: GCC backend, core (medium)

Put it in `src/codegen/gcc/` behind a Cargo feature `gcc-backend` (optional
dependency on `gccjit` with `dlopen`).

Order of work, smallest first:

1. Integers, booleans, `Display` of numbers, `plenty_main`, exit status.
2. User functions, calls, multiple returns, tail calls.
3. `match`, patterns, join locals.
4. Strings and static data.
5. Checked arithmetic and traps.
6. Floats and saturating casts.

Crate layout note: a separate `plenty-codegen-gcc` workspace crate would need
`Op` and `Ty` from `plenty`, and `plenty` would need the backend. Cargo does not
allow that cycle. Use a module behind a feature now. Split out a `plenty-core`
crate later only if you need it.

### Phase 3: GCC backend, full coverage (medium)

1. Sum types (`enums.rs`), including `I128` inline sums.
2. Collections.
3. Metadata blobs with pointers.
4. Generators (switch-based resume, `get_address` for callbacks).

### Phase 4: CLI and tests (small)

- CLI: `--backend cranelift|gcc`. Default `cranelift`. If the binary was built
  without `gcc-backend`, or `gccjit::load` fails, give a clear error.
- Tests: all 20 test files that compile programs reach
  `compile_ops_to_executable` (`src/codegen.rs:156`), most through
  `tests/support/mod.rs`. Read a `PLENTY_BACKEND` env var there, and one
  change runs the full AOT suite on GCC:
  `PLENTY_BACKEND=gcc cargo test --features gcc-backend`.
- Run the GCC suite at `-O0` and `-O2`. Undefined behaviour bugs show only at
  `-O2`.
- CI: add one job that installs libgccjit and runs the command above.
- Docs: update `CLAUDE.md` (pipeline diagram, build table) and the AOT sections
  of `DESIGN.md`.

### Phase 5: options (optional)

- `-O0` / `-O2` / `-Os` flag. Use `set_optimization_level` for `-O0`–`-O3` and
  `add_command_line_option("-Os")` for size.
- Debugging: `dump_reproducer_to_file(path)` writes a C program that rebuilds
  the same context. `set_dump_code_on_compile(true)` prints the assembly.
  `set_dump_initial_gimple(true)` prints GCC's view of your trees. Put these
  behind an env var such as `PLENTY_GCC_DUMP`.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| GCC cannot always do a required tail call. On x86-64 SysV, a callee with more stack arguments than its caller fails. Cranelift's `Tail` convention handles this; the C ABI does not. | Compile error for valid Plenty programs | Phase 0 test. If it fails: pass arguments through a pointer to a frame struct when they do not fit in registers, or turn tail calls into a loop with a dispatcher (trampoline) |
| Tail calls to functions with multiple returns. A struct return larger than 16 bytes goes through a hidden pointer, and GCC may refuse the tail call | Compile error | Phase 0 test. If it fails: pass the caller's return pointer explicitly as a parameter |
| `__builtin_*_overflow` does not work through `get_builtin_function` | No checked arithmetic | Use the typed builtins (`__builtin_sadd_overflow`, `__builtin_saddl_overflow`, `__builtin_uaddll_overflow`, ...). For 8- and 16-bit types, compute in 32 bits and check the range |
| Installed libgccjit is older than `LIBGCCJIT_ABI_24` | No metadata alignment; `gccjit::load` fails | Check with `objdump` in Phase 0. Show the required tag in the load error |
| Debug-build panics on GCC warnings | Test crashes with a GCC message | Fix the code that causes the warning; `catch_unwind` at the backend boundary |
| Undefined behaviour in generated trees | Wrong code only at `-O2` | Rules above; run the suite at `-O0` and `-O2` |
| `I128` ABI mismatch with the Rust runtime | Corrupt sum values | Phase 0 test with a runtime call that takes and returns `u128` |
| Two backends drift | Test failures on one backend only | Phase 1 seam; run all AOT tests on both backends |

## Effort

| Phase | Size | Note |
|---|---|---|
| 0 Spike | Small | A few evenings. It decides if the plan works |
| 1 Seam | Large | Touches all 2,840 backend lines; no new features |
| 2 Core | Medium | |
| 3 Full | Medium | Generators and metadata are the hard parts |
| 4 CLI/tests | Small | One env var covers the whole suite |

The total is about the same as writing the Cranelift backend again. Phase 1
returns some of that cost, because each later backend or `Op` is cheaper.

## Open questions

1. Do you want Phase 1 (shared lowering), or a separate copy of `Lowerer` for
   an experiment?
2. Which license option (see "License") do you want for GCC-enabled builds?
3. Is cross-compilation a goal? If yes, the "one target per libgccjit" limit
   matters, and you need a separate libgccjit for each target.
4. Should the GCC backend be in the default CI, or in an optional job only?
