//! `std.image`: pixel buffers, PNG decoding and encoding, cropping, colour
//! conversion and resizing. Fixtures are built here from an independent model
//! of the PNG format (stored zlib blocks, forward filters, Adam7), and checked
//! against Pillow when it is installed.
mod support;
use support::{hex, run_file as run};

const HEADER: &str = r#"import std.bytes
import std.encoding
import std.image
from std.bytes import Bytes, BytesError
from std.encoding import EncodingError
from std.fmt import FormatError
from std.image import ColorType, Filter, Format, Image, ImageError, Limits

type Problem = AllocError | IoError | FormatError | EncodingError | BytesError | ImageError

def unhex(text: &str) -> Result[Bytes, Problem]:
    Ok(std.encoding.hex_from_text(text)?)

def hex(data: &Bytes) -> Result[str, Problem]:
    Ok(std.encoding.hex_text(data, 0, data.len())?)

def named(color: ColorType) -> str:
    match color:
        case ColorType.Gray:
            "Gray"
        case ColorType.GrayAlpha:
            "GrayAlpha"
        case ColorType.Rgb:
            "Rgb"
        case ColorType.Rgba:
            "Rgba"

# An image as `WIDTHxHEIGHT COLOUR PIXELS`, or the error.
def shown(result: Result[Image, ImageError]) -> Result[str, Problem]:
    match result:
        case Ok(image):
            pixels = hex(image.pixels())?
            Ok(f"{image.width()}x{image.height()} {named(image.color())} {pixels}"?)
        case Err(error):
            Ok(str.repr(error)?)

def decoded(text: &str) -> Result[str, Problem]:
    shown(std.image.decode(&unhex(text)?, &std.image.limits()))

def described(text: &str) -> Result[str, Problem]:
    match std.image.info(&unhex(text)?):
        case Ok(info):
            format = str.repr(info.format)?
            Ok(f"{format} {info.width}x{info.height} {named(info.color)} {info.depth} {info.interlaced}"?)
        case Err(error):
            Ok(str.repr(error)?)

def made(width: i64, height: i64, color: ColorType, text: &str) -> Result[Image, Problem]:
    Ok(std.image.from_pixels(width, height, color, unhex(text)?)?)

# A deterministic image whose bytes do not compress.
def noisy(width: i64, height: i64, color: ColorType, seed: i64) -> Result[Image, Problem]:
    total = width * height * color.channels()
    mut pixels = std.bytes.with_capacity(total)?
    mut x = seed
    for _ in range(0, total):
        x = x ^ (x << 13)
        x = x ^ (x >> 7)
        x = x ^ (x << 17)
        pixels.push(u8.wrapping_from(x >> 24))?
    Ok(std.image.from_pixels(width, height, color, pixels)?)

# A smooth image, which the PNG filters predict well.
def smooth(width: i64, height: i64, color: ColorType) -> Result[Image, Problem]:
    mut image = std.image.new(width, height, color)?
    for y in range(0, height):
        for x in range(0, width):
            for c in range(0, color.channels()):
                image.set(x, y, c, u8.wrapping_from(3 * x + 5 * y + 40 * c))?
    Ok(image)

# Whether encoding at `level` and decoding gives back the same image.
def trip(image: &Image, level: i64) -> Result[bool, Problem]:
    png = std.image.encode_png(image, level)?
    back = std.image.decode(&png, &std.image.limits())?
    Ok(named(back.color()) == named(image.color()) and back.width() == image.width() and hex(back.pixels())? == hex(image.pixels())?)

"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}def main() -> Result[(), Problem]:\n{body}    Ok(())\n")
}

fn output(source: &str) -> String {
    let output = run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.image.ImageError.", "")
        .replace("std.image.", "")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

// ---- An independent PNG writer for fixtures ----

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A zlib stream of stored blocks: valid, and independent of any compressor.
fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if raw.is_empty() {
        out.extend([1, 0, 0, 0xff, 0xff]);
    }
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let size = block.len() as u16;
        out.extend(size.to_le_bytes());
        out.extend((!size).to_le_bytes());
        out.extend(block);
    }
    out.extend(adler32(raw).to_be_bytes());
    out
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[4..]);
    out.extend(crc.to_be_bytes());
    out
}

const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn header(width: u32, height: u32, depth: u8, kind: u8, interlace: u8) -> Vec<u8> {
    let mut data = width.to_be_bytes().to_vec();
    data.extend(height.to_be_bytes());
    data.extend([depth, kind, 0, 0, interlace]);
    chunk(b"IHDR", &data)
}

fn file(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    for part in chunks {
        out.extend(part);
    }
    out
}

fn end() -> Vec<u8> {
    chunk(b"IEND", &[])
}

fn channels(kind: u8) -> usize {
    match kind {
        2 => 3,
        4 => 2,
        6 => 4,
        _ => 1,
    }
}

/// An image in PNG's stored form: `kind` is the PNG colour type and each
/// sample fits in `depth` bits.
struct Stored {
    width: usize,
    height: usize,
    depth: u8,
    kind: u8,
    samples: Vec<u16>,
}

/// Palette entries used for indexed fixtures: fewer than 2^depth at depth 8,
/// so out-of-range indices can be tested separately.
fn entries(depth: u8) -> u32 {
    (1u32 << depth).min(200)
}

fn stored(width: usize, height: usize, depth: u8, kind: u8, seed: u64) -> Stored {
    let bound = if kind == 3 {
        entries(depth)
    } else {
        1u32 << depth
    };
    let mut state = seed;
    let samples = (0..width * height * channels(kind))
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) % u64::from(bound)) as u16
        })
        .collect();
    Stored {
        width,
        height,
        depth,
        kind,
        samples,
    }
}

fn palette(depth: u8) -> Vec<[u8; 3]> {
    (0..entries(depth))
        .map(|i| [(i * 7) as u8, (i * 13 + 5) as u8, (255 - i) as u8])
        .collect()
}

/// Samples packed into a scanline: big-endian at 16 bits, most significant
/// bit first below 8.
fn pack(samples: &[u16], depth: u8) -> Vec<u8> {
    match depth {
        16 => samples.iter().flat_map(|s| s.to_be_bytes()).collect(),
        8 => samples.iter().map(|&s| s as u8).collect(),
        _ => {
            let depth = usize::from(depth);
            let mut out = vec![0u8; (samples.len() * depth).div_ceil(8)];
            for (i, &sample) in samples.iter().enumerate() {
                let bit = i * depth;
                out[bit / 8] |= (sample as u8) << (8 - depth - bit % 8);
            }
            out
        }
    }
}

fn paeth(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn filtered(filter: u8, line: &[u8], above: &[u8], step: usize) -> Vec<u8> {
    (0..line.len())
        .map(|i| {
            let a = if i >= step {
                i32::from(line[i - step])
            } else {
                0
            };
            let b = i32::from(above[i]);
            let c = if i >= step {
                i32::from(above[i - step])
            } else {
                0
            };
            let predicted = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                _ => paeth(a, b, c),
            };
            (i32::from(line[i]) - predicted).rem_euclid(256) as u8
        })
        .collect()
}

const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// The decompressed image data: each pass's rows, each with its filter byte.
/// Row `n` overall uses filter `n % 5`.
fn scanlines(image: &Stored, interlaced: bool) -> Vec<u8> {
    let channels = channels(image.kind);
    let step = (channels * usize::from(image.depth) / 8).max(1);
    let passes: &[(usize, usize, usize, usize)] = if interlaced { &ADAM7 } else { &[(0, 0, 1, 1)] };
    let mut out = Vec::new();
    let mut row = 0;
    for &(x0, y0, dx, dy) in passes {
        if image.width <= x0 || image.height <= y0 {
            continue;
        }
        let mut above = Vec::new();
        for y in (y0..image.height).step_by(dy) {
            let samples: Vec<u16> = (x0..image.width)
                .step_by(dx)
                .flat_map(|x| {
                    let at = (y * image.width + x) * channels;
                    image.samples[at..at + channels].to_vec()
                })
                .collect();
            let line = pack(&samples, image.depth);
            if above.is_empty() {
                above = vec![0; line.len()];
            }
            let filter = (row % 5) as u8;
            row += 1;
            out.push(filter);
            out.extend(filtered(filter, &line, &above, step));
            above = line;
        }
    }
    out
}

fn scaled(value: u16, depth: u8) -> u8 {
    match depth {
        16 => ((u32::from(value) + 128) / 257) as u8,
        8 => value as u8,
        _ => (u32::from(value) * 255 / ((1 << depth) - 1)) as u8,
    }
}

/// The 8-bit pixels a decoder must produce. `alpha` holds a palette's tRNS
/// entries; `key` a gray or RGB tRNS colour in stored samples.
fn expected(image: &Stored, alpha: Option<&[u8]>, key: Option<&[u16]>) -> (&'static str, Vec<u8>) {
    let channels = channels(image.kind);
    let colors = palette(image.depth);
    let mut out = Vec::new();
    for pixel in image.samples.chunks(channels) {
        if image.kind == 3 {
            let index = usize::from(pixel[0]);
            out.extend(colors[index]);
            if let Some(alpha) = alpha {
                out.push(alpha.get(index).copied().unwrap_or(255));
            }
            continue;
        }
        out.extend(pixel.iter().map(|&s| scaled(s, image.depth)));
        if let Some(key) = key {
            out.push(if pixel == key { 0 } else { 255 });
        }
    }
    let name = match (image.kind, alpha.is_some() || key.is_some()) {
        (0, false) => "Gray",
        (0, true) | (4, _) => "GrayAlpha",
        (2 | 3, false) => "Rgb",
        _ => "Rgba",
    };
    (name, out)
}

/// The PNG for `image` with its palette, transparency and every row filter.
fn fixture(image: &Stored, interlaced: bool, alpha: Option<&[u8]>, key: Option<&[u16]>) -> Vec<u8> {
    let mut chunks = vec![header(
        image.width as u32,
        image.height as u32,
        image.depth,
        image.kind,
        u8::from(interlaced),
    )];
    if image.kind == 3 {
        let colors: Vec<u8> = palette(image.depth).concat();
        chunks.push(chunk(b"PLTE", &colors));
    }
    if let Some(alpha) = alpha {
        chunks.push(chunk(b"tRNS", alpha));
    }
    if let Some(key) = key {
        let data: Vec<u8> = key.iter().flat_map(|s| s.to_be_bytes()).collect();
        chunks.push(chunk(b"tRNS", &data));
    }
    chunks.push(chunk(b"IDAT", &zlib(&scanlines(image, interlaced))));
    chunks.push(end());
    file(&chunks)
}

fn shown(width: usize, height: usize, (name, pixels): (&str, Vec<u8>)) -> String {
    format!("{width}x{height} {name} {}", hex(&pixels))
}

/// A small valid gray image and its expected decoding.
fn gray() -> (Vec<u8>, String) {
    let image = stored(5, 3, 8, 0, 1);
    let png = fixture(&image, false, None, None);
    (png, shown(5, 3, expected(&image, None, None)))
}

/// Every chunk of a PNG as (type, data); panics on a bad CRC.
fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(png[..8], SIGNATURE);
    let mut at = 8;
    let mut out = Vec::new();
    while at < png.len() {
        let size = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = png[at + 4..at + 8].try_into().unwrap();
        let data = png[at + 8..at + 8 + size].to_vec();
        let crc = u32::from_be_bytes(png[at + 8 + size..at + 12 + size].try_into().unwrap());
        assert_eq!(crc, crc32(&png[at + 4..at + 8 + size]), "CRC of {kind:?}");
        out.push((kind, data));
        at += 12 + size;
    }
    out
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn decodes_every_colour_type_depth_filter_and_interlacing() {
    // 9 x 7 gives every Adam7 pass pixels; widths that are not a multiple of
    // 8 leave padding bits at the end of low-depth rows.
    let combinations: &[(u8, &[u8])] = &[
        (0, &[1, 2, 4, 8, 16]),
        (2, &[8, 16]),
        (3, &[1, 2, 4, 8]),
        (4, &[8, 16]),
        (6, &[8, 16]),
    ];
    let mut body = String::new();
    let mut want = Vec::new();
    for &(kind, depths) in combinations {
        for &depth in depths {
            for (width, height) in [(9, 7), (1, 1), (3, 2)] {
                for interlaced in [false, true] {
                    let seed = u64::from(kind) * 100 + u64::from(depth) + width as u64;
                    let image = stored(width, height, depth, kind, seed);
                    let png = fixture(&image, interlaced, None, None);
                    body.push_str(&format!("print(decoded(\"{}\")?)?\n", hex(&png)));
                    want.push(shown(width, height, expected(&image, None, None)));
                }
            }
        }
    }
    native(&body, &want.join("\n"));
}

#[test]
fn transparency_chunks_add_alpha() {
    let gray = stored(4, 3, 16, 0, 9);
    // Two 16-bit levels that reduce to the same 8-bit value: only the one
    // equal to the key is transparent.
    let mut close = stored(2, 1, 16, 0, 1);
    close.samples = vec![0x1234, 0x1235];
    let rgb = stored(4, 3, 8, 2, 4);
    let mut keyed = rgb.samples.clone();
    keyed[3..6].copy_from_slice(&rgb.samples[0..3]);
    let rgb = Stored {
        samples: keyed,
        ..rgb
    };
    let indexed = stored(6, 5, 4, 3, 3);
    let low = stored(9, 2, 2, 0, 5);
    // Image, tRNS palette alphas, tRNS colour key, interlaced.
    type Case<'a> = (&'a Stored, Option<&'a [u8]>, Option<Vec<u16>>, bool);
    let cases: Vec<Case> = vec![
        (&gray, None, Some(vec![gray.samples[0]]), false),
        (&close, None, Some(vec![0x1234]), false),
        (&rgb, None, Some(rgb.samples[0..3].to_vec()), true),
        (&indexed, Some(&[0, 128, 255, 7][..]), None, false),
        (&indexed, Some(&[9; 16][..]), None, true),
        (&low, None, Some(vec![low.samples[1]]), true),
    ];
    let mut body = String::new();
    let mut want = Vec::new();
    for (image, alpha, key, interlaced) in &cases {
        let png = fixture(image, *interlaced, *alpha, key.as_deref());
        body.push_str(&format!("print(decoded(\"{}\")?)?\n", hex(&png)));
        want.push(shown(
            image.width,
            image.height,
            expected(image, *alpha, key.as_deref()),
        ));
    }
    let output = output(&program(&body));
    assert_eq!(output, want.join("\n"));
    assert!(
        output.lines().nth(1).unwrap().ends_with("120012ff"),
        "{output}"
    );
}

#[test]
fn chunk_layouts_that_decoders_must_accept() {
    let image = stored(6, 4, 8, 2, 77);
    let data = zlib(&scanlines(&image, false));
    let want = shown(6, 4, expected(&image, None, None));
    let ihdr = header(6, 4, 8, 2, 0);
    // One IDAT per byte, with empty IDATs between.
    let mut split = vec![ihdr.clone(), chunk(b"IDAT", &[])];
    for &byte in &data {
        split.push(chunk(b"IDAT", &[byte]));
        split.push(chunk(b"IDAT", &[]));
    }
    split.push(end());
    let ancillary = vec![
        ihdr.clone(),
        chunk(b"tEXt", b"Comment\0made by hand"),
        chunk(b"gAMA", &45455u32.to_be_bytes()),
        // A suggested palette for a truecolour image is ignored.
        chunk(b"PLTE", &[1, 2, 3]),
        chunk(b"prVt", b"private, safe to copy"),
        chunk(b"IDAT", &data),
        chunk(b"tIME", &[7, 234, 10, 30, 12, 0, 0]),
        end(),
    ];
    let mut trailing = file(&[ihdr.clone(), chunk(b"IDAT", &data), end()]);
    trailing.extend(b"bytes after IEND are ignored");
    let body = [file(&split), file(&ancillary), trailing]
        .iter()
        .map(|png| format!("print(decoded(\"{}\")?)?\n", hex(png)))
        .collect::<String>();
    native(&body, &[want.as_str(); 3].join("\n"));
}

#[test]
fn malformed_inputs_are_rejected() {
    let image = stored(5, 3, 8, 0, 1);
    let rows = scanlines(&image, false);
    let data = zlib(&rows);
    let good = header(5, 3, 8, 0, 0);
    let idat = chunk(b"IDAT", &data);
    let with = |middle: &[Vec<u8>]| {
        let mut chunks = vec![good.clone()];
        chunks.extend(middle.iter().cloned());
        chunks.push(end());
        file(&chunks)
    };
    let mut bad_crc = file(&[good.clone(), idat.clone(), end()]);
    let crc_at = 8 + 25 - 1;
    bad_crc[crc_at] ^= 1;
    let mut bad_adler = data.clone();
    *bad_adler.last_mut().unwrap() ^= 1;
    let mut bad_filter = rows.clone();
    bad_filter[0] = 5;
    let mut long = rows.clone();
    long.extend([0, 1, 2, 3, 4, 5]);
    let indexed = stored(4, 2, 8, 3, 2);
    let mut out_of_range = indexed.samples.clone();
    out_of_range[5] = 200;
    let indexed_bad = Stored {
        samples: out_of_range,
        ..stored(4, 2, 8, 3, 2)
    };
    let indexed_rows = zlib(&scanlines(&indexed_bad, false));
    let plte = chunk(b"PLTE", &palette(8).concat());
    let indexed_header = header(4, 2, 8, 3, 0);
    let indexed_file = |middle: &[Vec<u8>]| {
        let mut chunks = vec![indexed_header.clone()];
        chunks.extend(middle.iter().cloned());
        chunks.push(chunk(b"IDAT", &zlib(&scanlines(&indexed, false))));
        chunks.push(end());
        file(&chunks)
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("Unsupported", Vec::new()),
        ("Unsupported", SIGNATURE[..7].to_vec()),
        ("Unsupported", {
            let mut wrong = with(std::slice::from_ref(&idat));
            wrong[1] = b'Q';
            wrong
        }),
        ("Checksum", bad_crc),
        ("Checksum", with(&[chunk(b"IDAT", &bad_adler)])),
        ("Corrupt", file(&[idat.clone(), end()])),
        (
            "Corrupt",
            file(&[chunk(b"IHDR", &[0; 12]), idat.clone(), end()]),
        ),
        (
            "Corrupt",
            file(&[header(0, 3, 8, 0, 0), idat.clone(), end()]),
        ),
        (
            "Corrupt",
            file(&[header(5, 0x8000_0000, 8, 0, 0), idat.clone(), end()]),
        ),
        (
            "Corrupt",
            file(&[header(5, 3, 3, 0, 0), idat.clone(), end()]),
        ),
        (
            "Corrupt",
            file(&[header(5, 3, 16, 3, 0), idat.clone(), end()]),
        ),
        (
            "Corrupt",
            file(&[header(5, 3, 8, 5, 0), idat.clone(), end()]),
        ),
        (
            "Corrupt",
            file(&[header(5, 3, 8, 0, 2), idat.clone(), end()]),
        ),
        ("Corrupt", {
            let mut method = header(5, 3, 8, 0, 0);
            method[8 + 10] = 1;
            let crc = crc32(&method[4..21]);
            method[21..25].copy_from_slice(&crc.to_be_bytes());
            file(&[method, idat.clone(), end()])
        }),
        ("Corrupt", with(&[good.clone(), idat.clone()])),
        ("Corrupt", with(&[end(), idat.clone()])),
        ("Corrupt", with(&[chunk(b"IDAT", &zlib(&bad_filter))])),
        ("Corrupt", with(&[chunk(b"IDAT", &zlib(&long))])),
        (
            "Corrupt",
            with(&[chunk(b"IDAT", &zlib(&rows[..rows.len() - 1]))]),
        ),
        (
            "Corrupt",
            with(&[chunk(b"IDAT", &[0x78, 0x01, 0xff, 0xff])]),
        ),
        (
            "Corrupt",
            with(&[
                chunk(b"IDAT", &data[..10]),
                chunk(b"tEXt", b"a\0b"),
                chunk(b"IDAT", &data[10..]),
            ]),
        ),
        ("Corrupt", with(&[idat.clone(), chunk(b"IDAT", &data)])),
        (
            "Corrupt",
            file(&[good.clone(), idat.clone(), chunk(b"IEND", &[0])]),
        ),
        ("Corrupt", with(&[chunk(b"tE1t", b""), idat.clone()])),
        ("Corrupt", {
            let mut huge = with(std::slice::from_ref(&idat));
            huge[33..37].copy_from_slice(&0x8000_0000u32.to_be_bytes());
            huge
        }),
        ("Corrupt", with(&[chunk(b"PLTE", &[1, 2, 3]), idat.clone()])),
        ("Corrupt", with(&[chunk(b"tRNS", &[0, 1, 2]), idat.clone()])),
        (
            "Corrupt",
            with(&[
                chunk(b"tRNS", &[0, 1]),
                chunk(b"tRNS", &[0, 1]),
                idat.clone(),
            ]),
        ),
        (
            "Corrupt",
            file(&[indexed_header.clone(), chunk(b"IDAT", &indexed_rows), end()]),
        ),
        (
            "Corrupt",
            file(&[
                indexed_header.clone(),
                plte.clone(),
                chunk(b"IDAT", &indexed_rows),
                end(),
            ]),
        ),
        ("Corrupt", indexed_file(&[chunk(b"PLTE", &[1, 2, 3, 4])])),
        ("Corrupt", indexed_file(&[chunk(b"PLTE", &[])])),
        ("Corrupt", indexed_file(&[plte.clone(), plte.clone()])),
        (
            "Corrupt",
            indexed_file(&[chunk(b"tRNS", &[1]), plte.clone()]),
        ),
        (
            "Corrupt",
            indexed_file(&[plte.clone(), chunk(b"tRNS", &[1; 201])]),
        ),
        ("Corrupt", {
            let low = header(4, 2, 1, 3, 0);
            file(&[
                low,
                chunk(b"PLTE", &[0; 9]),
                chunk(b"IDAT", &zlib(&[0, 0, 0, 0])),
                end(),
            ])
        }),
        (
            "Unsupported",
            with(&[chunk(b"ABCD", b"critical"), idat.clone()]),
        ),
        ("Unsupported", with(&[idat.clone(), chunk(b"ZZZZ", b"")])),
        ("Truncated", file(&[good.clone(), idat.clone()])),
        ("Truncated", file(std::slice::from_ref(&good))),
    ];
    let body = cases
        .iter()
        .map(|(_, png)| format!("print(decoded(\"{}\")?)?\n", hex(png)))
        .collect::<String>();
    let want: Vec<&str> = cases.iter().map(|(error, _)| *error).collect();
    let got = output(&program(&body));
    for (index, (line, want)) in got.lines().zip(&want).enumerate() {
        assert_eq!(line, *want, "case {index}");
    }
    assert_eq!(got.lines().count(), want.len());
}

#[test]
fn every_truncation_is_reported() {
    // Each proper prefix fails: short of the signature it is not a PNG, and
    // after it the input ends before IEND.
    for interlaced in [false, true] {
        let image = stored(5, 4, 8, 6, 31);
        let png = fixture(&image, interlaced, None, None);
        let body = format!(
            r#"
data = unhex("{}")?
for count in range(0, data.len()):
    print(count, decoded(&hex(&data.slice(0, count)?)?)?, described(&hex(&data.slice(0, count)?)?)?)?
"#,
            hex(&png)
        );
        let got = output(&program(&body));
        let header_end = 8 + 25;
        let idat_end = png.len() - 12;
        for (count, line) in got.lines().enumerate() {
            let parts: Vec<&str> = line.split(' ').collect();
            assert_eq!(parts[0], count.to_string());
            let decoded = if count < 8 {
                "Unsupported"
            } else {
                "Truncated"
            };
            assert_eq!(parts[1], decoded, "{line}");
            let described = if count < 8 {
                "Unsupported"
            } else if count < idat_end {
                "Truncated"
            } else {
                "Format.Png"
            };
            assert_eq!(parts[2], described, "{line}");
            assert!(header_end < idat_end);
        }
        assert_eq!(got.lines().count(), png.len());
    }
}

#[test]
fn detect_and_info_read_only_the_header() {
    let indexed = fixture(&stored(9, 7, 4, 3, 1), true, None, None);
    let keyed = fixture(&stored(3, 2, 16, 2, 1), false, None, Some(&[1, 2, 3]));
    // A header promising far more than the limits allow, with no real data:
    // `info` reads it, and `decode` stops before allocating.
    let huge = file(&[header(100000, 100000, 8, 6, 0), chunk(b"IDAT", &[]), end()]);
    let jpeg = "ffd8ffe000104a464946";
    let gif = hex(b"GIF89a\x01\x00\x01\x00");
    let old_gif = hex(b"GIF87a");
    let webp = hex(b"RIFF\x24\x00\x00\x00WEBPVP8 ");
    let body = format!(
        r#"
for text in ["{png}", "{jpeg}", "{gif}", "{old_gif}", "{webp}", "52494646", "", "89504e47"]?:
    print(str.repr(std.image.detect(&unhex(text)?))?)?
print(described("{indexed}")?)?
print(described("{keyed}")?)?
print(described("{huge}")?)?
print(described("{jpeg}")?)?
print(decoded("{huge}")?)?
print(decoded("{jpeg}")?)?
print(decoded("{gif}")?)?
print(decoded("{webp}")?)?
"#,
        png = hex(&gray().0),
        indexed = hex(&indexed),
        keyed = hex(&keyed),
        huge = hex(&huge),
    );
    native(
        &body,
        "Option[Format].Some(Format.Png)\nOption[Format].Some(Format.Jpeg)\n\
         Option[Format].Some(Format.Gif)\nOption[Format].Some(Format.Gif)\n\
         Option[Format].Some(Format.WebP)\nOption[Format].Nothing\nOption[Format].Nothing\n\
         Option[Format].Nothing\n\
         Format.Png 9x7 Rgb 4 True\nFormat.Png 3x2 Rgba 16 False\nFormat.Png 100000x100000 Rgba 8 False\n\
         Unsupported\nLimitExceeded\nUnsupported\nUnsupported\nUnsupported",
    );
}

#[test]
fn limits_bound_width_height_and_bytes() {
    let (png, want) = gray();
    let png = hex(&png);
    // The 5 x 3 gray image needs 15 bytes.
    native(
        &format!(
            r#"
data = unhex("{png}")?
for limits in [Limits(5, 3, 15), Limits(4, 3, 15), Limits(5, 2, 15), Limits(5, 3, 14), Limits(-1, 3, 15), Limits(5, 3, -1)]?:
    print(shown(std.image.decode(&data, &limits))?)?
defaults = std.image.limits()
print(defaults.max_width, defaults.max_height, defaults.max_bytes)?
print(str.repr(defaults.check(8192, 8192, ColorType.Rgba))?)?
print(str.repr(defaults.check(8193, 8192, ColorType.Rgba))?)?
print(str.repr(defaults.check(16384, 16384, ColorType.Gray))?)?
print(str.repr(defaults.check(16385, 1, ColorType.Gray))?)?
print(str.repr(defaults.check(0, 1, ColorType.Gray))?)?
widest = Limits(i64.MAX, i64.MAX, i64.MAX)
print(str.repr(widest.check(2147483647, 2147483647, ColorType.Gray))?)?
print(str.repr(widest.check(2147483647, 2147483647, ColorType.Rgba))?)?
print(str.repr(widest.check(2147483648, 1, ColorType.Gray))?)?
"#
        ),
        &format!(
            "{want}\nLimitExceeded\nLimitExceeded\nLimitExceeded\nOutOfRange\nOutOfRange\n\
             16384 16384 268435456\nResult[(), ImageError].Ok(())\n\
             Result[(), ImageError].Err(LimitExceeded)\nResult[(), ImageError].Ok(())\n\
             Result[(), ImageError].Err(LimitExceeded)\nResult[(), ImageError].Err(OutOfRange)\n\
             Result[(), ImageError].Ok(())\nResult[(), ImageError].Err(LimitExceeded)\n\
             Result[(), ImageError].Err(OutOfRange)"
        ),
    );
}

#[test]
fn pixel_buffers_keep_their_dimensions() {
    native(
        r#"
mut image = std.image.new(3, 2, ColorType.GrayAlpha)?
print(image.width(), image.height(), named(image.color()), hex(image.pixels())?)?
image.set(2, 1, 1, 9u8)?
print(image.get(2, 1, 1)?, image.get(0, 0, 0)?)?
mut twin = image.copy()?
twin.set(0, 0, 0, 7u8)?
print(named(twin.color()), hex(twin.pixels())?, hex(image.pixels())?)?
for position in [(3, 0, 0), (0, 2, 0), (0, 0, 2), (-1, 0, 0)]?:
    x, y, c = position
    print(str.repr(image.get(x, y, c))?, str.repr(image.set(x, y, c, 1u8))?)?
pixels = image.into_pixels()
print(hex(&pixels)?)?
print(shown(std.image.from_pixels(1, 1, ColorType.Rgb, unhex("010203")?))?)?
print(shown(std.image.from_pixels(1, 1, ColorType.Rgb, unhex("0102")?))?)?
print(shown(std.image.from_pixels(1, 1, ColorType.Rgb, unhex("01020304")?))?)?
print(shown(std.image.new(0, 1, ColorType.Gray))?)?
print(shown(std.image.new(1, -1, ColorType.Gray))?)?
print(shown(std.image.new(2147483648, 1, ColorType.Gray))?)?
print(shown(std.image.new(2147483647, 2147483647, ColorType.Rgba))?)?
print(ColorType.Gray.channels(), ColorType.GrayAlpha.channels(), ColorType.Rgb.channels(), ColorType.Rgba.channels())?
print(ColorType.Gray.has_alpha(), ColorType.GrayAlpha.has_alpha(), ColorType.Rgb.has_alpha(), ColorType.Rgba.has_alpha())?
"#,
        "3 2 GrayAlpha 000000000000000000000000\n9 0\n\
         GrayAlpha 070000000000000000000009 000000000000000000000009\n\
         Result[u8, ImageError].Err(OutOfRange) Result[(), ImageError].Err(OutOfRange)\n\
         Result[u8, ImageError].Err(OutOfRange) Result[(), ImageError].Err(OutOfRange)\n\
         Result[u8, ImageError].Err(OutOfRange) Result[(), ImageError].Err(OutOfRange)\n\
         Result[u8, ImageError].Err(OutOfRange) Result[(), ImageError].Err(OutOfRange)\n\
         000000000000000000000009\n1x1 Rgb 010203\nOutOfRange\nOutOfRange\nOutOfRange\n\
         OutOfRange\nOutOfRange\nLimitExceeded\n1 2 3 4\nFalse True False True",
    );
}

#[test]
fn crop_copies_a_rectangle() {
    native(
        r#"
image = made(3, 3, ColorType.Gray, "010203040506070809")?
print(shown(image.crop(1, 1, 2, 2))?)?
print(shown(image.crop(0, 2, 3, 1))?)?
print(shown(image.crop(0, 0, 3, 3))?)?
rgb = made(2, 2, ColorType.Rgb, "0a0b0c1415161e1f20282930")?
print(shown(rgb.crop(1, 0, 1, 2))?)?
for box in [(2, 2, 2, 1), (0, 0, 4, 1), (-1, 0, 1, 1), (0, 0, 0, 1), (0, 3, 1, 1)]?:
    x, y, w, h = box
    print(shown(image.crop(x, y, w, h))?)?
"#,
        "2x2 Gray 05060809\n3x1 Gray 070809\n3x3 Gray 010203040506070809\n\
         1x2 Rgb 141516282930\nOutOfRange\nOutOfRange\nOutOfRange\nOutOfRange\nOutOfRange",
    );
}

/// BT.709 luma, rounded, as `convert` computes it.
fn luma(r: u32, g: u32, b: u32) -> u8 {
    ((2126 * r + 7152 * g + 722 * b + 5000) / 10000) as u8
}

#[test]
fn convert_moves_between_every_colour_type() {
    // Two pixels in each source type; every source and target pair.
    let sources = [
        ("Gray", "10f0"),
        ("GrayAlpha", "108020ff"),
        ("Rgb", "ff000010e0f0"),
        ("Rgba", "ff000040102030c0"),
    ];
    let names = ["Gray", "GrayAlpha", "Rgb", "Rgba"];
    let mut body = String::new();
    let mut want = Vec::new();
    for (source, pixels) in sources {
        let bytes = unhex(pixels);
        let per = bytes.len() / 2;
        body.push_str(&format!(
            "{source}_image = made(2, 1, ColorType.{source}, \"{pixels}\")?\n"
        ));
        for target in names {
            body.push_str(&format!(
                "print(shown({source}_image.convert(ColorType.{target}))?)?\n"
            ));
            let mut out = Vec::new();
            for pixel in bytes.chunks(per) {
                let (r, g, b) = if per < 3 {
                    (pixel[0], pixel[0], pixel[0])
                } else {
                    (pixel[0], pixel[1], pixel[2])
                };
                let alpha = if per.is_multiple_of(2) {
                    pixel[per - 1]
                } else {
                    255
                };
                if target.starts_with("Gray") {
                    out.push(if per < 3 {
                        r
                    } else {
                        luma(r.into(), g.into(), b.into())
                    });
                } else {
                    out.extend([r, g, b]);
                }
                if target.ends_with("Alpha") || target == "Rgba" {
                    out.push(alpha);
                }
            }
            want.push(format!("2x1 {target} {}", hex(&out)));
        }
    }
    native(&body, &want.join("\n"));
    assert_eq!(luma(255, 0, 0), 54);
}

#[test]
fn resize_samples_and_filters() {
    native(
        r#"
row = made(2, 1, ColorType.Gray, "00c8")?
print(shown(row.resize(4, 1, Filter.Bilinear))?)?
print(shown(row.resize(4, 1, Filter.Nearest))?)?
print(shown(row.resize(1, 1, Filter.Bilinear))?)?
print(shown(row.resize(2, 3, Filter.Bilinear))?)?
square = made(4, 4, ColorType.Gray, "000102030405060708090a0b0c0d0e0f")?
print(shown(square.resize(2, 2, Filter.Nearest))?)?
print(shown(square.resize(8, 1, Filter.Nearest))?)?
# A transparent neighbour does not tint an opaque pixel.
edge = made(2, 1, ColorType.Rgba, "ff0000ff00ff0000")?
print(shown(edge.resize(4, 1, Filter.Bilinear))?)?
fade = made(2, 1, ColorType.GrayAlpha, "ff800000")?
print(shown(fade.resize(4, 1, Filter.Bilinear))?)?
# A uniform image stays uniform at any size.
mut flat = std.image.new(7, 5, ColorType.Rgb)?
for y in range(0, 5):
    for x in range(0, 7):
        flat.set(x, y, 0, 90u8)?
        flat.set(x, y, 1, 180u8)?
        flat.set(x, y, 2, 255u8)?
for size in [(3, 2), (13, 11), (1, 1), (7, 9), (2, 5)]?:
    w, h = size
    resized = flat.resize(w, h, Filter.Bilinear)?
    mut uniform = True
    for y in range(0, h):
        for x in range(0, w):
            uniform = uniform and resized.get(x, y, 0)? == 90u8 and resized.get(x, y, 1)? == 180u8 and resized.get(x, y, 2)? == 255u8
    print(w, h, uniform)?
print(shown(row.resize(2, 1, Filter.Nearest))?)?
print(shown(row.resize(0, 1, Filter.Bilinear))?)?
print(shown(row.resize(1, 2147483648, Filter.Nearest))?)?
"#,
        "4x1 Gray 003296c8\n4x1 Gray 0000c8c8\n1x1 Gray 64\n2x3 Gray 00c800c800c8\n\
         2x2 Gray 05070d0f\n8x1 Gray 080809090a0a0b0b\n\
         4x1 Rgba ff0000ffff0000bfff00004000000000\n4x1 GrayAlpha ff80ff60ff200000\n\
         3 2 True\n13 11 True\n1 1 True\n7 9 True\n2 5 True\n2x1 Gray 00c8\nOutOfRange\nOutOfRange",
    );
}

#[test]
fn encoded_pngs_round_trip_and_are_well_formed() {
    let body = r#"
for color in [ColorType.Gray, ColorType.GrayAlpha, ColorType.Rgb, ColorType.Rgba]?:
    for level in [0, 1, 6, 9]?:
        print(trip(&noisy(13, 7, color, 99)?, level)? and trip(&smooth(13, 7, color)?, level)? and trip(&noisy(1, 1, color, 5)?, level)?)?
big = smooth(64, 64, ColorType.Rgb)?
stored = std.image.encode_png(&big, 0)?
packed = std.image.encode_png(&big, 9)?
print(packed.len() * 10 < stored.len())?
print(str.repr(std.image.encode_png(&big, 10))?)?
print(str.repr(std.image.encode_png(&big, -1))?)?
print(hex(&std.image.encode_png(&smooth(3, 2, ColorType.GrayAlpha)?)?)?)?
print(hex(&std.image.encode_png(&noisy(200, 120, ColorType.Rgba, 7)?, 6)?)?)?
"#;
    let got = output(&program(body));
    let lines: Vec<&str> = got.lines().collect();
    assert_eq!(lines[..17], ["True"; 17], "{got}");
    assert_eq!(
        lines[17..19],
        [
            "Result[std.bytes.Bytes, ImageError].Err(OutOfRange)",
            "Result[std.bytes.Bytes, ImageError].Err(OutOfRange)"
        ]
    );
    // The structure, checked independently of the decoder.
    let small = chunks(&unhex(lines[19]));
    assert_eq!(small[0].0, *b"IHDR");
    assert_eq!(small[0].1, [0, 0, 0, 3, 0, 0, 0, 2, 8, 4, 0, 0, 0]);
    assert_eq!(small.last().unwrap(), &(*b"IEND", Vec::new()));
    // Noise does not compress, so 96000 bytes need two IDAT chunks.
    let large = chunks(&unhex(lines[20]));
    let idats: Vec<_> = large.iter().filter(|(kind, _)| kind == b"IDAT").collect();
    assert!(idats.len() >= 2, "{} IDAT chunks", idats.len());
    assert_eq!(idats[0].1[0], 0x78);
    let kinds: Vec<[u8; 4]> = large.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(kinds[0], *b"IHDR");
    assert!(kinds[1..kinds.len() - 1].iter().all(|kind| kind == b"IDAT"));
}

/// Runs a Python script with Pillow, or `None` when either is missing.
fn pillow(script: &str, input: &str) -> Option<String> {
    use std::io::Write;
    let probe = std::process::Command::new("python3")
        .args(["-c", "import PIL"])
        .output()
        .ok()?;
    if !probe.status.success() {
        return None;
    }
    let mut child = std::process::Command::new("python3")
        .args(["-c", script])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_string();
    let feeder = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output().unwrap();
    feeder.join().unwrap().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(String::from_utf8(output.stdout).unwrap())
}

#[test]
fn pillow_reads_our_pngs_and_we_read_its_pngs() {
    // Pillow writes compressed, filtered, interlaced and paletted files; each
    // line is `PNG-HEX EXPECTED` where EXPECTED is how `shown` prints it.
    let writer = r#"
import io, random
from PIL import Image
random.seed(5)
names = {'L': 'Gray', 'LA': 'GrayAlpha', 'RGB': 'Rgb', 'RGBA': 'Rgba'}
def noise(mode, w, h):
    im = Image.new(mode, (w, h))
    bands = len(im.getbands())
    im.putdata([tuple(random.randrange(256) for _ in range(bands)) if bands > 1 else random.randrange(256) for _ in range(w * h)])
    return im
def emit(im, expected, **options):
    out = io.BytesIO()
    im.save(out, 'PNG', **options)
    print(out.getvalue().hex(), f'{expected.width}x{expected.height}', names[expected.mode], expected.tobytes().hex())
for mode in ['L', 'LA', 'RGB', 'RGBA']:
    im = noise(mode, 21, 13)
    emit(im, im, optimize=True)
    emit(im, im, interlace=1)
    smooth = Image.linear_gradient('L').resize((40, 30)).convert(mode)
    emit(smooth, smooth, compress_level=9)
p = noise('P', 17, 9)
p.putpalette([random.randrange(256) for _ in range(768)])
emit(p, p.convert('RGB'))
emit(p, p.convert('RGBA'), transparency=bytes([0, 64, 128, 255, 3]))
one = noise('L', 19, 5).convert('1')
emit(one, one.convert('L'))
"#;
    let Some(fixtures) = pillow(writer, "") else {
        eprintln!("skipping: python3 with Pillow is not installed");
        return;
    };
    let mut body = String::new();
    let mut want = Vec::new();
    for line in fixtures.lines() {
        let (png, expected) = line.split_once(' ').unwrap();
        body.push_str(&format!("print(decoded(\"{png}\")?)?\n"));
        want.push(expected.to_string());
    }
    native(&body, &want.join("\n"));

    // Pillow decodes what we encode, at every level.
    let encoded = output(&program(
        r#"
for color in [ColorType.Gray, ColorType.GrayAlpha, ColorType.Rgb, ColorType.Rgba]?:
    for level in [0, 1, 9]?:
        image = noisy(23, 11, color, 3 + level)?
        print(hex(&std.image.encode_png(&image, level)?)?, named(color), hex(image.pixels())?)?
        gradient = smooth(31, 17, color)?
        print(hex(&std.image.encode_png(&gradient, level)?)?, named(color), hex(gradient.pixels())?)?
"#,
    ));
    let reader = r#"
import io, sys
from PIL import Image
modes = {'Gray': 'L', 'GrayAlpha': 'LA', 'Rgb': 'RGB', 'Rgba': 'RGBA'}
for line in sys.stdin:
    png, name, pixels = line.split()
    im = Image.open(io.BytesIO(bytes.fromhex(png)))
    im.load()
    print(im.mode == modes[name] and im.tobytes().hex() == pixels)
"#;
    let checked = pillow(reader, &encoded).unwrap();
    assert_eq!(checked, "True\n".repeat(24));
}

#[test]
fn bilinear_resizing_matches_pillow() {
    // Each line is `PNG-HEX WIDTH HEIGHT CHANNELS RESIZED-HEX`, Pillow's
    // bilinear resize of a noise image. Without alpha the results are equal.
    // Both filter alpha images premultiplied but round the premultiplication
    // differently, so there samples may differ by 1.
    let writer = r#"
import io, random
from PIL import Image
random.seed(9)
for mode in ['L', 'LA', 'RGB', 'RGBA']:
    im = Image.new(mode, (23, 17))
    bands = len(im.getbands())
    im.putdata([tuple(random.randrange(256) for _ in range(bands)) if bands > 1 else random.randrange(256) for _ in range(23 * 17)])
    out = io.BytesIO()
    im.save(out, 'PNG')
    for w, h in [(7, 5), (40, 30), (23, 9), (11, 17), (3, 41), (1, 1)]:
        print(out.getvalue().hex(), w, h, bands, im.resize((w, h), Image.BILINEAR).tobytes().hex())
"#;
    let Some(fixtures) = pillow(writer, "") else {
        eprintln!("skipping: python3 with Pillow is not installed");
        return;
    };
    let mut body = String::from("limits = std.image.limits()\n");
    let mut cases = Vec::new();
    for line in fixtures.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        let [png, width, height, channels, resized] = fields[..] else {
            panic!("{line}");
        };
        body.push_str(&format!(
            "print(shown(std.image.decode(&unhex(\"{png}\")?, &limits)?.resize({width}, {height}, Filter.Bilinear))?)?\n"
        ));
        cases.push((channels.parse::<usize>().unwrap(), unhex(resized)));
    }
    let got = output(&program(&body));
    let lines: Vec<&str> = got.lines().collect();
    assert_eq!(lines.len(), cases.len(), "{got}");
    for (line, (channels, want)) in lines.iter().zip(&cases) {
        // `shown` prints `WIDTHxHEIGHT COLOUR PIXELS`.
        let ours = unhex(line.rsplit(' ').next().unwrap());
        assert_eq!(ours.len(), want.len(), "{line}");
        if channels % 2 == 1 {
            assert_eq!(&ours, want);
        } else {
            let close = ours.iter().zip(want).all(|(a, b)| a.abs_diff(*b) <= 1);
            assert!(close, "{ours:?}\n{want:?}");
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn limits_are_checked_before_any_allocation() {
    let huge = file(&[header(100000, 100000, 8, 6, 0), chunk(b"IDAT", &[]), end()]);
    native(
        &format!(
            r#"
data = unhex("{}")?
limits = std.image.limits()
print("__test_begin_no_allocations__")?
result = std.image.decode(&data, &limits)
print("__test_end_no_allocations__")?
print(str.repr(result.is_err())?)?
"#,
            hex(&huge)
        ),
        "True",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_without_leaks() {
    // Every operation is retried with allocation failing after 0, 1, 2, ...
    // successful allocations. Each attempt must report Allocation or succeed,
    // and the exit-time balance check verifies that failed attempts leak
    // nothing.
    let plain = fixture(&stored(9, 7, 8, 2, 1), false, None, None);
    let interlaced = fixture(&stored(9, 7, 4, 3, 2), true, Some(&[1, 2, 3]), None);
    let body = format!(
        r#"
plain = unhex("{}")?
interlaced = unhex("{}")?
limits = std.image.limits()
image = smooth(9, 7, ColorType.Rgba)?
for operation in range(0, 10):
    for after in range(0, 40):
        marker = f"__test_fail_allocations_after_{{after}}__"?
        print(marker)?
        mut result: Result[Image, ImageError] = Err(ImageError.OutOfRange)
        match operation:
            case 0:
                result = std.image.decode(&plain, &limits)
            case 1:
                result = std.image.decode(&interlaced, &limits)
            case 2:
                match std.image.encode_png(&image, 6):
                    case Ok(png):
                        result = std.image.decode(&png, &limits)
                    case Err(error):
                        result = Err(error)
            case 3:
                result = image.crop(1, 1, 3, 3)
            case 4:
                result = image.convert(ColorType.Gray)
            case 5:
                result = image.resize(4, 3, Filter.Nearest)
            case 6:
                result = image.resize(20, 3, Filter.Bilinear)
            case 7:
                result = std.image.new(5, 5, ColorType.Rgb)
            case 8:
                match std.image.info(&interlaced):
                    case Ok(_):
                        result = std.image.new(1, 1, ColorType.Gray)
                    case Err(error):
                        result = Err(error)
            case _:
                result = image.copy()
        print("__test_restore_allocations__")?
        text = shown(result)?
        if not text.endswith(".Allocation(AllocError.OutOfMemory)"):
            print(operation, after, text)?
            break
"#,
        hex(&plain),
        hex(&interlaced)
    );
    let got = output(&program(&body));
    let lines: Vec<&str> = got.lines().collect();
    assert_eq!(lines.len(), 10, "{got}");
    for (operation, line) in lines.iter().enumerate() {
        let parts: Vec<&str> = line.split(' ').collect();
        assert_eq!(parts[0], operation.to_string(), "{got}");
        // Success prints the image, `WIDTHxHEIGHT COLOUR PIXELS`.
        assert!(
            parts[2].contains('x') && parts.len() == 5,
            "an error other than Allocation: {got}"
        );
        // Operations that allocate fail at the first allocation.
        if operation != 8 {
            assert_ne!(parts[1], "0", "operation {operation} did not allocate");
        }
    }
}
