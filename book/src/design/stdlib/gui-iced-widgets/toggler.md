# Toggler

The view displays the model’s boolean. The widget sends its nonzero action ID on interaction; update decides whether to toggle the value.

From `examples/gui/iced_widgets`, run `plenty run --bin toggler` or try `plenty run --bin toggler -- --headless 1` to send the action without opening a window.

```plenty build
{{#include ../../../../../examples/gui/iced_widgets/src/toggler.plenty}}
```
