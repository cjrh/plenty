# RangeSlider

Use two handles to edit an inclusive interval. Supply distinct mutable endpoints
with finite values satisfying `minimum <= low <= high <= maximum`.
The boolean reports a change in this pass. A checked rejection leaves both
endpoints untouched.

From `examples/gui/egui_widgets`, run `plenty run --bin range_slider`.

```plenty build
{{#include ../../../../../examples/gui/egui_widgets/src/range_slider.plenty}}
```
