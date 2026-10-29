# Native engines

An engine is a large native library behind a `std` module that ships outside
the runtime archive, as a separate shared library. Only programs whose
retained code reaches the module link it. [`std.dataframe`](../../stdlib/dataframe-engine.md)
(Polars) and [`std.linalg`](../../stdlib/linalg-engine.md) (faer, issue #490)
are engines today; images (issue #476) is another candidate.

## When a module is an engine

Code in the runtime archive costs nothing to programs that do not reach it:
the linker drops it with `--gc-sections` (see [linking](linking.md#external-build-systems)),
provided only the module's own entry symbols reach it (issue #501). It still
grows the compiler, which embeds the archive, and every compiler build
compiles it. So a native module stays in the runtime archive unless one of
these holds:

1. **Size.** A program reaching one ordinary operation of the module grows by
   more than 2 MB, measured as the stripped executable minus the stripped
   `print("hello")` executable.
2. **Dependencies.** Its crates need a native toolchain beyond rustc and `cc`,
   a system library outside the [release baseline](installing.md), or a
   build that adds more than a minute to a clean release build of the compiler.
3. **Release cadence.** It must be rebuilt or upgraded independently of the
   compiler, for example to follow a fast-moving third-party release.

Measured on 2026-10-26 with stripped release builds:

| Code | Size |
|---|---:|
| `print("hello")` executable | 789,584 bytes |
| plus a `std.tls` client configuration | 2,075,096 bytes (+1.3 MB) |
| runtime archive embedded in the compiler | 15.1 MiB |
| Polars dataframe engine, `libplenty_dataframe_v1.so` | 89,553,952 bytes |

`std.tls`, the largest runtime module, stays in the archive; the dataframe
engine qualifies on size and build time. A new wrapper's issue or PR records the
same measurement before choosing.

## Shared libraries, not static archives

Engines are shared libraries. A separately supplied static archive, passed to
the linker only when reached, was considered and rejected:

- **Linking.** An engine written in Rust carries its own copy of Rust's
  standard library, allocator shims and panic runtime. The runtime archive
  already defines these and installs Plenty's global allocator, so two Rust
  static archives in one link either collide or must be built as one crate
  graph, which is the runtime archive again. A shared engine keeps its copy
  private and exports only its facade symbols.
- **Executable size.** A static engine adds the code each program reaches.
  Large engines dispatch internally (query planners, codec tables), so a
  program reaching one operation retains most of the engine, in every
  executable. A shared engine adds only a `DT_NEEDED` entry and the imported
  symbols, and one copy serves every program on the host.
- **Deployment.** The cost of a shared engine: the application ships the
  library beside the executable ([distribution output](distributions.md) automates this).
- **Reproducibility.** Under the [reproducibility contract](linking.md#reproducible-artifacts)
  an executable that reaches an engine also depends on the engine's directory,
  which ordinary output records in `RUNPATH` (distribution output uses only
  `$ORIGIN`), and on the facade's symbols, but not on the
  engine's bytes. A static engine would make every executable depend on the
  engine archive's bytes, which would then need the runtime's deterministic
  build too.

Revisit static archives only for an engine with no Rust standard library or
allocator of its own.

## The engine contract

A second engine follows `std.dataframe` in each of these.

| Part | Rule |
|---|---|
| Facade | `stdlib/std/NAME.plentyi`, a trusted interface whose `extern def` declarations name the engine's C symbols. Plenty code in the facade is ordinary `std` code. |
| Symbols | Every export starts with a prefix unique to the engine and its ABI generation, such as `pldf_v1_`. The library exports nothing else. |
| ABI generation | `libplenty_NAME_vN.so`, also its SONAME. Adding an operation adds symbols and keeps `N`. Changing or removing a symbol, a layout or an ownership rule starts `N+1`, with a new prefix and file name, so existing executables keep loading `vN`. |
| Reachability | The compiler follows the object's relocations from its roots, as `--gc-sections` does. An unused import, unused helper or unused facade function needs no engine and adds no `DT_NEEDED`. |
| Discovery at compile time | `CompileOptions` field, then `PLENTY_NAME_ENGINE`, then `libplenty_NAME_vN.so` beside the compiler executable. Ordinary output rejects `:` in the engine directory (an ELF RUNPATH separator). Build fingerprints include the selected path, size and modification time. |
| Discovery at run time | `RUNPATH` of `$ORIGIN`, then the engine's directory at link time for ordinary output; [distribution output](distributions.md) uses only `$ORIGIN`. The loader's `LD_LIBRARY_PATH` and system configuration also apply. |
| Missing engine | Linking fails, naming the module, the path tried, how to build or install the engine, and the environment variable. Type-checking never needs the engine. |
| Mismatched engine | An older engine lacking a symbol fails at link time (`undefined reference to PREFIX...`) or at load time (`symbol lookup error`). Release packaging rejects an engine whose exports do not cover the facade's declarations. A different `N` is a different file and is never substituted. |
| Errors | Each call returns an error code; the engine catches panics at the C boundary and reports them as an error. Handles are freed by the engine that allocated them. Engine allocations are outside Plenty's allocation accounting; the module's reference page states whether allocation failure is recoverable. |
| Build | A separate Cargo workspace, `plenty-NAME/`, with its own `Cargo.lock`, built by a script outside the compiler build, for the baseline x86-64 CPU and the release glibc. |
| Release | A separate archive from the same release, `plenty-NAME-VERSION-TARGET.tar.gz` with a `.sha256`, containing `lib/libplenty_NAME_vN.so`, `engine.json` (Plenty version, source commit, target, ABI generation, library checksum, minimum glibc), `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD-PARTY-NOTICES.md` for the engine's crates, the Rust standard library licences, and a README with the deployment recipe. `--dist` validates metadata and copied-library checksum and includes the notices; ordinary compilation does not read `engine.json`. |
| Tests | The ordinary suite checks that unreached uses link no engine and that a reached use without one reports the missing engine. A separate CI job builds the engine and runs the real-engine tests, gated by `PLENTY_NAME_TESTS=1`. |

## Adding a second engine

The compiler side is a table: an entry in `ENGINES` (`src/toolchain.rs`)
gives the facade module, library, symbol prefix, environment variable and
install hint, and drives reachability, link arguments, the facade's
`.plentyi` resolution and build fingerprints. The `CompileOptions` override
is still a named field per engine.

The shared build and packaging entry points are `scripts/build_native_engine.py`
and `scripts/release/package-engine.py`, each taking an engine name. The old
dataframe entry points remain compatible. Packaging checks facade exports and
the engine SONAME, generates component-specific dependency notices, and includes
an engine-specific deployment README. The native-engine CI matrix builds and
tests each engine separately; engine tests never make normal compiler builds
depend on either engine workspace.
