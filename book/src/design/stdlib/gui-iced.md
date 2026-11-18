# `std.gui.retained.iced`

Import `std.gui.retained.iced as iced` for a bounded retained-mode tree. See
[GUI source builds](gui-build.md) for setup and the counter package. Plenty
owns the model and event policy; iced supplies the window, layout, font
handling, event loop and software renderer. See the [signatures](signatures.md#stdguiretainediced).

`iced.run()` enters once on the process main thread. `create()` transfers one
opaque `State` on success; failure transfers nothing. `render(&State, token)`
constructs a `Frame(token)` (no allocation), opens `row()` or `column()`
containers, appends `text(&label)` and `button(&caption, action)` nodes and
balances each container with `end()`. The bridge copies the borrowed UTF-8
spans before returning. One balanced root, at most 16 open containers, 256
nodes, 4,096 bytes per caption and 65,536 bytes of text per render. Action IDs
are nonzero unique `u64` identifiers, not indexes. Matching-frame failures
latch: no partial tree is published even if an error is ignored.

The iced view reads the last validated snapshot. Its update validates the
clicked action against that snapshot, calls `event(&mut State, action)`, then
renders again after the exclusive loan ends. No widget keeps a Plenty loan.
A stale/wrong-thread frame or repeated/nested entry returns `InvalidContext`.
Invalid structure returns `InvalidLayout`; invalid input, exceeded caps and
native failures have separate variants. `allocation` maps `AllocError` to
`OutOfMemory` or `CapacityOverflow` without erasing the reason. `run` returns
checked callback errors and releases an established owner through its original
Plenty destructor on exit (including checked failure); mutation is not rolled
back. Native rendering and font allocations can abort on OOM; this is not a
recovery guarantee for traps, panics that abort, or process termination.

## Acknowledgements

- [iced](https://iced.rs/), Héctor Ramón and contributors (MIT): widgets, application update/view and native window integration, pinned to iced 0.14.0.
- [tiny-skia](https://docs.rs/tiny-skia/0.11.4/) (BSD-3-Clause): software rasterization.
- [winit](https://docs.rs/winit/0.30.13/) (Apache-2.0): window events and X11 integration.
- `iced_graphics` embeds the Iced Icons font. Audit this asset as well as the renderer crate notices before redistribution.
- The in-tree [GPUI experiment](../../completed-work/libraries.md#experimental-gpui-host-binding) informed the bounded tree and owner tests; it is not a dependency.
