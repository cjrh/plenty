//! Copy-only hash bookkeeping. Keep the Vec-sized legacy descriptor; a selected
//! allocation holds its own block handle and the array in one provider request.
use crate::allocator::{Allocator, Block};
use crate::memory::{self, AllocError};
use std::alloc::Layout;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

#[repr(C)]
struct Array<T> {
    block: Block,
    capacity: usize,
    data: [T; 0],
}

struct Selected<T> {
    array: Option<NonNull<Array<T>>>,
    len: usize,
}
impl<T> Selected<T> {
    fn capacity(&self) -> usize {
        self.array.map_or(0, |p| unsafe { p.as_ref().capacity })
    }
    fn data(&self) -> *mut T {
        self.array.map_or(NonNull::<T>::dangling().as_ptr(), |p| {
            // SAFETY: array_layout reserves the trailing array at this offset.
            unsafe { std::ptr::addr_of_mut!((*p.as_ptr()).data).cast() }
        })
    }
}
impl<T> Drop for Selected<T> {
    fn drop(&mut self) {
        if let Some(p) = self.array {
            // Move the handle off its backing before release clears that backing.
            unsafe { drop(std::ptr::addr_of!((*p.as_ptr()).block).read()) };
        }
    }
}
// SAFETY: the affine block and its elements move together; mutation is exclusive.
unsafe impl<T: Send> Send for Selected<T> {}
unsafe impl<T: Sync> Sync for Selected<T> {}

enum Storage<T> {
    Legacy(Vec<T>),
    Selected(Selected<T>),
}
pub(crate) struct HashBuffer<T: Copy>(Storage<T>);
const _: () = assert!(size_of::<HashBuffer<u64>>() == size_of::<Vec<u64>>());

impl<T: Copy> HashBuffer<T> {
    pub const fn new() -> Self {
        Self(Storage::Legacy(Vec::new()))
    }
    pub fn selected() -> Self {
        Self(Storage::Selected(Selected {
            array: None,
            len: 0,
        }))
    }
    fn array_layout(count: usize) -> Result<Layout, AllocError> {
        // Include the owning header (and its alignment) in overflow checks.
        memory::object_layout::<Array<T>, T>(count)
    }
    pub fn check_selected_capacity(count: usize) -> Result<(), AllocError> {
        Allocator::backing_layout(Self::array_layout(count)?).map(drop)
    }
    pub fn capacity(&self) -> usize {
        match &self.0 {
            Storage::Legacy(v) => v.capacity(),
            Storage::Selected(s) => s.capacity(),
        }
    }
    pub fn try_reserve(
        &mut self,
        additional: usize,
        allocator: Option<&Allocator>,
    ) -> Result<(), AllocError> {
        match (&mut self.0, allocator) {
            (Storage::Legacy(v), None) => memory::try_reserve(v, additional),
            (Storage::Selected(s), Some(a)) => {
                let count = s
                    .len
                    .checked_add(additional)
                    .ok_or(AllocError::CapacityOverflow)?;
                if count <= s.capacity() {
                    return Ok(());
                }
                let mut block = a.allocate(Self::array_layout(count)?)?;
                // SAFETY: the layout covers the header and `count` aligned elements.
                // Only the initialized Copy prefix moves to the disjoint new array.
                unsafe {
                    let p = NonNull::new(block.as_mut_ptr().cast::<Array<T>>()).unwrap();
                    p.write(Array {
                        block,
                        capacity: count,
                        data: [],
                    });
                    let next = Selected {
                        array: Some(p),
                        len: s.len,
                    };
                    next.data().copy_from_nonoverlapping(s.data(), s.len);
                    *s = next;
                }
                Ok(())
            }
            _ => unreachable!("hash buffer and owner provenance must agree"),
        }
    }
    // Preserve the initialized bytes for Order's borrowed drain.
    pub fn as_ptr(&self) -> *const T {
        match &self.0 {
            Storage::Legacy(v) => v.as_ptr(),
            Storage::Selected(s) => s.data(),
        }
    }
    pub fn clear(&mut self) {
        match &mut self.0 {
            Storage::Legacy(v) => v.clear(),
            Storage::Selected(s) => s.len = 0,
        }
    }
    pub fn resize_reserved(&mut self, len: usize, value: T) {
        assert!(len <= self.capacity());
        match &mut self.0 {
            Storage::Legacy(v) => v.resize(len, value),
            Storage::Selected(s) => {
                for i in s.len..len {
                    unsafe { s.data().add(i).write(value) };
                }
                s.len = len;
            }
        }
    }
}
impl<T: Copy> Deref for HashBuffer<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        match &self.0 {
            Storage::Legacy(v) => v,
            Storage::Selected(s) => unsafe { std::slice::from_raw_parts(s.data(), s.len) },
        }
    }
}
impl<T: Copy> DerefMut for HashBuffer<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        match &mut self.0 {
            Storage::Legacy(v) => v,
            Storage::Selected(s) => unsafe { std::slice::from_raw_parts_mut(s.data(), s.len) },
        }
    }
}
