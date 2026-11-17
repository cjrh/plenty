# Experimental GPUI host binding

The external source package in `experiments/packages/gpui/` demonstrates a real
[GPUI 0.2.2](https://docs.rs/gpui/0.2.2/gpui/) window with Plenty-owned counter
state. It supplies bounded rows, columns, UTF-8 text and clickable labels.
It is not `std.gpui`, a stable widget API or an ordinary `plenty run` target.
GPUI's native GUI implementation is substantial and evolving; wrapping it is
justified instead of recreating it in Plenty, and the dependency is isolated
from compiler/runtime builds.

## Build and example

From the repository root:

```sh
cargo build --locked --release --bin plenty
cd experiments/packages/gpui
CARGO_BUILD_JOBS=6 python3 -B build.py
target/optimized/plenty-gpui
```

`examples/counter.plenty` is the complete executable example. Increment changes
Plenty state, Reset sets it to zero, and the new Count label is rendered by
GPUI. The package README gives native prerequisites, exact API, bounds and
validation commands. Linux GNU x86-64 is the initial platform; a desktop,
Vulkan driver, fontconfig/fonts and X11/Wayland development libraries are needed.
X11/Xwayland is renderer-qualified; Wayland is compiled but not input-qualified.

`build.py` stages one application source file and the binding, emits a Plenty
application object with [executable C adapters](exports.md#executable-callbacks),
then builds **one Rust native support archive** containing `plenty-runtime`, the
bridge and exact `gpui = "=0.2.2"` in one isolated Cargo graph. The final `cc` link
uses that archive, the application object and rustc-reported native dependencies.
There is no shared app library or Rust host executable. Plenty owns main and
calls `gpui.run()` on the process main thread; ordinary runtime startup is retained.
The explicit helper still needs matching runtime sources/Cargo and native platform
prerequisites; automatic package native build selection is not implemented.
`--fast-compile` selects Plenty's other build mode.

## Boundary and ownership

The host uses [owned exported objects](owned-exports.md) for `State` and compiles
its C adapter against the generated header. `create` returns one owner;
`render` borrows it shared and `event` borrows it exclusively. Main-thread calls
are serialized, never nested. No GPUI/Rust layout crosses the boundary. After
callbacks stop, the host calls the originating destructor exactly once. Native code remains in the executable until process exit; no unload or hot reload
is offered. `gpui.run()` rejects wrong-thread, repeated or reentrant entry before
creating state.

Every render receives a fresh numeric frame token and three opaque host function
addresses. The `Frame` wrapper rejects null addresses and uses existing
[indirect extern calls](trusted-declarations.md#calling-a-native-address) to
send bounded commands synchronously. The trusted host must supply exact
prototypes; non-null arbitrary addresses cannot be validated. Do not retain a
Frame or its addresses. Token/thread/phase checks reject stale or worker use;
there are no thread-safety annotations. Text is borrowed using the
[UTF-8 adapter](borrowing-text.md#text-representations) and copied before return,
including NUL bytes. Native builder failures discard the frame.

The initial fixed window is 480 × 280 logical pixels. Each frame has at most
256 nodes, 16 nested containers, 4096 bytes per label and 65536 total text bytes.
Clickable labels use nonzero unique action IDs, not retained Plenty closures.
An event returns before `cx.notify()` triggers a shared render. Errors stop
callbacks and produce failure exit status; state mutations are not rolled back.
The counter stops at one million rather than overflowing.

GPUI/Rust allocations can still abort. Command entry contains Rust unwinds,
but upstream panics outside it and Plenty traps remain fatal; do not promise
recoverable OOM or actor/arena recovery. Clickable divs do not provide a complete
keyboard or accessibility contract. This is deliberately an external experiment.
The original separate-staticlib probe failed on duplicate Rust standard-library
and allocator symbols. The unified Cargo graph supplies exactly one runtime,
without symbol suppression or archive surgery. CLI-only builds retain their
existing graph and link no GUI platform support. No perfect per-widget stripping
is promised: only explicit native entrypoints/adapters establish reachability;
there is no universal widget registry or dispatch table in this proof.

## Qualification and distribution

`make test` separates deterministic boundary tests and real Plenty executable
lifecycle/event tests from `make smoke`, which opens a real X11 window, injects
clicks, compares rendered Count pixels, resets and closes normally. The native
runner's `--headless` option exercises only the ABI/model, not a renderer.

The dependency lock preserves compatible libc 0.2.186 rather
than resolving to libc 0.2.191, which breaks an old transitive xattr dependency.
The release notice generator has an explicit `gpui` component; it fails closed
when upstream transitive crates omit license texts. No binary distribution
is qualified or included. Redistribution needs those reviewed texts and Plenty
runtime notices. A coordinated pinned GPUI + gpui-kit evaluation, widget catalog and std GUI API
are separate follow-up work, not part of this infrastructure proof. Broader qualification belongs in the [backlog](../../../backlog.md).

## Acknowledgements

- [GPUI / Zed Industries](https://gpui.rs/): wrapped native GUI implementation,
  published `gpui` 0.2.2, Apache-2.0. The package downloads this dependency; no
  upstream implementation source is copied into Plenty.
