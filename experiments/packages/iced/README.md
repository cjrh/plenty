# iced counter experiment

[The counter](examples/counter.plenty) runs as **one Plenty executable**. Plenty
creates the model, renders the `Count:` label and the column/row of Increment
and Reset buttons, checks the update, and handles the result of the event loop.
Rust translates the submitted tree to real iced widgets. There is no counter
model, action switch for Increment/Reset, or counter-specific view in Rust.

## Try it

Linux GNU x86-64, matching Plenty compiler and runtime sources (tested with
0.8.32 on Fedora 44), Python 3, C/Rust toolchains, cached locked crates, system
fonts and working X11/xkbcommon libraries. No system packages are installed.
From repository root:

```sh
export CARGO_BUILD_JOBS=3
cargo build --offline --locked --release --bin plenty
cd experiments/packages/iced
python3 -B build.py
# Deterministic ABI/model run; no display or input:
target/optimized/plenty-iced --headless 1 1 2
# To open the real window interactively on your own X11 display:
target/optimized/plenty-iced
```

Click Increment or Reset; closing the window returns to Plenty main. Increasing
at 1,000,000 returns `LimitExceeded`, not a wrapped or silently unchanged value.
`--plenty PATH` selects a matching compiler. `build.py --fast-compile` stages
under `target/fast-compile/` instead. Both Plenty modes use Cargo's dev archive
by default; `--release` selects a release archive. `build.py FILE` expects a
single-file application with the exact reviewed `iced_app` export contract; it
stages the file and this package's two binding modules, not arbitrary application
module trees.
`plenty check` works with a local path dependency, but `plenty run` does not
select/build the support archive automatically.

## Where the boundary is

- `build.py` emits `application.o` and `iced_app.h` via `--executable-exports
  iced_app`. Cargo builds [the isolated staticlib](native/Cargo.toml) with the
  normal executable runtime, header-checked C shim, Rust bridge and iced. The
  [same-tree linker](../../../scripts/link_native_executable.py) uses Cargo's
  matching staticlib and rustc's reported system libraries in order. No
  shared app library, Rust launcher, archive extraction, or special linker
  concessions. This is not a fully static Linux distribution.
- [build.py](build.py) passes the [pinned contract guard](native/expected-contract.txt)
  to the final linker with `--require-contract`. The [C shim](native/app.c)
  also references that pin before creating the owner. Unlike C pointer prototypes, this symbol
  distinguishes an exclusive loan from transfer of ownership: changing
  `event(state: &mut State, ...)` to `event(state: State, ...)` fails the
  final link. It checks the [whole generated interface](../../../book/src/design/facilities/04-interop/contracts.md#compatibility-fingerprints),
  not just callbacks used by iced: even an unrelated additional export is
  rejected. Private state or function body changes do not by themselves
  change the guard. If the interface changes intentionally, review the
  generated contract against the bridge's ownership/error assumptions before
  updating the pin; do not derive the expected guard from each build's input.
  This checks compatibility, not the trustworthiness of native code.
- Native `main` records the process main thread before delegating to the normal
  runtime. Plenty `main` calls `iced.run()` synchronously, at most once, even
  after failure. Repeated, recursive and wrong-thread entry fail before the
  factory. Successful `iced_app_create` transfers one opaque `State` owner;
  Rust drops it through the generated `iced_app_State_destroy`, never `free`.
  A failed create transfers nothing.
- `iced_app_render(&State, token)` submits `Frame` calls to separate directly
  linked `row`, `column`, `end`, `text` and `button` entry points. There are
  no app-supplied function pointers. A thread-local builder checks the active
  token; ended frames, nested renders and other threads can't borrow it. A
  token never recycles. UTF-8 spans (including interior NUL) are validated
  and copied before return. One balanced root, at most 256 nodes, 16 open
  containers, 4,096 bytes per label and 65,536 total label bytes per render;
  nonzero action IDs must be unique. Errors on matching-frame operations are
  sticky: no partial tree is published even if Plenty ignores that error.
- iced `view` reads only the last validated Rust-owned snapshot. A button
  sends `Pressed(id)`. `update` checks that ID against the snapshot, calls
  `iced_app_event(&mut State, id)` and **after that call returns**, renders
  again. No widget, listener or task retains a Plenty loan. A callback error
  stops updates and exits the loop; `run()` returns its specific `IcedError`.
  Bridge diagnostics on stderr are best-effort; a write error does not change
  the returned callback status. There is no rollback after mutation. A caught
  upstream panic or native startup error maps to `NativeFailure`. This is not
  recovery from a process abort or a promise that all upstream errors return.

## Validation and evidence

From this package, `make test` runs the Rust boundary/entry suite and both-mode
end-to-end lifecycle fixtures; `make fmt clippy` runs the native checks. For
an existing compiler use `make test PLENTY=/absolute/path/to/plenty`. To test
native input as well:

```sh
make smoke                        # private compositor, real XTest clicks and native close
python3 -B tests/test_counter.py --smoke --plenty ../../../target/release/plenty
```

The smoke requires headless Weston (pixman, kiosk shell), Xwayland, `xauth`,
Python Xlib and Pillow. It creates its **own** compositor and authenticated X
server (no inherited display). It checks that label pixels change twice and
return to the initial pixels, and saves the actual window images under
`target/smoke/`. This uses the real renderer, unlike `--headless`. The optional
lifecycle smoke also verifies the originating Plenty destructor on normal close
and on render failure after a click (`target/{lifecycle,error}-smoke/runner.log`).
Headless fixtures exercise the same dispatch/render path, including checked
arithmetic failure, bad/stale context, startup and factory errors, exactly-once
destruction and recursion/re-entry. Wrong-thread, ignored frame errors, bad
UTF-8, duplicate actions and size/shape limits have direct bridge tests.

## Renderer, credits and limits

[iced 0.14.0](https://docs.rs/iced/0.14.0/iced/) is by Héctor Ramón and the
iced contributors (MIT). The lockfile selects `iced_widget` 0.14.2,
`iced_winit` and `iced_tiny_skia` 0.14.1. The only requested iced features are
`tiny-skia` (software renderer), `x11` and `thread-pool` (iced 0.14 requires
one executor). This small stationary tree needs no GPU, async I/O, native
Wayland, theme loader, image support or scroll performance claim. No wgpu or
GPUI crate in the archive graph. [tiny-skia](https://docs.rs/tiny-skia/0.11.4/)
provides software rasterization (BSD-3-Clause); [winit](https://docs.rs/winit/0.30.13/)
provides native window events (Apache-2.0). The direct-symbol frame builder
and ABI tests adapt the in-tree GPUI experiment's limits and owner contract.

The bounded builder reserves fallibly, but upstream widget construction,
fonts, renderer and event loop can allocate infallibly or abort. Window/input
qualification is in a private Xwayland server, **not** the live desktop, native
Wayland or other platforms. This is not `std.gui`, a general widget catalog,
or a framework recommendation. No dependencies or fonts are vendored here.
Credits and Cargo metadata are not a complete third-party notices bundle:
no binary redistribution qualification without auditing the built dependency,
asset and platform graph and assembling its required notices.
