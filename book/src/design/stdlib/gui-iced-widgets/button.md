# Button

Click **Approve** to change **Waiting** to **Approved**. The button declares an action; `update`, not `view`, changes the model. Try `plenty run --bin button -- --headless 1` to dispatch that action without a window.

From `examples/gui/iced_widgets`, run `plenty run --bin button`.

```plenty build
{{#include ../../../../../examples/gui/iced_widgets/src/button.plenty}}
```
