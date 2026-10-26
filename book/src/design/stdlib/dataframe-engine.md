# Install the dataframe engine

The ordinary compiler build does not compile Polars. Importing
`std.dataframe` can be type-checked without its engine. Linking needs the
engine only when retained native code reaches a dataframe operation,
including calls through dependencies. An unused import or unused local
helper does not add a Polars dynamic dependency.

## Install a prebuilt release engine

Download `plenty-dataframe-VERSION-x86_64-unknown-linux-gnu.tar.gz` and its
`.sha256` file from the **same Plenty release** as your compiler. This is a
separate optional release asset; the compiler archive stays independent.
Verify and extract it, then copy the library beside `plenty` and `plentyc`:

```sh
sha256sum -c plenty-dataframe-VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf plenty-dataframe-VERSION-x86_64-unknown-linux-gnu.tar.gz
cp plenty-dataframe-VERSION-x86_64-unknown-linux-gnu/lib/libplenty_dataframe_v1.so \
   /path/to/plenty-VERSION-x86_64-unknown-linux-gnu/bin/
```

Replace `VERSION` and the destination path with your installed release.
Alternatively, point `PLENTY_DATAFRAME_ENGINE` at the extracted library.
Keep the archive's `engine.json`, license files and notices for redistribution.
The manifest records the Plenty version, source commit, target, ABI generation,
library checksum and minimum glibc. It is descriptive metadata, not a loader
configuration or a compiler-enforced version check.

The prebuilt engine supports x86-64 GNU/Linux with glibc **2.28 or later**
and `libgcc_s.so.1`, with no AVX2 requirement. Rust, Cargo and a source checkout
are unnecessary. musl/Alpine and other OS/architecture targets are unsupported.
The compiler still needs its [linking prerequisites](../toolchain/01-building/installing.md).
See [shipping dataframe applications](dataframe-deployment.md) for runtime
library placement, notices and compatibility checks.

## Build from source

From a Plenty source checkout, with a stable Rust toolchain:

```sh
python3 scripts/build_dataframe_engine.py --out target/release
```

Build the compiler separately with `cargo build --release --locked`.
The script pins Polars 0.55.2 with `lazy`, `parquet`, `csv`, `strings`, `regex`,
`temporal`, `dtype-date`, `dtype-datetime`, `semi_anti_join` and `asof_join`,
uses three Cargo jobs by default, and installs
`libplenty_dataframe_v1.so`. Use
`--target-dir PATH` to reuse engine build artifacts and `--jobs N` to
control build concurrency. It forces baseline `x86-64` code generation
and the system C linker, overriding ambient CPU flags; AVX2 is not required.
The supported target is currently `x86_64-unknown-linux-gnu`.
Build on a system with a compatible glibc version for the deployment host.
This source-build command does not impose the release's glibc 2.28 baseline;
the release workflow builds and audits the engine inside its pinned container.

The extra string features support the native expression API. Polars 0.55.2's
string dispatch imports its optional time module even without string-to-date
operations, so this feature combination also needs `temporal` to compile.
The date/datetime dtype features support [temporal columns and expressions](dataframe-datetime.md).
Checked parsing and calendar validation also use the pinned Chrono dependency.
Ordinary compiler/runtime builds still do not build Polars.
The `parquet` feature already enables Polars' streaming engine; CSV support
adds only `csv`, not a second execution engine. The `semi_anti_join` feature
supports native membership joins; `asof_join` supports checked integer-key
as-of matching. Temporal and grouped as-of APIs are not exposed.

## Discovery and compatibility

The dataframe engine follows the general [native engine contract](../toolchain/01-building/engines.md).
The compiler looks beside its own executable, or at the file named by
`PLENTY_DATAFRAME_ENGINE`. An explicit environment path takes precedence.
The Rust compilation API also offers `CompileOptions.dataframe_engine`.
Missing engines produce an installation hint at link time.

```sh
export PLENTY_DATAFRAME_ENGINE=/opt/plenty/libplenty_dataframe_v1.so
plenty --compile analysis.plenty -o analysis
```

The resulting executable contains a versioned shared-library dependency,
not a copy of the entire engine. Its RUNPATH searches its own directory
and the engine's build-time directory. Deploy the executable beside the
matching `libplenty_dataframe_v1.so`, or install that library in the
loader's search path. Keep compiler bindings and engine from the same
Plenty release; the `v1` name denotes the C ABI generation, not compatibility
with arbitrary future Polars builds. Directly emitted objects also require
this engine when externally linked. Engine directories containing a colon
are rejected because ELF RUNPATH uses that character as a separator.

Rebuild/reinstall the engine when updating the compiler's dataframe API.
New query operations add version-1 symbols; an older engine cannot satisfy
their native links even though its filename is unchanged.
The system loader's `LD_LIBRARY_PATH` and `LD_PRELOAD` can override RUNPATH;
keep stale engine copies out of those paths when switching installations.

Normal programs need neither this library nor Rust. They do not gain a
`DT_NEEDED` entry for Polars. A project fingerprint includes the selected
engine path and metadata, so replacing or relocating it triggers relinking.
Already-linked executables still need their dynamic dependencies at run time.

## Development checks

To exercise the optional component after building it:

```sh
export PLENTY_DATAFRAME_ENGINE="$PWD/target/release/libplenty_dataframe_v1.so"
export PLENTY_DATAFRAME_TESTS=1
cargo test --locked --test integration test_std_dataframe::
cargo test --locked --test integration test_tutorial::dataframe_examples_with_optional_engine
```

Regular tutorial checks type-check dataframe examples without the engine.
The separate dataframe CI job builds it and executes those examples and
the real-engine integration tests. These tests cover bulk transfer, query
plans, nulls, errors and Parquet, as well as absence of the dynamic dependency
from programs that do not use it.
