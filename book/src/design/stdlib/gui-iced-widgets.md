# iced widget examples

Each example is a complete executable using [`std.gui.retained.iced`](gui-iced.md).
Follow the [GUI source build instructions](gui-build.md#source-installation-and-host),
then run one from the checkout:

```sh
cd examples/gui/iced_widgets
plenty run --bin text
```

Use the binary name shown for each example. Append `-- --headless` to inspect its initial snapshot without opening a window.

## Text

Binary: `text`. A text node can be the whole view. This one shows **Ready**. With no buttons there are no valid actions, so the application omits `update`.

```plenty build
{{#include ../../../../examples/gui/iced_widgets/src/text.plenty}}
```

## Button

Binary: `button`. Click **Approve** to change **Waiting** to **Approved**. The button declares an action; `update`, not `view`, changes the model. Try `plenty run --bin button -- --headless 1` to dispatch that action without a window.

```plenty build
{{#include ../../../../examples/gui/iced_widgets/src/button.plenty}}
```

## Row

Binary: `row`. **Mass:** and **kg** are siblings on the same line. Start the row before either child and close it after both.

```plenty build
{{#include ../../../../examples/gui/iced_widgets/src/row.plenty}}
```

## Column

Binary: `column`. **Weather station** appears above **Harbor**. `end()` closes the most recently opened container; it is not a visible widget. The counter nests a row inside a column using the same rule.

```plenty build
{{#include ../../../../examples/gui/iced_widgets/src/column.plenty}}
```
