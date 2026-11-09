//! Explicit trusted native object layouts, independent of Plenty class layout.
use super::*;
use crate::foreign::{ExportObject, Intrinsic, ObjectRelocation, Target};

impl Parser {
    fn object_integer(&mut self) -> Result<u64> {
        let at = self.take();
        if let Kind::Number(value) = &at.kind
            && let Ok(value) = value.parse::<u64>()
        {
            return Ok(value);
        }
        Err(at.error("expected an unsuffixed nonnegative decimal integer"))
    }

    pub(super) fn export_object(&mut self, symbol: String, at: &Token) -> Result<ExportObject> {
        foreign::check_symbol(&symbol, at)?;
        if !matches!(
            crate::native_target(),
            "x86_64-unknown-linux-gnu" | "aarch64-unknown-linux-gnu"
        ) {
            return Err(at.error("C object exports currently require a 64-bit GNU/Linux target"));
        }
        self.expect("(")?;
        self.expect("size")?;
        self.expect("=")?;
        let size = self.object_integer()?;
        self.expect(",")?;
        self.expect("align")?;
        self.expect("=")?;
        let align = self.object_integer()?;
        self.expect(")")?;
        if !(1..=1048576).contains(&size)
            || !align.is_power_of_two()
            || align > 4096
            || !size.is_multiple_of(align)
        {
            return Err(at.error("C object size must be 1..1048576 and a multiple of its power-of-two alignment (1..4096)"));
        }
        self.expect(":")?;
        self.kind(Kind::Newline, "a newline after `:`")?;
        self.kind(Kind::Indent, "an indented object initializer")?;
        let mut object = ExportObject {
            symbol,
            align,
            bytes: vec![0; size as usize],
            relocations: vec![],
        };
        let mut occupied = vec![false; size as usize];
        let mut count = 0;
        while !matches!(self.peek().kind, Kind::Dedent | Kind::Eof) {
            let field_at = self.peek().clone();
            count += 1;
            if count > 4096 {
                return Err(field_at.error("C objects support at most 4096 initializer fields"));
            }
            let offset = self.object_integer()?;
            self.expect(":")?;
            let kind = self.name()?;
            self.expect("=")?;
            let (bytes, natural_align) = match kind.as_str() {
                "u8" | "u16" | "u32" | "u64" => {
                    let width = match kind.as_str() {
                        "u8" => 1,
                        "u16" => 2,
                        "u32" => 4,
                        _ => 8,
                    };
                    let value = self.object_integer()?;
                    if width < 8 && value >= (1u64 << (width * 8)) {
                        return Err(field_at.error("C object integer initializer is out of range"));
                    }
                    (value.to_le_bytes()[..width].to_vec(), width as u64)
                }
                "bytes" => {
                    let value = self.take();
                    let Kind::Bytes(bytes) = value.kind else {
                        return Err(value.error("expected a byte string initializer"));
                    };
                    (bytes.to_vec(), 1)
                }
                "callback" | "object" => {
                    let value = self.take();
                    let Kind::Text(ref symbol) = value.kind else {
                        return Err(value.error("expected a C symbol string"));
                    };
                    foreign::check_symbol(symbol, &value)?;
                    // Extent checking below guarantees the offset fits u32.
                    if offset > u32::MAX as u64 {
                        return Err(field_at.error("C object field is outside the declared object"));
                    }
                    object.relocations.push(ObjectRelocation {
                        offset: offset as u32,
                        symbol: symbol.to_string(),
                        function: kind == "callback",
                    });
                    (vec![0; 8], 8)
                }
                _ => {
                    return Err(field_at.error(
                        "expected u8, u16, u32, u64, bytes, callback or object initializer",
                    ));
                }
            };
            let end = offset
                .checked_add(bytes.len() as u64)
                .filter(|end| *end <= size)
                .ok_or_else(|| field_at.error("C object field is outside the declared object"))?;
            if bytes.is_empty() || align < natural_align || !offset.is_multiple_of(natural_align) {
                return Err(
                    field_at.error("C object fields must be nonempty and naturally aligned")
                );
            }
            let range = offset as usize..end as usize;
            if occupied[range.clone()].iter().any(|used| *used) {
                return Err(field_at.error("C object initializer fields overlap"));
            }
            occupied[range.clone()].fill(true);
            object.bytes[range].copy_from_slice(&bytes);
            self.kind(Kind::Newline, "the end of an object initializer")?;
        }
        self.kind(Kind::Dedent, "the end of an object initializer block")?;
        Ok(object)
    }
}

pub(super) fn check(
    boundary: &[&Function],
    exports: &[crate::exports::Export],
    imports: &HashSet<String>,
) -> Result<()> {
    let mut objects = std::collections::BTreeMap::new();
    for f in boundary {
        if let Some(declaration) = &f.foreign
            && let Target::Intrinsic(intrinsic) = &declaration.target
            && let Intrinsic::Object(object) = intrinsic.as_ref()
            && (imports.contains(&object.symbol)
                || exports.iter().any(|e| e.symbol == object.symbol)
                || objects
                    .insert(object.symbol.clone(), (object, &f.at))
                    .is_some())
        {
            return Err(f.at.error(format!(
                "duplicate or imported C object symbol `{}`",
                object.symbol
            )));
        }
    }
    for (object, at) in objects.values() {
        for relocation in &object.relocations {
            let exists = if relocation.function {
                exports.iter().any(|e| e.symbol == relocation.symbol)
            } else {
                objects.contains_key(&relocation.symbol)
            };
            if !exists {
                return Err(at.error(format!(
                    "C object relocation `{}` must name an {} in this library",
                    relocation.symbol,
                    if relocation.function {
                        "exported function"
                    } else {
                        "exported object"
                    }
                )));
            }
        }
    }
    Ok(())
}
