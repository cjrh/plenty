# MouseArea

Wrap one child and send the action on a left-button press that the child did not consume. Close the group with `end()`. Its action ID must be nonzero and unique in the view.

From `examples/gui/iced_widgets`, run `plenty run --bin mouse_area`. To test the update without a window, use `plenty run --bin mouse_area -- --headless 1`; this dispatches the action directly.

```plenty build
{{#include ../../../../../examples/gui/iced_widgets/src/mouse_area.plenty}}
```
