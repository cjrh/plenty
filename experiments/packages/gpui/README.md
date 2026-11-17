# Experimental Plenty GPUI package

A real [GPUI 0.2.2](https://docs.rs/gpui/0.2.2/gpui/) window, row/column
layout, UTF-8 labels and clickable labels. The counter's state and event logic
are Plenty; Rust owns the window/event loop. This is **not `std.gpui`**, not
an ordinary `plenty run` application, and not a stable widget API. GPUI is a
large, evolving native GUI implementation, not functionality reasonably
reimplemented in a small Plenty module. Neither compiler nor runtime workspace
has acquired a GPUI dependency.

## Build and run

From this repository root:

```sh
cargo build --locked --release --bin plenty
cd experiments/packages/gpui
CARGO_BUILD_JOBS=6 python3 -B build.py
native/target/debug/plenty-gpui
```

Click **Increment** or **Reset**; close the window to shut down. The selected
renderer is reported to stderr for qualification. The initial
window is fixed at 480 × 280 logical pixels, titled `Plenty GPUI`. Counter values
are bounded to 0..1,000,000; increment at the ceiling reports `LimitExceeded`
and exits unsuccessfully, rather than overflowing or hiding a failed action.

Linux GNU x86-64 only. Tested with Plenty 0.8.32, Rust 1.100.0, Fedora 44,
X11/Xwayland and an AMD Vulkan renderer. A working desktop, Vulkan loader/driver,
fontconfig and a sans-serif font are necessary. Build dependencies include a
Rust/C/C++ toolchain, pkg-config, fontconfig/freetype, X11/XCB, xkbcommon **and
xkbcommon-x11**, Wayland and OpenSSL development packages. On Fedora these
include `fontconfig-devel freetype-devel libX11-devel libxcb-devel
libxkbcommon-devel libxkbcommon-x11-devel wayland-devel openssl-devel
vulkan-loader-devel`; consult the pinned GPUI source's Linux dependencies for
other distributions. No system packages are installed by the helper.

`--plenty /absolute/path/to/plenty` selects a matching compiler.
`--fast-compile` changes Plenty compilation; `--release` also optimizes the
Rust launcher (not qualified in the initial measurements). `build.py FILE`
accepts a **single-file** application following `examples/counter.plenty`'s
exports. Its only local imports may be `gpui` and `std`; application module-tree
staging is not supported. Other Plenty projects can depend on this source
package by path for checking, but native linking is always explicit.

The helper stages `gpui.plenty`, its trusted declarations and the application
under `target/optimized/` or `target/fast-compile/`. It builds a **resident Plenty
shared library** with its generated header, then the independent Cargo workspace
in `native/`. The Rust binary has a relative RUNPATH to that application profile;
keep the package output layout when moving it. Do not replace/rebuild the app
library while a runner is live. No `LD_LIBRARY_PATH` is required. There is no
unloading, library discovery or hot reload. The generated header is compiled by
`native/app.c`, so incompatible application exports fail the native build.

A Plenty static library was tested first: its bundled Rust runtime collides
with the host's Rust standard library and allocator. This implementation uses
normal shared-library symbol hiding instead, **not** archive surgery or
multiple-definition suppression. Rust passes three command function addresses
as opaque arguments to each render; no global callback registry is needed.

## API and lifecycle

The application must export these names under library name `gpui_app`:

- `create() -> Result[State, AllocError] = "gpui_app_create"`.
- `render(state: &State, token: u64, begin_function: gpui.Function,
  end_function: gpui.Function, text_function: gpui.Function)
  -> Result[(), gpui.GpuiError] = "gpui_app_render"`.
- `event(state: &mut State, action: u64) -> Result[(), gpui.GpuiError]
  = "gpui_app_event"`.

`State` is the owned exported Plenty class; Rust calls its generated originating
destructor exactly once after callback cessation. The shared library stays
loaded until process exit. There is no Rust/GPUI object-layout exposure.
All entry, callbacks and destruction are serialized on the launch/main thread.
Render has a shared loan, event an exclusive loan; neither reenters the other.
A click uses an action ID from the last valid scene, ends its Plenty mutation,
then calls `cx.notify()` to render again. Do not give callbacks thread-safety
annotations or move their state to workers/actors. Bridge calls are not worker
certified; opaque function addresses are thread-bound.

Construct `gpui.Frame(token, begin_function, end_function, text_function)?`
inside `render` only. Null addresses/zero token are rejected before indirect
calls. Non-null function addresses must be the exact host-provided prototypes;
opaque pointers are a trusted ABI contract, not arbitrary-address validation.
The host code is resident for every call. Do not store frames/addresses in app
state or call after render. Stale tokens, nested frames and wrong-thread calls
are rejected; tokens are never reused (exhaustion is `CapacityOverflow`).

`row(gap=8.0f32, padding=8.0f32, background=0x202020u32)` and `column(...)`
begin containers; `end()` closes one. `text(&label, color=0xffffffu32,
size=20.0f32)` adds a label; `button(&label, action, color=..., size=...)` adds
a clickable div, **not** a fully accessible/keyboard-operable button widget.
Build one root, balance containers, and propagate errors. Button action IDs are
nonzero and unique per frame. Text is borrowed and copied before return,
including embedded NUL; invalid raw UTF-8 is rejected, not truncated.

Limits per frame: 256 nodes (containers included), depth 16 containers,
4096 bytes per label, 65536 total UTF-8 bytes. Colors are 24-bit RGB; gap/padding
are finite 0..1024 pixels and text sizes finite 1..128 pixels. Native builder
errors poison and discard the whole frame even if ignored. Callback failures
stop further callbacks, report to stderr, exit unsuccessfully and release state;
mutated app state is not rolled back. Close is deferred one event-loop turn to
avoid GPUI 0.2.2's X11 inline-close borrow panic.

`GpuiError` distinguishes `OutOfMemory`, `CapacityOverflow`, `InvalidContext`,
`LimitExceeded`, `InvalidLayout`, `InvalidArgument`, `NativeFailure`.
`gpui.allocation` preserves the two `AllocError` causes for f-string failures.
The command builder uses fallible reserve, but **GPUI/Rust upstream allocations
can abort, native panics outside command entry and Plenty traps remain fatal**.
No Rust unwind may cross Plenty frames; native command entry catches unwinds.
Do not run this bridge under actor/arena abandonment or promise recoverable OOM.

No text editing/IME contract, async Plenty integration, images, menus, multiple
windows, portability, accessibility guarantees or production packaging is
provided by this slice. Outstanding candidates belong only in the book backlog.

## Validation

```sh
make test           # six Rust boundary tests + real Plenty shared-library ABI tests
make fmt
make clippy
make smoke          # real X11/Xwayland desktop, libX11/libXtst, ImageMagick import
```

See [the measured qualification record](VALIDATION.md) for actual results and costs.

The deterministic suite checks copied UTF-8/NUL/empty text, limits and malformed
input, thread/phase/stale tokens, callback errors, original Plenty destruction,
null callback configuration, failed factories, package resolution and worker
rejection. Both Plenty compilation modes are tested. It leaves the uninstrumented
counter selected. `native/target/debug/plenty-gpui --headless 1 1 2` runs real
Plenty/native ABI and model tests, **not** a GPUI window or test renderer.

The X11 smoke uses XTest clicks and rendered Count-region pixels, restores the
initial pixels with Reset, then closes via Alt+F4 and checks exit status. It
assumes desktop scale 1 and the default Linux counter decorations/layout, with
bounded click-to-focus retries. Run
on an idle desktop: it changes focus and pointer position. Screenshots/logs go
to `/tmp/plenty-gpui-smoke/`, never the repository. Wayland is compiled, but its
native input/renderer path and other targets are not qualified.

## Dependencies and notices

`native/Cargo.lock` is independent; GPUI is pinned to **`=0.2.2`**. The lock was
seeded from that published crate's lock, then resolved for this host (681 total
entries, including other targets/build tools, not the executed build count).
A fresh newest-version resolution failed: old `xattr 0.2.3` uses `libc::ENOATTR`,
removed in libc 0.2.191. The retained compatible libc is 0.2.177. Do not casually
update this graph. The published GPUI archive SHA-256 is
`979b45cfa6ec723b6f42330915a1b3769b930d02b2d505f9697f8ca602bee707`;
its recorded source revision has `dirty=true`, so the archive is authoritative.

From the repository root, the release notice generator now recognizes the
isolated component:

```sh
python3 -B scripts/release/third_party_notices.py --repo . --component gpui --out /tmp/gpui-notices.txt
```

**Binary redistribution is not qualified.** This command deliberately fails
closed: 31 transitive published crates omit license texts. Their exact names
are reported instead of producing incomplete notices. No binaries or copied
upstream source are distributed in this package. Collect/review those texts
before redistribution; the Plenty application/runtime also needs compiler
component notices. System-library licenses remain relevant to redistribution.

## Acknowledgements

- [GPUI / Zed Industries](https://gpui.rs/), the actual window/layout/text/input
  implementation, via the published `gpui` 0.2.2 crate; Apache-2.0. No upstream
  implementation source was copied into this package.
