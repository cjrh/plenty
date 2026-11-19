# Float

![iced Float example window](screenshots/float_scaled.png)

Enlarge one child as an overlay without reserving extra layout space. Supply a scale from 100 (unchanged) through 400 (four times the original size); other values return `InvalidArgument`. Close the group with `end()`.

From `examples/gui/iced_widgets`, run `plenty run --bin float_scaled`.

```plenty build
{{#include ../../../../../examples/gui/iced_widgets/src/float_scaled.plenty}}
```
