# std.image

`import std.image` holds images as owned buffers of 8-bit pixels, decodes and
encodes PNG files, and crops, converts and resizes images. It has one error
type, `ImageError`.

The module is ordinary Plenty over [`std.bytes`](bytes.md) and
[`std.compress`](compress.md), which supplies zlib and CRC-32; it adds no
runtime code. Decoding checks the image's dimensions against caller-supplied
limits before it allocates. It is available on the supported x86-64 Linux GNU
target.

```plenty
import std.image
from std.image import ColorType, Filter, ImageError

def main() -> Result[(), IoError | ImageError]:
    # A 4 x 2 RGB image: a red top row over a black bottom row.
    mut image = std.image.new(4, 2, ColorType.Rgb)?
    for x in range(4):
        image.set(x, 0, 0, 255u8)?
    png = std.image.encode_png(&image)?
    decoded = std.image.decode(&png, &std.image.limits())?
    print(decoded.width(), decoded.height(), decoded.pixels() == image.pixels())?
    small = decoded.resize(2, 1, Filter.Bilinear)?
    print(small.get(0, 0, 0)?, small.get(0, 0, 1)?)?
    gray = decoded.convert(ColorType.Gray)?
    print(gray.get(0, 0, 0)?, gray.get(0, 1, 0)?)?
    Ok(())
```
```output
4 2 True
128 0
54 0
```

## Functions

| Function | Result | Contract |
|---|---|---|
| `new(width: i64, height: i64, color: ColorType)` | `Result[Image, ImageError]` | A zeroed image: black, and transparent if it has alpha |
| `from_pixels(width: i64, height: i64, color: ColorType, pixels: Bytes)` | `Result[Image, ImageError]` | Takes `pixels` as the buffer, without copying |
| `detect(data: &Bytes)` | `Option[Format]` | The format named by the first bytes of `data` |
| `info(data: &Bytes)` | `Result[Info, ImageError]` | The header of a PNG file, without decoding pixels |
| `decode(data: &Bytes, limits: &Limits)` | `Result[Image, ImageError]` | The image in a whole PNG file, within `limits` |
| `encode_png(image: &Image, level: i64 = 6)` | `Result[Bytes, ImageError]` | A PNG file of `image`, compressed at zlib `level` |
| `limits()` | `Limits` | The default limits; see [Decoding limits](#decoding-limits) |

Width and height lie in 1 to 2³¹−1, the PNG maximum, so every image can be
encoded; other values are `OutOfRange`, and an image whose size in bytes does
not fit in `i64` is `LimitExceeded`. `from_pixels` needs a buffer of exactly
`width * height * channels` bytes, otherwise `OutOfRange`.

`detect` recognises PNG, JPEG, GIF and WebP signatures and returns `Nothing`
for anything else. Only PNG is decoded: `info` and `decode` return
`Unsupported` for any input that `detect` does not name as PNG, including an
empty or partial signature.

`Format` is `Png`, `Jpeg`, `Gif` or `WebP`. `Filter` is `Nearest` or
`Bilinear`; see [Resizing](#resizing).

`Info` is a `@copy` class with public fields:

| Field | Meaning |
|---|---|
| `format: Format` | Always `Png` |
| `width: i64`, `height: i64` | The dimensions in pixels |
| `color: ColorType` | The colour type `decode` produces |
| `depth: i64` | The stored bits per sample: 1, 2, 4, 8 or 16 |
| `interlaced: bool` | Whether the file is Adam7-interlaced |

## Images

`ColorType` is `Gray`, `GrayAlpha`, `Rgb` or `Rgba`. Its `channels()` method
returns 1 to 4 and `has_alpha()` whether the last channel is alpha. Channels
are interleaved in the order of the name. An `Image` stores rows top to
bottom and pixels left to right, `width * channels` bytes per row with no
padding, so the byte of channel `c` at (`x`, `y`) is at
`(y * width + x) * channels + c`. Its fields are private, so the buffer length
always matches the dimensions.

| Method | Result | Contract |
|---|---|---|
| `width()`, `height()` | `i64` | The dimensions in pixels |
| `color()` | `ColorType` | The colour type |
| `pixels()` | `&Bytes` | The buffer, borrowed |
| `into_pixels(self: Image)` | `Bytes` | Gives up the image for its buffer, without copying |
| `get(x, y, channel)` | `Result[u8, ImageError]` | One sample |
| `set(self: &mut Image, x, y, channel, value: u8)` | `Result[(), ImageError]` | Replaces one sample |
| `copy()` | `Result[Image, ImageError]` | An independent image with the same pixels |
| `crop(x, y, width, height)` | `Result[Image, ImageError]` | A copy of the rectangle whose top-left pixel is (`x`, `y`) |
| `convert(color: ColorType)` | `Result[Image, ImageError]` | A copy in another colour type; see [Colour conversion](#colour-conversion) |
| `resize(width, height, filter: Filter)` | `Result[Image, ImageError]` | A resampled copy; see [Resizing](#resizing) |

A coordinate or channel outside the image, or a rectangle that is empty or
extends past an edge, is `OutOfRange`. The builtin `copy(image)` is rejected
because the buffer is a resource; use the method.

## Decoding limits

`decode` reads the header and checks it against a `Limits` before it
allocates anything, so a small file that declares a large image fails at once
with `LimitExceeded`:

| Field | Bound |
|---|---|
| `max_width: i64` | Width in pixels |
| `max_height: i64` | Height in pixels |
| `max_bytes: i64` | The decoded buffer, `width * height * channels` bytes |

`std.image.limits()` returns 16384 × 16384 pixels and 256 MiB, enough for an
8192 × 8192 `Rgba` image. Construct `Limits(max_width, max_height, max_bytes)`
for other bounds. `max_bytes` counts the channels of the decoded colour type:
a palette image with transparency decodes to `Rgba`, four bytes per pixel. A
negative limit is `OutOfRange`. `check(width, height, color)` applies the same
test to any dimensions, for example before `new` or `resize` with sizes taken
from input.

The limits bound what `decode` allocates: besides the result, it holds two
scanlines of at most 8 × `width` + 1 bytes each, a palette of at most 1 KiB
and an [`Inflater`](compress.md#inflater) of about 62 KiB. The result is
reserved at full size once the header passes. A non-interlaced image is
decoded into it row by row, so a file that declares a large image but holds
little data writes little memory; an interlaced image is zero-filled first,
because its passes write scattered pixels.

`decode` takes the whole file in memory, so bound the input when reading it
as well. Read one byte more than the bound to tell a file that fits from one
that does not:

```plenty
import std.bytes
import std.image
import std.io
from std.bytes import Bytes
from std.image import ColorType, ImageError, Limits

type Problem = IoError | std.bytes.BytesError | std.io.StreamError | std.io.Partial | ImageError

# The whole file, or LimitExceeded if it is longer than `limit` bytes.
def load(path: &str, limit: i64) -> Result[Bytes, Problem]:
    mut file = std.io.open_file(path, "r")?
    mut data = std.bytes.empty()?
    read = std.io.read_up_to(&mut file, &mut data, limit + 1)?
    file.close()?
    if read > limit:
        return Err(ImageError.LimitExceeded)
    Ok(data)

def main() -> Result[(), Problem]:
    banner = std.image.new(3000, 100, ColorType.Rgb)?
    png = std.image.encode_png(&banner)?
    mut out = std.io.open_file("banner.png", "w")?
    std.io.write_all(&mut out, &png, 0, png.len())?
    out.close()?

    data = load("banner.png", 1048576)?
    header = std.image.info(&data)?
    print(header.width, header.height, header.depth)?
    previews = Limits(1024, 1024, 4194304)
    match std.image.decode(&data, &previews):
        case Ok(image):
            print(image.width())?
        case Err(ImageError.LimitExceeded):
            print("too large for a preview")?
        case Err(error):
            return Err(error)
    Ok(())
```
```output
3000 100 8
too large for a preview
```

## Decoding

`decode` accepts every PNG colour type, bit depth and filter, interlaced or
not, and produces 8 bits per channel:

| PNG colour type | Decodes to | With `tRNS` |
|---|---|---|
| Grayscale, 1 to 16 bits | `Gray` | `GrayAlpha` |
| Truecolour, 8 or 16 bits | `Rgb` | `Rgba` |
| Indexed, 1 to 8 bits | `Rgb` | `Rgba` |
| Grayscale with alpha, 8 or 16 bits | `GrayAlpha` | Not allowed |
| Truecolour with alpha, 8 or 16 bits | `Rgba` | Not allowed |

Samples of 1, 2 and 4 bits scale exactly to 0–255. A 16-bit sample `v`
becomes `(v + 128) // 257`, the nearest 8-bit level. A `tRNS` colour key is
compared with the stored samples at full depth, so a 16-bit key matches only
its exact colour. Palette entries without a `tRNS` alpha are opaque.

The file must follow the PNG structure exactly; anything else is an error:

- **Signature and header.** `IHDR` must come first, with a valid size,
  colour type and depth pair, compression and filter method 0, and interlace
  method 0 or 1.
- **Chunks.** Every chunk's CRC is checked: a mismatch is `Checksum`. A chunk
  length over 2³¹−1 or a type that is not four ASCII letters is `Corrupt`. An
  unknown critical chunk, one whose type starts with an uppercase letter, is
  `Unsupported`; unknown ancillary chunks are skipped after their CRC check.
- **Order.** `PLTE` and `tRNS` come before the image data, once each,
  `PLTE` first, and only for colour types that allow them; an indexed image
  needs `PLTE`. The `IDAT` chunks are consecutive, and their contents form one
  zlib stream. `IEND` follows, with no data. Bytes after `IEND` are ignored.
- **Image data.** The zlib Adler-32 is checked: a mismatch is `Checksum`.
  A stream that ends early, holds more data than the header describes, or
  contains an invalid filter type or palette index is `Corrupt`.
- **Truncation.** Input that ends inside a chunk or before `IEND` is
  `Truncated`.

Ancillary chunks are ignored, including `gAMA`, `cHRM`, `sRGB`, `iCCP`,
`bKGD`, `pHYs`, text and time chunks, and the animation chunks of APNG, so an
animated PNG decodes to its default image.

`info` reads the signature, `IHDR` and the chunks before the image data with
the same checks, and allocates nothing.

## Encoding

`encode_png` writes an 8-bit, non-interlaced PNG of the image's colour type:
the signature, `IHDR`, the image data in `IDAT` chunks of about 64 KiB, and
`IEND`, with no ancillary chunks. `level` is the zlib level, 0 to 9;
anything else is `OutOfRange`. Level 0 stores rows unfiltered and
uncompressed. Above 0, each row is filtered all five ways and the one with
the smallest sum of absolute differences is kept, the heuristic the PNG
specification recommends. Equal images and levels give equal files.

## Colour conversion

`convert` copies the image into another colour type:

- **Colour to gray** uses ITU-R BT.709 luma of the stored values,
  `(2126 × red + 7152 × green + 722 × blue + 5000) // 10000`.
- **Gray to colour** copies the gray level into all three colour channels.
- **Dropping alpha** discards it, without blending against a background.
- **Adding alpha** makes every pixel opaque.

Converting to the image's own colour type returns a copy. Values are used as
stored, without gamma decoding.

## Resizing

`resize` takes target dimensions under the rules of `new`. It does not apply
`Limits`; check sizes taken from input with `Limits.check` first. Resizing to
the same dimensions returns a copy.

- **`Nearest`** copies, for each target pixel, the source pixel under its
  centre.
- **`Bilinear`** weights source pixels with a triangle filter, one axis at a
  time. When shrinking, the filter widens by the reduction factor so every
  source pixel contributes, which avoids the aliasing of point sampling.
  Weights are fixed-point with 22 fractional bits, following Pillow's
  `Image.resize`.

Images with alpha are resampled with premultiplied colour, so transparent
pixels do not tint their neighbours; a fully transparent result has colour 0.
Bilinear results equal Pillow's for images without alpha and are within 1
with alpha, where the two round premultiplied values differently.

## Errors

```text
pub enum ImageError:
    OutOfRange
    Corrupt
    Checksum
    Truncated
    Unsupported
    LimitExceeded
    Allocation(AllocError)
```

| Variant | Meaning |
|---|---|
| `OutOfRange` | A dimension, coordinate, channel, rectangle, buffer length, level, or limit out of bounds |
| `Corrupt` | A malformed PNG file; see [Decoding](#decoding) |
| `Checksum` | A chunk CRC or zlib Adler-32 mismatch |
| `Truncated` | The input ended inside a chunk or before `IEND` |
| `Unsupported` | A format other than PNG, or an unknown critical chunk |
| `LimitExceeded` | An image outside the `Limits`, or too large to address |
| `Allocation(cause)` | A buffer could not be allocated |

## Ownership and threads

An `Image` owns its buffer; dropping it releases the buffer. `from_pixels`
and `into_pixels` move a buffer in and out without copying, and `pixels`
borrows it. Every other operation that returns an image returns a new one
and leaves its source unchanged. The module has no global state, passes the
ordinary worker-effect checks, and may be used from any thread.

## Costs

| Operation | Allocates |
|---|---|
| `new`, `copy`, `crop`, `convert`, `resize` with `Nearest` | The result |
| `resize` with `Bilinear` | The result; for each resampled axis, 16 bytes plus 8 bytes per tap for each target position, with 4 taps when enlarging and about 2 × the reduction factor + 2 when shrinking; and an intermediate image of the target width and source height when the height changes and either the width changes or the image has alpha |
| `decode` | The result, two scanlines, the palette, and an `Inflater`; see [Decoding limits](#decoding-limits) |
| `encode_png` | The file, which grows by doubling; a zlib `Compressor` (about 387 KiB); five filtered copies of a row and a zero row, or one row at level 0; and about 64 KiB of compressed data before each `IDAT` is framed |
| `detect`, `info`, `from_pixels`, `into_pixels`, `get`, `set`, `Limits.check` | Nothing |

Decoding is linear in the file size and pixel count. Encoding is linear in
the pixel count, with zlib slower at higher levels. A bilinear resize costs
one multiply per tap per target sample on each axis. Pixel loops are scalar
Plenty without SIMD. On a 3000 × 2000 RGB image, decoding took about twice
as long as Pillow, encoding at level 6 about 2.7 times, and bilinear
shrinking to a third about 6 times.

The module is compiled only into programs that import it.

## Limits

- PNG only: JPEG, GIF and WebP are detected but `Unsupported`.
- 8 bits per channel. 16-bit samples are reduced on decode, and there is no
  16-bit, palette, low-depth or interlaced output.
- No colour management: gamma, chromaticity, sRGB and ICC chunks are ignored,
  and conversion and resizing work on stored values, not linear light.
- No metadata: text, time, physical size and background chunks are neither
  returned nor written. No APNG animation frames.
- Whole files only: no decoding from a `std.io` reader or row by row.
- Only nearest and bilinear filters; no box, bicubic or Lanczos.
- No rotation, flipping, compositing or drawing.
- One target, x86-64 Linux GNU.

## Acknowledgements

- The [W3C PNG Third Edition](https://www.w3.org/TR/png-3/) supplies the
  format, the filter-selection heuristic and the Adam7 interlace by
  Adam M. Costello; the decoder and encoder are implemented independently.
- The Paeth filter is from Alan W. Paeth, "Image File Compression Made Easy",
  *Graphics Gems II* (1991).
- Bilinear weights follow [Pillow](https://python-pillow.github.io/)'s
  resampling; the module does not wrap or ship its code. Tests compare
  decoding, encoding and bilinear resizing with Pillow when it is installed.
- Gray conversion uses the ITU-R BT.709 weights, as the Rust
  [`image`](https://docs.rs/image/) crate does.
