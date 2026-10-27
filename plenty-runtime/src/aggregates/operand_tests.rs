//! Dispatcher frames deliberately leave unused scratch words uninitialized.
//! Run these under Miri: ordinary native execution cannot reliably detect an
//! unused load that optimization erases.
use super::{byte_storage::Storage, operation, read_operands};
use crate::memory::Header;
use std::{mem::MaybeUninit, ptr};

unsafe extern "C" fn unused_destroy(_: *mut Header) {
    unreachable!("these test buffers are stack-owned")
}

fn buffer(bytes: &[u8]) -> Storage {
    Storage {
        header: Header::new(unused_destroy),
        bytes: bytes.to_vec(),
    }
}

/// Models codegen: three initialized fixed operands, an uninitialized result,
/// and only the supplied scratch operands. No reference covers uninit words.
unsafe fn invoke(op: i64, operands: &[u128]) -> u128 {
    assert!(operands.len() <= 5);
    let mut frame = [MaybeUninit::<u128>::uninit(); 6];
    for (index, slot) in frame[..3].iter_mut().enumerate() {
        slot.write(operands.get(index).copied().unwrap_or(0));
    }
    for (index, value) in operands.iter().enumerate().skip(3) {
        frame[index + 1].write(*value);
    }
    // SAFETY: fixed inputs and each actual operand are initialized. The
    // dispatcher must write the result and must not read any absent operand.
    unsafe {
        operation(op, frame.as_mut_ptr().cast(), ptr::null(), ptr::null());
        frame[3].assume_init()
    }
}

#[test]
fn byte_length_does_not_read_absent_scratch_words() {
    let mut bytes = buffer(b"abc");
    let owner = ptr::addr_of_mut!(bytes) as u128;
    // SAFETY: live buffer, declared operands, and no output reference aliases.
    assert_eq!(unsafe { invoke(171, &[owner]) }, 3);
}

#[test]
fn byte_fill_reads_the_fourth_operand_but_not_the_absent_fifth() {
    let mut bytes = buffer(b"abcd");
    let owner = ptr::addr_of_mut!(bytes) as u128;
    // SAFETY: valid range and exclusively owned buffer.
    assert_eq!(unsafe { invoke(237, &[owner, 1, 3, b'x' as u128]) }, 0);
    assert_eq!(bytes.bytes, b"axxd");
}

#[test]
fn byte_overwrite_preserves_both_scratch_operands() {
    let mut bytes = buffer(b"abcd");
    let mut source = buffer(b"wxyz");
    let owner = ptr::addr_of_mut!(bytes) as u128;
    let source = ptr::addr_of_mut!(source) as u128;
    // SAFETY: separate live buffers and valid source/destination ranges.
    assert_eq!(unsafe { invoke(179, &[owner, 1, source, 2, 4]) }, 0);
    assert_eq!(bytes.bytes, b"ayzd");
}

#[test]
fn other_dispatch_families_leave_absent_scratch_words_unread() {
    // Crypto, compression, network, selectors, SQLite, TLS and columns.
    for opcode in [
        300, 307, 316, 320, 322, 323, 324, 410, 418, 420, 440, 441, 470, 482, 493, 521, 528,
    ] {
        let mut frame = [MaybeUninit::<u128>::uninit(); 6];
        for (index, value) in [11, 22, 33].into_iter().enumerate() {
            frame[index].write(value);
        }
        // SAFETY: these opcodes have no scratch inputs, and the fixed words
        // are initialized. The result and both spare words remain uninit.
        let operands = unsafe { read_operands::<5>(opcode, frame.as_ptr().cast()) };
        assert_eq!(operands, [11, 22, 33, 0, 0], "opcode {opcode}");
    }
}

#[test]
fn scratch_words_preserve_all_128_bits() {
    let high = 1u128 << 120;
    let mut frame = [MaybeUninit::<u128>::uninit(); 6];
    for (index, value) in [11, 22, 33].into_iter().enumerate() {
        frame[index].write(value);
    }
    frame[4].write(high + 44);
    // SAFETY: column assignment uses one scratch operand, initialized above.
    let four = unsafe { read_operands::<4>(527, frame.as_ptr().cast()) };
    assert_eq!(four, [11, 22, 33, high + 44]);
    frame[5].write(high + 55);
    // SAFETY: datagram send uses two scratch operands, both initialized.
    let five = unsafe { read_operands::<5>(416, frame.as_ptr().cast()) };
    assert_eq!(five, [11, 22, 33, high + 44, high + 55]);
}
