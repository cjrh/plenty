# `std.gui.retained.iced`

Import `std.gui.retained.iced as iced` for a bounded retained-mode tree. See
[GUI source builds](gui-build.md) for setup and the counter package. Plenty
owns the model and event policy; iced supplies the window, layout, font
handling, event loop and software renderer. See the [signatures](signatures.md#stdguiretainediced).

## Widgets and layout

| Example | Call | Effect |
| --- | --- | --- |
| [Text](gui-iced-widgets/text.md) | `frame.text(&message)` | Displays borrowed text; no action. |
| [Button](gui-iced-widgets/button.md) | `frame.button(&caption, action)` | Sends a nonzero action ID on click. |
| [Row](gui-iced-widgets/row.md) | `frame.row()` … `frame.end()` | Arranges children from left to right. |
| [Column](gui-iced-widgets/column.md) | `frame.column()` … `frame.end()` | Stacks children from top to bottom. |

## Example: a counter

Click **Increment** and the button sends action `1` to `update`. The next read-only `view` displays the new count; **Reset** sends action `2`.
After [setting up the source build](gui-build.md#source-installation-and-host),
run from the checkout root:

```sh
(cd examples/gui/iced_counter && plenty run)
```

```plenty build
{{#include ../../../../examples/gui/iced_counter/src/main.plenty}}
```

## Running the application

`iced.run(create=create, view=view, update=update)` enters once on the process main thread and reads the original
process arguments after the executable name. No arguments opens a window;
`--headless [ACTION ...]` renders the initial snapshot, then dispatches each
decimal `u64` action against the current snapshot. A malformed or unknown ID
returns `InvalidArgument`. Other first arguments, including an application
filename, return `InvalidArgument` **before** creating state. Parsing those
arguments in Plenty does not remove them. Even rejected arguments consume
the one main-thread entry: a second call returns `InvalidContext`.

Pass ordinary function values or capture-free closures; callback variables can be selected at runtime. Display-only apps can omit `update`: an action delivered to the default handler returns `InvalidArgument`. After admission, Plenty calls `create` once and makes two checked setup allocations for its callback environment and driver. Native code borrows the driver only during `run`. The read-only `view` receives `&State` and `&Frame`, opens `row()` or `column()`
containers, appends `text(&label)` and `button(&caption, action)` nodes and
balances each container with `end()`. The bridge copies the borrowed UTF-8
spans before returning. One balanced root, at most 16 open containers, 256
nodes, 4,096 bytes per caption and 65,536 bytes of text per render. Action IDs
are nonzero unique `u64` identifiers, not indexes. `&str` captions use the
reviewed no-copy UTF-8 extern adapter, not a `&strview` conversion. Failures
inside an admitted operation on the matching frame latch: no partial tree is published even if an error is ignored.

The iced view reads the last validated snapshot. Its update validates the
clicked action against that snapshot, calls `update(&mut State, action)`, then
renders again after the exclusive loan ends. No widget keeps a Plenty loan.
A stale/wrong-thread frame or repeated/nested entry returns `InvalidContext`.
Invalid structure returns `InvalidLayout`; invalid input, exceeded caps and
native failures have separate variants. `allocation` maps `AllocError` to
`OutOfMemory` or `CapacityOverflow`: flat variants preserve the payload-free
callback ABI. If iced returns a native loop error while closing after a checked
callback failure, the checked error takes precedence. `run` returns checked callback errors and releases an established owner through its original
Plenty destructor on exit (including checked failure); mutation is not rolled
back. Native rendering and font allocations can abort on OOM; this is not a
recovery guarantee for traps, panics that abort, or process termination.

## Acknowledgements

- [iced](https://iced.rs/), Héctor Ramón and contributors (MIT): widgets, application update/view and native window integration, pinned to iced 0.14.0.
- [tiny-skia](https://docs.rs/tiny-skia/0.11.4/) (BSD-3-Clause): software rasterization.
- [winit](https://docs.rs/winit/0.30.13/) (Apache-2.0): window events and X11 integration.
- `iced_graphics` embeds the Iced Icons font. Audit this asset as well as the renderer crate notices before redistribution.
- The in-tree [GPUI experiment](../../completed-work/libraries.md#experimental-gpui-host-binding) informed the bounded tree and owner tests; it is not a dependency.
