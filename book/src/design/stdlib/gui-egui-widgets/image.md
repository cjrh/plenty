# Image

Draw a small image from borrowed RGBA bytes with `ui.image_rgba`. Supply four bytes (red, green, blue, straight alpha) per pixel, left to right and top to bottom. The example shows an orange and a blue pixel magnified 16 times.

**Source and size.** The binding uploads raw pixels on each draw, not a path, URL, encoded file, or persistent texture handle. Each dimension must be 1–256 pixels. Nearest-neighbor scaling uses 1–16 logical points per source pixel (default 1). Image bytes and text together must fit the 65,536-byte pass budget. No decoder, animation, or additional image options are exposed.

From `examples/gui/egui_widgets`, run `plenty run --bin image`.

```plenty build
{{#include ../../../../../examples/gui/egui_widgets/src/image.plenty}}
```
