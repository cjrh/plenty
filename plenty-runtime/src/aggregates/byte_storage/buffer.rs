//! Keep the ordinary `Header, Vec<u8>` footprint, including for embedded codec
//! state. Only the typed enum discriminant selects a backing representation.
use crate::allocator::{Allocator, Block};
use crate::memory::{self, AllocError, Header};
use std::alloc::Layout;
use std::mem::{ManuallyDrop, offset_of};
use std::ptr::{self, NonNull};

#[repr(C)]
pub(in crate::aggregates) struct Storage {
    header: Header,
    bytes: Buffer,
}

enum Buffer {
    Ordinary(Vec<u8>),
    // Points at a sibling in the same stationary SelectedOwner. Neither this
    // prefix nor its fields may be moved out of a live selected allocation.
    Selected(SelectedRef),
}

/// No Clone or independent ownership: the prefix lives at the head of the
/// allocation containing its one metadata record, for the same lifetime.
struct SelectedRef(NonNull<SelectedBytes>);
impl std::ops::Deref for SelectedRef {
    type Target = SelectedBytes;
    fn deref(&self) -> &SelectedBytes {
        // SAFETY: every live prefix retains its initialized sibling.
        unsafe { self.0.as_ref() }
    }
}
impl std::ops::DerefMut for SelectedRef {
    fn deref_mut(&mut self) -> &mut SelectedBytes {
        // SAFETY: exclusive prefix access includes its private metadata.
        unsafe { self.0.as_mut() }
    }
}

struct SelectedBytes {
    allocator: Allocator,
    backing: Option<Block>,
    len: usize,
}

#[repr(C)]
struct SelectedOwner {
    storage: Storage,
    bytes: SelectedBytes,
    object: ManuallyDrop<Block>,
}

const _: () = {
    assert!(size_of::<Buffer>() == size_of::<Vec<u8>>());
    assert!(align_of::<Buffer>() == align_of::<Vec<u8>>());
    assert!(offset_of!(Storage, header) == 0);
    assert!(offset_of!(Storage, bytes) == size_of::<Header>());
    assert!(size_of::<Storage>() == size_of::<Header>() + size_of::<Vec<u8>>());
    assert!(align_of::<Storage>() == align_of::<Header>());
    assert!(offset_of!(SelectedOwner, storage) == 0);
};

impl SelectedBytes {
    fn capacity(&self) -> usize {
        self.backing.as_ref().map_or(0, |block| block.bytes().len())
    }

    fn reserve(&mut self, additional: usize, amortized: bool) -> Result<(), AllocError> {
        let needed = self
            .len
            .checked_add(additional)
            .ok_or(AllocError::CapacityOverflow)?;
        memory::checked_capacity::<u8>(needed)?;
        let capacity = self.capacity();
        if needed <= capacity {
            return Ok(());
        }
        let target = if amortized {
            needed.max(capacity.saturating_mul(2)).max(8)
        } else {
            needed
        };
        let layout = Layout::array::<u8>(target).map_err(|_| AllocError::CapacityOverflow)?;
        let mut replacement = self.allocator.allocate(layout)?;
        if let Some(old) = &self.backing {
            replacement.bytes_mut()[..self.len].copy_from_slice(&old.bytes()[..self.len]);
        }
        self.backing = Some(replacement);
        Ok(())
    }
}

pub(in crate::aggregates) fn create_in(
    allocator: &Allocator,
    capacity: usize,
) -> Result<*mut Storage, AllocError> {
    // Reserve the payload before the object, as in ordinary create; on
    // refusal the provisional backing returns to the same provider.
    let mut bytes = SelectedBytes {
        allocator: allocator.clone(),
        backing: None,
        len: 0,
    };
    bytes.reserve(capacity, false)?;
    let mut object = allocator.allocate(Layout::new::<SelectedOwner>())?;
    // SAFETY: object owns this aligned extent. Form the sibling pointer only
    // at the final address, and keep the allocation until the last release.
    unsafe {
        let owner = object.as_mut_ptr().cast::<SelectedOwner>();
        let data = NonNull::new_unchecked(ptr::addr_of_mut!((*owner).bytes));
        owner.write(SelectedOwner {
            storage: Storage {
                header: Header::new(destroy_selected),
                bytes: Buffer::Selected(SelectedRef(data)),
            },
            bytes,
            object: ManuallyDrop::new(object),
        });
        Ok(owner.cast())
    }
}

unsafe extern "C" fn destroy_selected(pointer: *mut Header) {
    // SAFETY: final release. Move out the block that contains the descriptor
    // before destroying fields, then release it without accessing owner again.
    unsafe {
        let owner = pointer.cast::<SelectedOwner>();
        let block = ManuallyDrop::take(&mut (*owner).object);
        ptr::drop_in_place(owner);
        drop(block);
    }
}

impl Storage {
    pub(in crate::aggregates) fn ordinary(
        destroy: unsafe extern "C" fn(*mut Header),
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            header: Header::new(destroy),
            bytes: Buffer::Ordinary(bytes),
        }
    }

    /// Only for authenticated, ordinary codec state that lends spare capacity
    /// to the codec. Never reconstruct a Vec over a provider's block.
    pub(in crate::aggregates) fn codec_bytes_mut(&mut self) -> &mut Vec<u8> {
        let Buffer::Ordinary(bytes) = &mut self.bytes else {
            unreachable!("selected codec state")
        };
        bytes
    }

    /// A generated view holds the owner's loan and may not outlive or resize
    /// it. Writable views require an exclusive Plenty loan. Keep the legacy
    /// Vec path unchanged; selected rows retain the backing pointer's provenance.
    pub(super) fn view_parts(&self) -> (*const u8, usize) {
        match &self.bytes {
            Buffer::Ordinary(bytes) => (bytes.as_slice().as_ptr(), bytes.len()),
            Buffer::Selected(data) => (
                data.backing
                    .as_ref()
                    .map_or(std::ptr::dangling::<u8>(), Block::as_ptr),
                data.len,
            ),
        }
    }
    pub(in crate::aggregates) fn as_slice(&self) -> &[u8] {
        match &self.bytes {
            Buffer::Ordinary(bytes) => bytes,
            Buffer::Selected(data) => data
                .backing
                .as_ref()
                .map_or(&[], |b| &b.bytes()[..data.len]),
        }
    }
    pub(in crate::aggregates) fn as_mut_slice(&mut self) -> &mut [u8] {
        match &mut self.bytes {
            Buffer::Ordinary(bytes) => bytes,
            Buffer::Selected(data) => {
                let data = &mut **data;
                data.backing
                    .as_mut()
                    .map_or(&mut [], |b| &mut b.bytes_mut()[..data.len])
            }
        }
    }
    pub(in crate::aggregates) fn capacity(&self) -> usize {
        match &self.bytes {
            Buffer::Ordinary(bytes) => bytes.capacity(),
            Buffer::Selected(data) => data.capacity(),
        }
    }
    pub(super) fn reserve(&mut self, additional: usize, amortized: bool) -> Result<(), AllocError> {
        match &mut self.bytes {
            Buffer::Ordinary(bytes) if amortized => {
                memory::try_reserve_amortized(bytes, additional)
            }
            Buffer::Ordinary(bytes) => memory::try_reserve(bytes, additional),
            Buffer::Selected(data) => data.reserve(additional, amortized),
        }
    }
    // The socket historically maps any Vec::try_reserve error to OutOfMemory.
    pub(in crate::aggregates) fn reserve_address(&mut self, size: usize) -> Result<(), AllocError> {
        match &mut self.bytes {
            Buffer::Ordinary(bytes) => bytes.try_reserve(size).map_err(|_| AllocError::OutOfMemory),
            Buffer::Selected(data) => data.reserve(size, true),
        }
    }
    pub(in crate::aggregates) fn resize_reserved(&mut self, len: usize, value: u8) {
        assert!(len <= self.capacity());
        match &mut self.bytes {
            Buffer::Ordinary(bytes) => bytes.resize(len, value),
            Buffer::Selected(data) => {
                let data = &mut **data;
                if len > data.len {
                    data.backing.as_mut().unwrap().bytes_mut()[data.len..len].fill(value);
                }
                data.len = len;
            }
        }
    }
    pub(super) fn gather_reserved(&mut self, values: impl ExactSizeIterator<Item = u8>) {
        match &mut self.bytes {
            Buffer::Ordinary(bytes) => {
                debug_assert!(values.len() <= bytes.capacity() - bytes.len());
                bytes.extend(values);
            }
            Buffer::Selected(_) if values.len() == 0 => (),
            Buffer::Selected(data) => {
                let data = &mut **data;
                let end = data.len + values.len();
                let target = &mut data.backing.as_mut().unwrap().bytes_mut()[data.len..end];
                for (byte, value) in target.iter_mut().zip(values) {
                    *byte = value;
                }
                data.len = end;
            }
        }
    }
    pub(super) fn extend_reserved(&mut self, source: &[u8]) {
        match &mut self.bytes {
            Buffer::Ordinary(bytes) => {
                debug_assert!(source.len() <= bytes.capacity() - bytes.len());
                bytes.extend_from_slice(source);
            }
            Buffer::Selected(_) if source.is_empty() => (),
            Buffer::Selected(data) => {
                let data = &mut **data;
                let end = data.len + source.len();
                data.backing.as_mut().unwrap().bytes_mut()[data.len..end].copy_from_slice(source);
                data.len = end;
            }
        }
    }
}
