# Plenty + egui: a counter in one UI pass

Read `examples/counter.plenty`. Plenty owns the state, button order, captions,
checked arithmetic and formatted count. Each call to `Ui.button` runs a real
`egui::Ui::button` **now** and returns its `Response::clicked()` as a checked
boolean. `Ui.label` paints the resulting value in the same pass. There is no
message queue, action ID, render plan, shared application library or retained
widget tree on the Plenty side. egui itself tracks interaction/layout history.

## Build and try

This is an external source package, not a std module or automatic `plenty run`
native dependency. Use the compiler with the executable-callback support from
the same checkout (native executable foundation, local PR #1316). Linux
x86-64, Cargo/Rust (upstream MSRV 1.95), a C11 compiler and platform system
libraries are required. The isolated Rust manifest pins
egui/eframe 0.37.0 with embedded default fonts and eframe's `glow` and `x11`
features. No WGPU renderer or native Wayland window backend is built here.
A native window needs working X11, xkbcommon and OpenGL/glutin drivers.

From the repository root, with the locked crates available locally:

```sh
cargo build --offline --locked --bin plenty
cd experiments/packages/egui
make build PLENTY=../../../target/debug/plenty
target/optimized/plenty-egui --headless 20,15 20,15 20,36
make test PLENTY=../../../target/debug/plenty
make smoke PLENTY=../../../target/debug/plenty
```

`--headless [X,Y ...]` runs the same callback inside a real `egui::Context`,
without initializing eframe, a GPU or a display. Up to 64 `u16` coordinate pairs
in logical points each generate pointer move, press, release and four empty
redraw frames. Stdout traces the painted text shapes, not reads of counter
state. The example above clicks Increment twice then Reset. The no-argument
binary opens an interactive window on the caller's display; use the isolated
smoke target instead for automated validation. It needs Weston with the
headless/pixman backend and kiosk shell, Xwayland, xauth, Python Xlib, Pillow
and software GL. No pre-existing display is probed.

`build.py [single-source.plenty] [--fast-compile] [--plenty PATH]` stages
source and bindings under `target/{optimized,fast-compile}`; run one build
at a time per worktree. The optional `--release` controls Rust optimization,
not Plenty's compile mode. Builds disable Cargo networking and require the
locked dependencies in the local cache.

## What crosses the boundary

- `egui.run()` is a process-main-thread, one-shot primitive. The native `main`
  registers that thread and forwards argv to the normal Plenty runtime. The
  Plenty `main` explicitly enters the UI. Wrong-thread, nested and repeated
  entry are rejected before creating another owner. UI externs are not worker
  certified. The two exported callbacks are `create() -> Result[State,
  AllocError]` and `draw(&mut State, u64) -> Result[(), EguiError]`.
- `--emit-object --executable-exports egui_app` emits the checked adapters,
  opaque owner/destructor and C header. `native/app.c` compiles against that
  header; `build.py` **independently pins** the reviewed ownership fingerprint.
  The final linker requires that exact symbol via `--require-defined` even
  with section GC. Changing the state loan to consumption can retain the same
  C prototype but is not compatible. Re-review the whole interface rather
  than copying a new generated hash into the build script.
- `cc` links the Plenty object with **one** Cargo-built static support archive
  containing the runtime, bridge and renderer. The helper selects this
  invocation's exact `plenty_egui` staticlib and ordered native link args from
  Cargo JSON using the same checkout's `scripts/link_native_executable.py`.
  No GPUI, iced, extra runtime, or global CLI link changes. Each Plenty
  profile stores the object and generated header beside the executable.
- A `Ui` wraps a checked `u64`, not a Rust pointer. During the one synchronous
  `draw`, a scoped thread-local slot lends the egui UI to widget calls. A
  fresh non-wrapping token identifies each loan; another thread, a stale
  token or a nested widget invocation cannot acquire it. A dynamic exclusive
  borrow covers each operation; the guard clears the slot on return or Rust
  unwind. Tokens may be stored as inert numbers: this is temporal checking,
  not a cryptographic authority or a sandbox against hostile native code.
- Plenty's UTF-8 adapter lends `(pointer, byte length)` only for that call.
  Reject oversize before dereference, invalid UTF-8, and null with positive
  length. egui takes the text it needs before the call returns. Limits: 4,096
  bytes per text, 65,536 bytes and 256 widgets per pass. Valid-context widget
  failures latch on the pass even if the client ignores the status.
- Both runners use a context configured with `max_passes = 1`. egui normally
  permits a sizing pass to be discarded and the UI function called again
  for one frame. This example forgoes that extra layout pass: state mutation
  is performed only on a real clicked response, not on press, paint, or
  idle redraw. The headless driver requests discard every frame to exercise
  the restriction. The hidden-window logic hook runs no application UI.
- Factory failure transfers no owner. Success is held by a non-Send/non-Sync
  RAII handle whose drop calls the **originating Plenty destructor**. Native
  startup errors and callback failures also release it. A checked draw error
  stops further callbacks and survives native close/diagnostic write errors.
  At `i64.MAX` the failed checked increment does not overwrite `count`.
  Other mutations already performed in a failing pass are **not** rolled back.
- `AllocError` distinguishes out-of-memory and capacity overflow from Plenty
  operations; these are not generic renderer errors. Upstream egui/eframe,
  fonts, GL, Rust infallible allocation and platform internals may abort on
  OOM. Widget panics and unwinding renderer failures are contained on the
  Rust side; traps, aborts and hostile foreign pointers are not recoverable
  checked errors. Exactly-once cleanup is not a process-abort guarantee.

## Attribution and limits

[egui and eframe](https://www.egui.rs/) by Emil Ernerfeldt and contributors
(MIT OR Apache-2.0) implement the immediate-mode UI and native GL integration.
The source experiment vendors no upstream code or fonts. A built binary does
contain third-party code and embedded `epaint_default_fonts` assets: audit the
selected transitive crates and the actual font notices (including Hack,
Ubuntu and the egui icon subset), as well as runtime and platform obligations,
before redistribution. The compiler's notices alone are insufficient. No
release notice bundle, redistribution qualification, broad platform/input or
accessibility contract, or `std.gui` API is claimed. See [validation](VALIDATION.md).
