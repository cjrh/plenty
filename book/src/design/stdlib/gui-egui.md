# `std.gui.immediate.egui`

Import `std.gui.immediate.egui as egui` to evaluate widgets in the current
UI pass. See [GUI source builds](gui-build.md) for the host requirements and
example packages and the [signatures](signatures.md#stdguiimmediateegui).
The external toolkit supplies layout, input, text and graphics integration;
there is no retained Plenty widget tree or event dispatcher.

## Widgets

| Example | Call | Effect |
| --- | --- | --- |
| [Label](gui-egui-widgets.md#label) | `ui.label(&message)` | Paints borrowed text; no interaction. |
| [Button](gui-egui-widgets.md#button) | `ui.button(&caption)` | Returns whether this button was clicked. |
| [Single-line text editor](gui-egui-widgets.md#single-line-text-editor) | `ui.text_edit(id, &mut text)` | Updates the string; returns whether Enter submitted it. |

## Example: a counter

Click **Increment** and `draw` changes the count immediately. The label below the buttons sees the new value in that same pass.
After [setting up the source build](gui-build.md#source-installation-and-host),
run from the checkout root:

```sh
(cd examples/gui/egui_counter && plenty run)
```

```plenty fragment
{{#include ../../../../examples/gui/egui_counter/src/main.plenty}}
```

## Running the application

`egui.run()` enters once on the process main thread and reads the original
process arguments after the executable name. No arguments opens a window;
`--headless [X,Y ...]` accepts up to 64 pointer clicks, each a pair of decimal
`u16` coordinates in logical points. Bad coordinates return `InvalidArgument`;
a 65th click returns `LimitExceeded`. A first argument other than `--headless`
returns `InvalidArgument` **before** creating state, even if Plenty already
parsed it as an application option or filename. This consumes the one
main-thread entry: a second call returns `InvalidContext`.

A successful `create()`
transfers one opaque state owner, held until close or error. Each synchronous
`draw(&mut State, token)` wraps its `u64` token with `Ui(token)` (no
allocation). `button(&caption)` evaluates a real egui button **now**: its
checked `bool` is the clicked response, not the press or a redraw. `label(&text)`
paints text in this pass, so a label placed after the buttons sees the updated
count. The UI limits egui to one pass per frame; idle redraws do not replay a
click. Widgets accept `&str` rather than `&strview` to borrow a UTF-8 span
through the reviewed no-copy extern adapter, only for this call. A scoped,
thread-local exclusive loan prevents nested widget access; it ends even on
unwind. Saved tokens do not keep the pass alive. Wrong thread, expired or
invalid context and repeated/nested entry return `InvalidContext`.

Limits are 256 widgets, 4,096 bytes per caption and 65,536 bytes per pass.
Oversize, invalid input and native failures have distinct errors. Failures inside an
admitted widget operation on the matching pass latch even if ignored.
A bad token or a nested attempt rejected before admission does not poison
the active pass. `allocation` preserves `AllocError` as
`OutOfMemory` or `CapacityOverflow`: flat variants preserve the payload-free
callback ABI. `run` propagates a checked startup/draw
error and calls the originating Plenty destructor exactly once if creation
succeeded. It does not roll back completed mutations. Egui, fonts, GL and
platform allocation can abort; there is no checked cleanup on process abort.

## Editing a single line

`ui.text_edit(id, &mut text)?` draws a `TextEdit` from the current Plenty
`str`. A successful edit replaces that string **before the call returns**.
The `bool` is a submission, not a change flag: the field processes this frame’s edits, then
consumes an unmodified Enter press if it has keyboard focus and surrenders
focus on submission. Modified Enter, unrelated focus loss, key release and
idle redraw do not submit. Give each
field a distinct `u64` ID and reuse it across draws, even if earlier widgets
are inserted or removed. Don't use the changing query, result count, frame
token or a filtered row number as its identity. Egui associates the caret,
selection and focus with the field; Plenty need not keep a second editor model.

The input and draft must each fit 4,096 UTF-8 bytes, not characters. A field
counts as one widget and charges the larger of its input and output byte
lengths against the pass budget. A draft may not grow beyond the remaining
space. A CR or LF in the Plenty string is `InvalidArgument`. In a paste, egui
replaces each CR and LF with one space. A limit violation is `LimitExceeded`,
not truncation.

These byte caps bound the model accepted by the field and its edit draft,
not clipboard acquisition, platform events or toolkit scratch buffers. The
bridge checks a routed paste’s borrowed byte length before drawing when the
payload alone cannot fit even after deleting a selection. During a pointer
transition that may redirect keyboard focus, it lets the editor route the
keyboard paste and checks the draft insertion instead.

The bridge edits a bounded temporary string. On change, Plenty allocates an
exact-length byte list, copies through the call-scoped strided adapter, then
validates and allocates the replacement `str`. Only then does it assign the
new text. A checked rejection, including failed allocation, preserves the
**original string**, even when the attempted edit replaced a selection.
Neither the native editor state nor other successful widget calls or model
updates in the frame are transactional. Toolkit/platform allocations have
the same abort limitation as for labels and buttons. See the
[command picker lesson](../../tutorial/special-purpose/gui-counters.md#filtering-commands-as-you-type).

## Acknowledgements

- [egui](https://www.egui.rs/) by Emil Ernerfeldt and contributors (MIT OR Apache-2.0), pinned to 0.37.0: immediate-mode widgets, responses, text and layout.
- [eframe](https://docs.rs/eframe/0.37.0/) (MIT OR Apache-2.0): native X11 event loop and glow renderer.
- [epaint_default_fonts](https://docs.rs/epaint_default_fonts/0.37.0/): embedded font assets with individual notices, including Hack, Ubuntu Light and the egui icon subset of emoji-icon-font. The crate's dual license does not replace the font licenses.
