# DragValue

![egui DragValue example window](screenshots/drag_value.png)

Edit a finite value between inclusive bounds. The boolean reports whether it changed; invalid input leaves the value untouched.

From `examples/gui/egui_widgets`, run `plenty run --bin drag_value`.

```plenty build
{{#include ../../../../../examples/gui/egui_widgets/src/drag_value.plenty}}
```
