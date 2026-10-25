//! One allocation containing a header and dense native-width scalar columns.
//! Capacity growth allocates/copies before publishing the new owner pointer.
use super::{Type, wrap};
use crate::memory::{self, AllocError, Header};

#[repr(C)]
struct Columns {
    header: Header,
    len: usize,
    capacity: usize,
    bytes: usize,
}

const _: () = {
    assert!(std::mem::offset_of!(Columns, len) == crate::clist_layout::LEN as usize);
    assert!(std::mem::offset_of!(Columns, capacity) == crate::clist_layout::CAPACITY as usize);
    assert!(size_of::<Columns>() == crate::clist_layout::DATA as usize);
};

#[unsafe(no_mangle)]
extern "C" fn plenty_trap_clist_index(site: *const crate::Site) -> ! {
    crate::fail_at(site, format_args!("clist index out of bounds"))
}

fn width(kind: u8) -> usize {
    match kind {
        b'1' | b'5' | b'b' => 1,
        b'2' | b'6' => 2,
        b'3' | b'7' | b'f' => 4,
        b'4' | b'8' | b'd' => 8,
        _ => unreachable!("clist scalar schema checked by compiler"),
    }
}
fn row_bytes(ty: &Type) -> usize {
    ty.key()
        .variants
        .iter()
        .map(|f| width(f.fields[0].kind))
        .sum()
}
unsafe extern "C" fn destroy(header: *mut Header) {
    // SAFETY: final release owns this exact flexible allocation.
    unsafe {
        let owner = header.cast::<Columns>();
        memory::free::<Columns, u8>(owner, (*owner).bytes);
    }
}
unsafe fn column(owner: *mut Columns, ty: &Type, field: usize) -> *mut u8 {
    // Capacity is rounded to a multiple of eight so every column base is
    // naturally aligned even when earlier columns contain bytes.
    unsafe {
        let prefix: usize = ty.key().variants[..field]
            .iter()
            .map(|f| width(f.fields[0].kind))
            .sum();
        owner.add(1).cast::<u8>().add(prefix * (*owner).capacity)
    }
}
unsafe fn reserve(slot: *mut u128, ty: &Type, additional: i64) -> Result<(), AllocError> {
    unsafe {
        let old = slot.read() as *mut Columns;
        let (len, cap) = if old.is_null() {
            (0, 0)
        } else {
            ((*old).len, (*old).capacity)
        };
        let extra = usize::try_from(additional).map_err(|_| AllocError::CapacityOverflow)?;
        let needed = len.checked_add(extra).ok_or(AllocError::CapacityOverflow)?;
        if needed <= cap {
            return Ok(());
        }
        let capacity = needed
            .max(cap.saturating_mul(2))
            .checked_add(7)
            .ok_or(AllocError::CapacityOverflow)?
            & !7;
        let bytes = capacity
            .checked_mul(row_bytes(ty))
            .ok_or(AllocError::CapacityOverflow)?;
        if bytes > isize::MAX as usize {
            return Err(AllocError::CapacityOverflow);
        }
        let new = memory::try_allocate::<Columns, u8>(bytes)?;
        new.write(Columns {
            header: Header::new(destroy),
            len,
            capacity,
            bytes,
        });
        if !old.is_null() {
            for (field, schema) in ty.key().variants.iter().enumerate() {
                std::ptr::copy_nonoverlapping(
                    column(old, ty, field),
                    column(new, ty, field),
                    len * width(schema.fields[0].kind),
                );
            }
        }
        // Only the payload word belongs to this clist. A reference projected
        // through an enum may share its tag word with ancestor variants.
        slot.cast::<u64>().write(new as u64);
        memory::plenty_release(old.cast());
        Ok(())
    }
}
unsafe fn position(owner: *mut Columns, index: i64) -> Option<usize> {
    let len = if owner.is_null() {
        0
    } else {
        unsafe { (*owner).len }
    };
    let index = if index < 0 { index + len as i64 } else { index };
    (index >= 0 && (index as usize) < len).then_some(index as usize)
}
unsafe fn read(owner: *mut Columns, ty: &Type, field: usize, index: usize) -> u128 {
    unsafe {
        let kind = ty.key().variants[field].fields[0].kind;
        let ptr = column(owner, ty, field).add(index * width(kind));
        match kind {
            b'1' => ptr.cast::<i8>().read() as i64 as u64 as u128,
            b'2' => ptr.cast::<i16>().read() as i64 as u64 as u128,
            b'3' => ptr.cast::<i32>().read() as i64 as u64 as u128,
            b'4' | b'8' | b'd' => ptr.cast::<u64>().read() as u128,
            b'5' | b'b' => ptr.read() as u128,
            b'6' => ptr.cast::<u16>().read() as u128,
            b'7' => ptr.cast::<u32>().read() as u128,
            b'f' => ptr.cast::<u32>().read() as u128,
            _ => unreachable!(),
        }
    }
}
unsafe fn write(owner: *mut Columns, ty: &Type, field: usize, index: usize, value: u128) {
    unsafe {
        let kind = ty.key().variants[field].fields[0].kind;
        let ptr = column(owner, ty, field).add(index * width(kind));
        match kind {
            b'1' | b'5' | b'b' => ptr.write(value as u8),
            b'2' | b'6' => ptr.cast::<u16>().write(value as u16),
            b'3' | b'7' => ptr.cast::<u32>().write(value as u32),
            b'4' | b'8' | b'd' => ptr.cast::<u64>().write(value as u64),
            b'f' => ptr.cast::<u32>().write(value as u32),
            _ => unreachable!(),
        }
    }
}

/// Operands use checked Plenty slot references; metadata is the clist type.
pub(super) unsafe fn call(op: i64, args: [u128; 4], ty: &Type, site: *const crate::Site) -> u128 {
    unsafe {
        let [a, b, c, d] = args;
        if op == 529 {
            let mut owner = 0;
            return match reserve(&mut owner, ty, a as i64) {
                Ok(()) => wrap(owner, 0),
                Err(error) => wrap((error as u128) << 64, 1),
            };
        }
        let slot = a as *mut u128;
        let owner = slot.read() as *mut Columns;
        match op {
            521 => match reserve(slot, ty, b as i64) {
                Ok(()) => wrap(0, 0),
                Err(error) => wrap((error as u128) << 64, 1),
            },
            522 => {
                if let Err(error) = reserve(slot, ty, 1) {
                    return wrap(wrap(b, error as u64), 1);
                }
                let owner = slot.read() as *mut Columns;
                let row = b as *const u128;
                for field in 0..ty.key().variants.len() {
                    write(owner, ty, field, (*owner).len, row.add(field).read());
                }
                (*owner).len += 1;
                wrap(0, 0)
            }
            523 => {
                if owner.is_null() {
                    0
                } else {
                    (*owner).len as u128
                }
            }
            524 => {
                if owner.is_null() {
                    0
                } else {
                    (*owner).capacity as u128
                }
            }
            525 | 530 => {
                let Some(index) = position(owner, b as i64) else {
                    return wrap(0, 0);
                };
                let row = c as *mut u128;
                for field in 0..ty.key().variants.len() {
                    row.add(field).write(read(owner, ty, field, index));
                }
                if op == 530 {
                    let following = (*owner).len - index - 1;
                    for (field, schema) in ty.key().variants.iter().enumerate() {
                        let width = width(schema.fields[0].kind);
                        let destination = column(owner, ty, field).add(index * width);
                        // Source and destination overlap. All live columns
                        // shift identically; stale tail bytes contain only
                        // primitives and are outside the new live length.
                        std::ptr::copy(destination.add(width), destination, following * width);
                    }
                    (*owner).len -= 1;
                }
                wrap(c, 1)
            }
            531 => {
                if !owner.is_null() {
                    // Primitive rows have no resources or destructors. Keep
                    // the allocation and let later appends overwrite slots.
                    (*owner).len = 0;
                }
                0
            }
            526 | 527 => {
                let index =
                    position(owner, b as i64).unwrap_or_else(|| plenty_trap_clist_index(site));
                if op == 526 {
                    read(owner, ty, c as usize, index)
                } else {
                    write(owner, ty, d as usize, index, c);
                    0
                }
            }
            528 => {
                if owner.is_null() {
                    0
                } else {
                    column(owner, ty, b as usize) as u128
                }
            }
            _ => unreachable!("clist opcode"),
        }
    }
}

pub(super) unsafe fn equal(a: u128, b: u128, ty: &Type) -> bool {
    unsafe {
        let a = a as *mut Columns;
        let b = b as *mut Columns;
        let len = |p: *mut Columns| if p.is_null() { 0 } else { (*p).len };
        len(a) == len(b)
            && (0..len(a)).all(|i| {
                ty.key().variants.iter().enumerate().all(|(f, schema)| {
                    super::equal(read(a, ty, f, i), read(b, ty, f, i), schema.fields[0])
                })
            })
    }
}

pub(super) unsafe fn len(value: u128) -> usize {
    if value == 0 {
        0
    } else {
        unsafe { (*(value as *const Columns)).len }
    }
}
