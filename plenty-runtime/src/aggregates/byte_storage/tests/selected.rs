use super::*;
use crate::allocator::{Block, Provider};
use std::alloc::Layout;
use std::sync::{Arc, Mutex};

#[derive(Default, Debug)]
struct Counts {
    calls: usize,
    live: usize,
    destroyed: bool,
    refuse_call: Option<usize>,
}
struct Policy(Arc<Mutex<Counts>>);
impl Provider for Policy {
    fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
        let mut counts = self.0.lock().unwrap();
        counts.calls += 1;
        if counts.refuse_call == Some(counts.calls) {
            // A distinctive error that cannot be produced by small system allocations.
            return Err(AllocError::CapacityOverflow);
        }
        let block = Allocator::system().allocate(layout)?;
        counts.live += 1;
        Ok(block)
    }
    fn release(&mut self, block: Block) {
        assert!(block.bytes().iter().all(|byte| *byte == 0));
        self.0.lock().unwrap().live -= 1;
    }
}
impl Drop for Policy {
    fn drop(&mut self) {
        let mut counts = self.0.lock().unwrap();
        assert_eq!(counts.live, 0);
        counts.destroyed = true;
    }
}
fn provider() -> (Allocator, Arc<Mutex<Counts>>) {
    let counts = Arc::new(Mutex::new(Counts::default()));
    let allocator = Allocator::new(&Allocator::system(), Policy(counts.clone()))
        .unwrap_or_else(|failure| panic!("{:?}", failure.error));
    (allocator, counts)
}

#[test]
fn reversed_slice_owns_exact_destination_and_grows_after_both_sources_drop() {
    let (input, input_counts) = provider();
    let (output, output_counts) = provider();
    let src = Owner(slice(b"a\0b\xffc", 0, 0, 1, 0, Some(&input)).unwrap());
    drop(input);
    let dst = Owner(slice(bytes(&src), 0, 0, -2, 0, Some(&output)).unwrap());
    drop(src);
    drop(output);
    assert!(input_counts.lock().unwrap().destroyed);
    assert_eq!(output_counts.lock().unwrap().calls, 2);
    assert_eq!(bytes(&dst), b"cba");
    // SAFETY: all operations borrow this still-live output; no view survives growth.
    unsafe {
        assert_eq!(run(172, [dst.0, 0, 0, 0, 0]), 3);
        assert_eq!(plenty_bytes_push(dst.0 as *mut u8, b'!' as u64), 0);
        let owner = dst.0 as *mut u8;
        let view = plenty_byte_view_mut(owner);
        assert_eq!(view.1, 4);
        let tail = plenty_byte_view(owner);
        assert_eq!(tail.1, 4);
        (view.0 as *mut u8).add(1).write(b'X');
        assert_eq!((tail.0 as *const u8).add(2).read(), b'a');
        plenty_byte_view_rows_mut(owner).add(3).write(b'?');
        assert_eq!(plenty_byte_view_rows(owner).add(3).read(), b'?');
        assert_eq!(bytes(&dst), b"cXa?");
    }
    assert_eq!(output_counts.lock().unwrap().calls, 3);
    assert_eq!(output_counts.lock().unwrap().live, 2);
    drop(dst);
    assert!(output_counts.lock().unwrap().destroyed);
}

#[test]
fn ordinary_byte_brackets_do_not_inherit_selected_source_provider() {
    let (allocator, counts) = provider();
    let src = Owner(slice(b"a\0b\xffc", 0, 0, 1, 0, Some(&allocator)).unwrap());
    let before = counts.lock().unwrap().calls;
    // SAFETY: BYTES_SLICE borrows the live source for this call (`src[1::2]`).
    let sliced = unsafe { run(crate::collection_opcodes::BYTES_SLICE, [src.0, 1, 0, 2, 1]) };
    assert_eq!(sliced >> 64, 0, "expected Ok(Bytes)");
    let dst = Owner(sliced);
    assert_eq!(counts.lock().unwrap().calls, before);
    drop(src);
    drop(allocator);
    assert!(counts.lock().unwrap().destroyed);

    // SAFETY: dst is the sole owner; no view into it survives the forced growth.
    unsafe {
        let capacity = run(172, [dst.0, 0, 0, 0, 0]);
        assert_eq!(run(175, [dst.0, capacity + 1, 0, 0, 0]), 0);
        assert_eq!(plenty_bytes_push(dst.0 as *mut u8, u64::from(b'!')), 0);
    }
    assert_eq!(bytes(&dst), b"\0\xff!");
}

#[test]
fn empty_slice_retains_provider_and_refused_growth_preserves_address_and_contents() {
    let (allocator, counts) = provider();
    let owner = Owner(slice(b"a", i64::MAX, i64::MIN, 1, 3, Some(&allocator)).unwrap());
    drop(allocator);
    assert_eq!(counts.lock().unwrap().live, 1);
    // SAFETY: owner remains live throughout; only current rows are read.
    unsafe {
        assert_eq!(run(172, [owner.0, 0, 0, 0, 0]), 0);
        assert_eq!(run(175, [owner.0, 3, 0, 0, 0]), 0);
        assert_eq!(run(178, [owner.0, view_operand(b"xyz"), 0, 0, 0]), 0);
        let address = plenty_byte_view_rows(owner.0 as *const u8);
        counts.lock().unwrap().refuse_call = Some(3);
        let failure = result(Err(AllocError::CapacityOverflow));
        assert_eq!(
            plenty_bytes_push(owner.0 as *mut u8, 77),
            (failure >> 64) as u64
        );
        assert_eq!(address, plenty_byte_view_rows(owner.0 as *const u8));
        assert_eq!(run(172, [owner.0, 0, 0, 0, 0]), 3);
        assert_eq!(bytes(&owner), b"xyz");
        assert_eq!(run(175, [owner.0, i64::MAX as u128, 0, 0, 0]), failure);
        assert_eq!(counts.lock().unwrap().calls, 3);
        assert_eq!(run(176, [owner.0, 4, 99, 0, 0]), 0);
        assert_eq!(bytes(&owner), b"xyzc");
    }
    drop(owner);
    assert!(counts.lock().unwrap().destroyed);
}

#[test]
fn each_create_refusal_releases_provisional_resources_without_touching_source() {
    for refused in 1..=2 {
        let (allocator, counts) = provider();
        counts.lock().unwrap().refuse_call = Some(refused);
        let source = Owner(from_slice(b"input").unwrap());
        assert_eq!(
            slice(bytes(&source), 0, 0, -1, 0, Some(&allocator)),
            Err(AllocError::CapacityOverflow)
        );
        assert_eq!(bytes(&source), b"input");
        assert_eq!(counts.lock().unwrap().calls, refused);
        assert_eq!(counts.lock().unwrap().live, 0);
        assert_eq!(
            create_in(&allocator, usize::MAX),
            Err(AllocError::CapacityOverflow)
        );
        assert_eq!(counts.lock().unwrap().calls, refused);
        drop(allocator);
        assert!(counts.lock().unwrap().destroyed);
    }
}

#[test]
fn overlapping_subviews_survive_selected_append_relocation_and_overwrite() {
    let (allocator, counts) = provider();
    let owner = Owner(slice(b"abcd", 0, 0, 1, 0, Some(&allocator)).unwrap());
    drop(allocator);
    // SAFETY: these ABI sources select initialized rows. No Rust reference is
    // kept across mutation; append must relocate before copying the input.
    unsafe {
        let input = view_operand(&bytes(&owner)[1..3]);
        assert_eq!(run(178, [owner.0, input, 0, 0, 0]), 0);
        assert_eq!(bytes(&owner), b"abcdbc");
        let input = view_operand(&bytes(&owner)[..4]);
        assert_eq!(run(179, [owner.0, 2, input, 0, 0]), 0);
        assert_eq!(bytes(&owner), b"ababcd");
    }
    drop(owner);
    assert!(counts.lock().unwrap().destroyed);
}

#[test]
fn ordinary_crypto_prefix_writes_digest_to_selected_packed_rows() {
    use crate::aggregates::crypto;
    let out = Owner(create_in(&Allocator::system(), 22).unwrap() as u128);
    unsafe {
        let state = Owner(crypto::call(300, [5, 0, 0, 0])); // SHA-1
        assert!(bytes(&state).is_empty());
        assert_eq!(run(176, [out.0, 22, 0xff, 0, 0]), 0);
        assert_eq!(crypto::call(304, [state.0, out.0, 1, 0]), 1);
        assert_eq!(bytes(&out), b"\xff\xda\x39\xa3\xee\x5e\x6b\x4b\x0d\x32\x55\xbf\xef\x95\x60\x18\x90\xaf\xd8\x07\x09\xff");
    }
}
