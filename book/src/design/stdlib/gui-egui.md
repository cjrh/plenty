# `std.gui.immediate.egui`

Import `std.gui.immediate.egui as egui` to evaluate widgets in the current
UI pass. See [GUI source builds](gui-build.md) for the host requirements and
counter package and the [signatures](signatures.md#stdguiimmediateegui).
The external toolkit supplies layout, input, text and graphics integration;
there is no retained Plenty widget tree or event dispatcher.

`egui.run()` enters once on the process main thread. A successful `create()`
transfers one opaque state owner, held until close or error. Each synchronous
`draw(&mut State, token)` wraps its `u64` token with `Ui(token)` (no
allocation). `button(&caption)` evaluates a real egui button **now**: its
checked `bool` is the clicked response, not the press or a redraw. `label(&text)`
paints text in this pass, so a label placed after the buttons sees the updated
count. The UI limits egui to one pass per frame; idle redraws do not replay a
click. Each widget borrows its UTF-8 span only for the call. A scoped,
thread-local exclusive loan prevents nested widget access; it ends even on
unwind. Saved tokens do not keep the pass alive. Wrong thread, expired or
invalid context and repeated/nested entry return `InvalidContext`.

Limits are 256 widgets, 4,096 bytes per caption and 65,536 bytes per pass.
Oversize, invalid input and native failures have distinct errors. Errors on a
live pass latch even if ignored. `allocation` preserves `AllocError` as
`OutOfMemory` or `CapacityOverflow`. `run` propagates a checked startup/draw
error and calls the originating Plenty destructor exactly once if creation
succeeded. It does not roll back completed mutations. Egui, fonts, GL and
platform allocation can abort; there is no checked cleanup on process abort.

## Acknowledgements

- [egui](https://www.egui.rs/) by Emil Ernerfeldt and contributors (MIT OR Apache-2.0), pinned to 0.37.0: immediate-mode widgets, responses, text and layout.
- [eframe](https://docs.rs/eframe/0.37.0/) (MIT OR Apache-2.0): native X11 event loop and glow renderer.
- [epaint_default_fonts](https://docs.rs/epaint_default_fonts/0.37.0/): embedded font assets with individual notices, including Hack, Ubuntu Light and the egui icon subset of emoji-icon-font. The crate's dual license does not replace the font licenses.
