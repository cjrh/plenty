//! Private compiler ABI. An owning handle is one retained ProviderHeader pointer;
//! null is the explicit system root. No public raw-pointer constructor exists.
use super::*;
use crate::aggregates::{Type, allocator_owners};
use crate::collection_opcodes::*;

#[inline(always)]
pub(crate) unsafe fn borrow_handle(a: u128) -> ManuallyDrop<Allocator> {
    // Check before touching ownership or allocating. Separately compiled hosts
    // must also keep retained owners out of actor/arena contexts.
    if !crate::actor_heap::allocator_registration_allowed() {
        crate::fail("retained allocator use is unsupported in actor or arena contexts");
    }
    ManuallyDrop::new(Allocator(NonNull::new(a as *mut ProviderHeader)))
}

#[inline(always)]
pub(crate) unsafe fn operation(op: i64, a: u128, b: u128, ty: *const Type) -> u128 {
    let into_handle = |allocator: Allocator| {
        let pointer = allocator.0.map_or(std::ptr::null_mut(), NonNull::as_ptr);
        std::mem::forget(allocator);
        pointer as u128
    };
    // SAFETY: generated std code passes null or a live retained header. This
    // temporary only borrows that reference except for the explicit release.
    let handle = unsafe { borrow_handle(a) };
    match op {
        ALLOCATOR_SYSTEM => into_handle(Allocator::system()),
        ALLOCATOR_SHARE => into_handle(Allocator::clone(&handle)),
        ALLOCATOR_RELEASE => {
            drop(ManuallyDrop::into_inner(handle));
            0
        }
        ALLOCATOR_BOX => {
            // The std wrapper owns b until success, then commits without dropping.
            // On failure no payload bytes have been moved or destroyed.
            match unsafe { allocator_owners::box_new_in(&handle, &*ty, b) } {
                Ok(boxed) => boxed as u128,
                Err(error) => (1 + 2 * error as u128) << 64,
            }
        }
        ALLOCATOR_LIST | ALLOCATOR_DICT_SET => {
            let result = usize::try_from(b as i64)
                .map_err(|_| AllocError::CapacityOverflow)
                .and_then(|capacity| {
                    // SAFETY: the compiler supplies the static collection descriptor for this opcode.
                    let ty = unsafe { &*ty };
                    if op == ALLOCATOR_LIST {
                        allocator_owners::list_new_in(&handle, ty, capacity)
                    } else {
                        allocator_owners::dict_set_new_in(&handle, ty, capacity)
                    }
                });
            match result {
                Ok(collection) => collection as u128,
                Err(error) => (1 + 2 * error as u128) << 64,
            }
        }
        _ => unreachable!("retained allocator opcode"),
    }
}

/// `array[u64, 3]` is three packed words plus eight bytes of inline tail
/// padding. Only std.alloc owns these arrays; ordinary code cannot obtain or
/// construct the enclosing ByteBlock's private field. Copying these words is
/// a move, never a backing retain. Do not cast Rust's unspecified Block layout.
pub(super) unsafe fn read_block(storage: u128) -> ManuallyDrop<Block> {
    let words = storage as *const u64;
    unsafe {
        ManuallyDrop::new(Block {
            backing: NonNull::new(words.read() as *mut Backing).expect("live block backing"),
            data: NonNull::new(words.add(1).read() as *mut u8).expect("live block data"),
            len: words.add(2).read() as usize,
        })
    }
}

pub(super) unsafe fn write_block(storage: u128, block: Block) -> u128 {
    let block = ManuallyDrop::new(block);
    let words = storage as *mut u64;
    unsafe {
        words.write(block.backing.as_ptr() as u64);
        words.add(1).write(block.data.as_ptr() as u64);
        words.add(2).write(block.len as u64);
        words.add(3).write(0);
    }
    storage
}

#[inline(always)]
pub(crate) unsafe fn block_operation(op: i64, [a, b, c, d]: [u128; 4]) -> u128 {
    if !crate::actor_heap::allocator_registration_allowed() {
        crate::fail("retained allocator use is unsupported in actor or arena contexts");
    }
    if op == ALLOCATOR_BLOCK {
        // Validate even compiler-generated scalar inputs. Layout's public
        // factory is not a license to use unchecked native Layout creation.
        let result = if (b as i64) < 0 || (c as i64) <= 0 {
            Err(AllocError::CapacityOverflow)
        } else {
            Layout::from_size_align(b as usize, c as usize)
                .map_err(|_| AllocError::CapacityOverflow)
                .and_then(|layout| {
                    let handle =
                        ManuallyDrop::new(Allocator(NonNull::new(a as *mut ProviderHeader)));
                    handle.allocate(layout)
                })
        };
        return match result {
            Ok(block) => unsafe { write_block(d, block) },
            Err(error) => (1 + 2 * error as u128) << 64,
        };
    }
    // SAFETY: the compiler resolves a private inline array reference to its
    // payload address. Writes require an exclusive Plenty borrow.
    let mut block = unsafe { read_block(a) };
    match op {
        ALLOCATOR_BLOCK_RELEASE => {
            drop(ManuallyDrop::into_inner(block));
            unsafe { std::ptr::write_bytes(a as *mut u8, 0, 32) };
            0
        }
        ALLOCATOR_BLOCK_LEN => block.len as u128,
        ALLOCATOR_BLOCK_ALIGNMENT => block.alignment() as u128,
        ALLOCATOR_BLOCK_GET | ALLOCATOR_BLOCK_SET => {
            if (b as i64) < 0 || b as usize >= block.len {
                crate::fail(if op == ALLOCATOR_BLOCK_GET {
                    "ByteBlock.get: index is outside 0..len()"
                } else {
                    "ByteBlock.set: index is outside 0..len()"
                });
            }
            if op == ALLOCATOR_BLOCK_GET {
                block.bytes()[b as usize] as u128
            } else {
                block.bytes_mut()[b as usize] = c as u8;
                0
            }
        }
        ALLOCATOR_BLOCK_SPLIT => {
            if (b as i64) < 0 || b as usize > block.len {
                crate::fail("ByteBlock.split_off: at is outside 0..len() inclusive");
            }
            let suffix = block.split_off(b as usize);
            unsafe {
                (a as *mut u64).add(2).write(block.len as u64);
                write_block(c, suffix)
            }
        }
        _ => unreachable!("retained block opcode"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[derive(Default)]
    struct Counts {
        allocations: AtomicUsize,
        releases: AtomicUsize,
        dropped_on: Mutex<Option<thread::ThreadId>>,
    }
    struct Policy {
        counts: Arc<Counts>,
        failure: Option<AllocError>,
    }
    impl Provider for Policy {
        fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
            self.counts.allocations.fetch_add(1, Ordering::Relaxed);
            match self.failure {
                Some(error) => Err(error),
                None => Allocator::system().allocate(layout),
            }
        }
        fn release(&mut self, block: Block) {
            assert!(block.bytes().iter().all(|byte| *byte == 0));
            self.counts.releases.fetch_add(1, Ordering::Relaxed);
            drop(block);
        }
    }
    impl Drop for Policy {
        fn drop(&mut self) {
            *self.counts.dropped_on.lock().unwrap() = Some(thread::current().id());
        }
    }
    fn provider(counts: &Arc<Counts>, failure: Option<AllocError>) -> Allocator {
        Allocator::new(
            &Allocator::system(),
            Policy {
                counts: counts.clone(),
                failure,
            },
        )
        .unwrap_or_else(|_| panic!("provider setup failed"))
    }
    fn handle(allocator: &Allocator) -> u128 {
        allocator.0.map_or(0, |pointer| pointer.as_ptr() as u128)
    }
    fn storage(words: &mut [u64; 4]) -> u128 {
        words.as_mut_ptr() as u128
    }

    #[test]
    fn block_binding_validates_layout_and_preserves_provider_errors() {
        let counts = Arc::new(Counts::default());
        let allocator = provider(&counts, Some(AllocError::OutOfMemory));
        let mut output = [0; 4];
        let pointer = storage(&mut output);
        unsafe {
            for (size, alignment) in [(-1i64, 8), (0, 0), (1, -1), (1, 3), (i64::MAX, 8)] {
                assert_eq!(
                    block_operation(
                        ALLOCATOR_BLOCK,
                        [handle(&allocator), size as u128, alignment as u128, pointer]
                    ),
                    (1 + 2 * AllocError::CapacityOverflow as u128) << 64,
                );
            }
            assert_eq!(counts.allocations.load(Ordering::Relaxed), 0);
            assert_eq!(
                block_operation(ALLOCATOR_BLOCK, [handle(&allocator), 8, 8, pointer]),
                (1 + 2 * AllocError::OutOfMemory as u128) << 64,
            );
            assert_eq!(counts.allocations.load(Ordering::Relaxed), 1);
            assert_eq!(output, [0; 4]);
        }
        // No system fallback and no release for a failed request.
        assert_eq!(counts.releases.load(Ordering::Relaxed), 0);
        drop(allocator);
        assert!(counts.dropped_on.lock().unwrap().is_some());
    }

    #[test]
    fn block_binding_splits_retain_provenance_until_empty_worker_release() {
        let counts = Arc::new(Counts::default());
        let allocator = provider(&counts, None);
        let mut left = [0; 4];
        let mut right = [0; 4];
        let mut empty = [0; 4];
        unsafe {
            let left_ptr = storage(&mut left);
            let right_ptr = storage(&mut right);
            let empty_ptr = storage(&mut empty);
            assert_eq!(
                block_operation(ALLOCATOR_BLOCK, [handle(&allocator), 9, 64, left_ptr]),
                left_ptr
            );
            drop(allocator);
            assert!(counts.dropped_on.lock().unwrap().is_none());
            assert_eq!(block_operation(ALLOCATOR_BLOCK_GET, [left_ptr, 8, 0, 0]), 0);
            block_operation(ALLOCATOR_BLOCK_SPLIT, [left_ptr, 3, right_ptr, 0]);
            assert_eq!(
                block_operation(ALLOCATOR_BLOCK_ALIGNMENT, [right_ptr, 0, 0, 0]),
                1
            );
            block_operation(ALLOCATOR_BLOCK_SET, [left_ptr, 2, 17, 0]);
            block_operation(ALLOCATOR_BLOCK_SET, [right_ptr, 0, 29, 0]);
            assert_eq!(
                block_operation(ALLOCATOR_BLOCK_GET, [left_ptr, 2, 0, 0]),
                17
            );
            assert_eq!(
                block_operation(ALLOCATOR_BLOCK_GET, [right_ptr, 0, 0, 0]),
                29
            );
            block_operation(ALLOCATOR_BLOCK_SPLIT, [right_ptr, 6, empty_ptr, 0]);
            block_operation(ALLOCATOR_BLOCK_RELEASE, [left_ptr, 0, 0, 0]);
            block_operation(ALLOCATOR_BLOCK_RELEASE, [right_ptr, 0, 0, 0]);
        }
        assert_eq!(counts.allocations.load(Ordering::Relaxed), 1);
        assert_eq!(counts.releases.load(Ordering::Relaxed), 0);
        let final_thread = thread::spawn(move || {
            unsafe {
                let pointer = storage(&mut empty);
                assert_eq!(block_operation(ALLOCATOR_BLOCK_LEN, [pointer, 0, 0, 0]), 0);
                block_operation(ALLOCATOR_BLOCK_RELEASE, [pointer, 0, 0, 0]);
            }
            thread::current().id()
        })
        .join()
        .unwrap();
        assert_eq!(counts.releases.load(Ordering::Relaxed), 1);
        assert_eq!(*counts.dropped_on.lock().unwrap(), Some(final_thread));
    }
}
