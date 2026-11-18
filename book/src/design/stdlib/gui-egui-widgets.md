# egui widget examples

Each example is a complete executable using [`std.gui.immediate.egui`](gui-egui.md).
Follow the [GUI source build instructions](gui-build.md#source-installation-and-host),
then run one from the checkout:

```sh
cd examples/gui/egui_widgets
plenty run --bin label
```

Use the binary name shown for each example. Append `-- --headless` to inspect its initial labels without opening a window.

## Label

Binary: `label`. The label reads **Ready** from state on each draw. There is no label handle to keep or update.

```plenty build
{{#include ../../../../examples/gui/egui_widgets/src/label.plenty}}
```

## Button

Binary: `button`. Click **Approve**. The successful `true` response changes **Waiting** to **Approved** before the label is drawn in the same pass. A later redraw keeps the new text without replaying the click.

```plenty build
{{#include ../../../../examples/gui/egui_widgets/src/button.plenty}}
```

## Single-line text editor

Binary: `text_edit`. Edit **Ada** and watch the label echo the current name without pressing Enter. Plain Enter also shows **Submitted** for that draw only. The fixed ID follows this field across redraws; the name itself must not be the ID.

```plenty build
{{#include ../../../../examples/gui/egui_widgets/src/text_edit.plenty}}
```
