# Building a GUI package from source

Choose [iced update/view](gui-iced.md) **or** [egui immediate mode](gui-egui.md).
Import the module in an executable's source graph, including through a package
dependency. The package needs no GUI dependency stanza. `plenty check` checks
the Plenty code without Cargo. `plenty build` builds and links one selected
backend; importing both in one executable is an error, even if one import is
unused. A CLI executable with neither import uses the ordinary bundled runtime.

## Source installation and host

Use a **trusted matching checkout** on x86-64 GNU/Linux. Keep the source-built
compiler under that checkout's `target/` and keep its sources in place. In
particular the two `experiments/packages/{iced,egui}/native/` manifests, locks,
reviewed headers and bridge sources, `plenty-runtime/`, and
`scripts/link_native_executable.py` are toolchain inputs, not files to put in
an application. Binary-only compiler archives and copied standalone compilers
do not install this support. After updating the checkout, rebuild the compiler.
Do not mix a compiler, runtime or bridge from different revisions.

Build prerequisites: Rust and Cargo (tested with Rust 1.100), Python 3.9+,
C11 compiler, archiver, `pkg-config`, glibc development files and a `cc`
linker supporting `--require-defined` (GNU ld or compatible mold). Provision
the locked compiler dependencies including its Git forks and **the chosen**
backend's locked registry dependencies in the Cargo cache. GUI builds run
Cargo offline, including the first build; they do not install system packages.
For a window use X11/Xwayland, libX11, libxkbcommon/X11 and fonts for iced;
egui additionally needs working OpenGL/glutin (EGL or GLX) drivers. No
hardware graphics or display is required for the counters' headless runs.

From the source root:

```sh
export CARGO_BUILD_JOBS=3
cargo build --locked --offline --bin plenty --target-dir target
export PATH="$PWD/target/debug:$PATH"
(cd examples/gui/iced_counter && plenty build --locked && target/optimized/iced_counter --headless 1 1 2)
(cd examples/gui/egui_counter && plenty build --locked && target/optimized/egui_counter --headless 20,15 20,15 20,36)
```

In either package `plenty run` opens the window; `plenty build --fast-compile`
produces `target/fast-compile/NAME`. Both modes use the same native Cargo dev
archive. The package command rechecks the chosen native graph with Cargo on
every build and relinks (the package source fingerprint alone cannot certify
a native archive). Cargo artifacts are under the **compiler checkout's**
`target/std-gui/{iced,egui}/`; deleting one of those directories forces a cold
native build. Do not delete live lock files, clean these directories, or use
them as target directories for independent Cargo commands during a build.
Plenty serializes builds sharing a backend cache from Cargo selection through
final driver consumption. A separate per-target lock covers the package
freshness check through binary and fingerprint publication, including builds
of separately selected workspace members sharing a binary name. Builds of
different targets using different backends (or CLI targets) can overlap.
This doesn't reserve a shared executable name after `build` returns.

The Cargo invocation explicitly targets `x86_64-unknown-linux-gnu`, ignoring a
conflicting Cargo default target. GUI package links reject compilers built
with `runtime-checks` (including `--all-features`) before invoking Cargo. That
instrumentation checks every process allocation against a startup baseline;
third-party GUI caches can retain memory until process exit. Use the normal
compiler build above. The build never silently substitutes an unchecked
runtime for a checked one. Native build inputs include
the installed Cargo/Rust/C toolchains, their configuration and environment
(e.g. `RUSTFLAGS`, target-specific rustflags, `CC`, `CFLAGS` and `AR`). Use
baseline-compatible flags; the lock fixes dependency versions, not native
bytes or CPU features selected by those tools. Plenty's normal per-function and program-object caches apply.
Cargo reports the staticlib and ordered system libraries, not a filename
guessed by scanning the target directory. The final link uses one archive
with the bridge, renderer and executable runtime, never a second runtime.

## Compatibility and limits

Use the callbacks, names and signatures in the [counter lesson](../../tutorial/special-purpose/gui-counters.md).
The bridge compiles against a fixed C header from the reviewed interface. The
app object supplies checked adapters, a type-specific destructor, and its
[ownership contract guard](../facilities/04-interop/contracts.md#compatibility-fingerprints).
The final link independently **requires the reviewed guard**: identical C
prototypes are not sufficient. A change from `&mut State` to `State` or even
an extra unrelated export no longer satisfies the contract. Never update the
pin from the input being linked. Changing the private model or callback bodies
need not change the contract. This is compatibility, not a native-code sandbox.

This is a baseline-CPU **package executable** build, not a script or
single-file launcher, test-hook/fuzz target, cross-build, plugin mechanism,
fully static binary, or distribution format. Neither GUI toolkit ships in
the compiler binary. A built application contains third-party code and may
embed assets; release compiler notices are **not** sufficient. Audit the
actual locked native graph, runtime, assets and platform obligations before
redistribution. The source audit entry points are
`scripts/release/third_party_notices.py --repo . --component iced --out NOTICES`
and `--component egui`. They check Cargo metadata with the `application`
feature and fail closed when a crate omits a reviewed license text. Metadata
can include optional crates absent from the actual build. Neither inventory
is a qualified GUI notice bundle; the release archives do not include these
components.
