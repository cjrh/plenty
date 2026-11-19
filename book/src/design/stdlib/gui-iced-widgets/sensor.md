# Sensor

![iced Sensor example window](screenshots/sensor.png)

Wrap one child and send the action when iced reports that it has come into view. The message carries only the action ID, not the measured size. Close the group with `end()`. Its action ID must be nonzero and unique in the view.

From `examples/gui/iced_widgets`, run `plenty run --bin sensor`. To test the update without a window, use `plenty run --bin sensor -- --headless 1`; this dispatches the action directly.

```plenty build
{{#include ../../../../../examples/gui/iced_widgets/src/sensor.plenty}}
```
