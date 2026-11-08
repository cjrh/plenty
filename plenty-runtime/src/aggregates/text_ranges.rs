//! Existing text builtins over checked UTF-8 ranges. Inputs remain borrowed;
//! builders allocate only owned results and never fabricate owning headers.
use super::{Collection, Type, fallible, text_views, try_text_pieces, wrap};
use crate::collection_opcodes::{TEXT_RANGE_BASE, TEXT_SLICE};
use crate::strings::{self, Text};

/// The original builtin's input types, result metadata and trap site apply.
/// Every text operand is instead a live, scalar-aligned UTF-8 range.
///
/// # Safety
/// The compiler holds every input loan through this call and supplies metadata
/// for the original operation, including iterable and numeric layouts.
pub(super) unsafe fn call(
    op: i64,
    [a, b, c, d, e]: [u128; 5],
    descriptor: *const Type,
    site: *const crate::Site,
) -> u128 {
    // SAFETY: the checked compiler operands satisfy the contract above.
    unsafe {
        let op = op - TEXT_RANGE_BASE;
        let source = text_views::text(a);
        match op {
            34 => {
                fallible(strings::try_concat_range(source, text_views::text(b)).map(|t| t as u128))
            }
            35 => {
                let result = match (*descriptor).kind {
                    b's' => strings::try_join_scalars_range(source, text_views::text(b)),
                    b'G' => strings::try_join_generator_range(
                        source,
                        b as *mut crate::generators::Generator,
                    ),
                    b'V' => {
                        let element = (*descriptor).key();
                        let stride = crate::list_layout::row_bytes(
                            element.kind,
                            element.inline_bytes as usize,
                            element.has_inline_range(),
                        );
                        let rows = b as u64 as *const u8;
                        let pieces = (0..(b >> 64) as usize).map(|i| {
                            crate::entries::dense_element(rows.add(i * stride), element)
                                as *const Text
                        });
                        strings::try_join_range(source, pieces)
                    }
                    _ => {
                        let collection = &*(b as *const Collection);
                        strings::try_join_range(
                            source,
                            collection
                                .entries
                                .iter()
                                .map(|entry| entry.key as *const Text),
                        )
                    }
                };
                fallible(result.map(|t| t as u128))
            }
            36 => {
                let separator = text_views::text(b);
                if separator.is_empty() {
                    crate::fail_at(
                        site,
                        format_args!("string split requires a nonempty separator"),
                    );
                }
                fallible(try_text_pieces(
                    source.split(separator),
                    (*descriptor).variants[0].fields[0],
                ))
            }
            37 => {
                let index = b as i64;
                let position = if index >= 0 {
                    Some(index as usize)
                } else {
                    source
                        .chars()
                        .count()
                        .checked_sub(index.unsigned_abs() as usize)
                };
                match position.and_then(|at| source.chars().nth(at)) {
                    Some(character) => wrap(
                        strings::pack(character.encode_utf8(&mut [0; 4]).as_bytes()) as u128,
                        1,
                    ),
                    None => wrap(0, 0),
                }
            }
            46 => {
                let range = super::slice_bounds(b as i64, c as i64, source.chars().count());
                fallible(
                    strings::try_stepped_slice_range(source, (range.start, 1, range.len()))
                        .map(|t| t as u128),
                )
            }
            47 => fallible(
                strings::try_replace_range(source, text_views::text(b), text_views::text(c))
                    .map(|t| t as u128),
            ),
            48 => source.starts_with(text_views::text(b)) as u128,
            49 => source.ends_with(text_views::text(b)) as u128,
            50 | 51 => {
                let needle = text_views::text(b);
                let found = if op == 50 {
                    source.find(needle)
                } else {
                    source.rfind(needle)
                };
                found.map_or(wrap(0, 0), |at| {
                    wrap(source[..at].chars().count() as u128, 1)
                })
            }
            52 => source.matches(text_views::text(b)).count() as u128,
            53..=55 => {
                fallible(strings::try_strip_range(source, op != 55, op != 54).map(|t| t as u128))
            }
            56 => fallible(strings::try_repeat_range(source, b as i64).map(|t| t as u128)),
            71 | 72 => {
                let needle = text_views::text(b);
                let selected = if op == 71 {
                    source.strip_prefix(needle)
                } else {
                    source.strip_suffix(needle)
                };
                fallible(strings::try_new(selected.unwrap_or(source)).map(|t| t as u128))
            }
            76 => source.is_ascii() as u128,
            77 => (!source.is_empty() && source.chars().all(char::is_whitespace)) as u128,
            78 => crate::numbers::parse(source, (*descriptor).kind),
            107 => fallible(try_text_pieces(
                crate::text_lines::Lines::new(source, b != 0),
                (*descriptor).variants[0].fields[0],
            )),
            129 => fallible(try_text_pieces(
                source.split_whitespace(),
                (*descriptor).variants[0].fields[0],
            )),
            130 => {
                let mut characters = source.chars();
                match (characters.next(), characters.next()) {
                    (Some(character), None) => wrap(character as u128, 0),
                    _ => wrap(wrap(0, 0), 1),
                }
            }
            133 | 134 | 138 => {
                (!source.is_empty()
                    && source.bytes().all(|byte| match op {
                        133 => byte.is_ascii_digit(),
                        134 => byte.is_ascii_alphabetic(),
                        _ => byte.is_ascii_alphanumeric(),
                    })) as u128
            }
            135 | 136 => fallible(strings::try_case_range(source, op == 136).map(|t| t as u128)),
            TEXT_SLICE => {
                let positions = super::slice_positions(
                    source.chars().count(),
                    b as i64,
                    c as i64,
                    d as i64,
                    e as i64,
                );
                fallible(strings::try_stepped_slice_range(source, positions).map(|t| t as u128))
            }
            _ => crate::fail("invalid text range builtin"),
        }
    }
}
