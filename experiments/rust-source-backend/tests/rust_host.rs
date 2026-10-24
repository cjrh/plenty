//! This integration test is a separate Rust crate consuming the candidate API.
use plenty_rust_source_spike::{Error, first, map_value, read_byte};
use std::io::Cursor;

#[test]
fn rust_host_uses_native_types_traits_and_borrows() {
    let owned_by_rust = vec![7, 8];
    assert!(std::ptr::eq(first(&owned_by_rust), &owned_by_rust[0]));
    let mut reader = Cursor::new([42]);
    assert_eq!(read_byte(&mut reader).unwrap(), 42);
    assert!(matches!(read_byte(&mut reader), Err(Error::Io(_))));
    let offset = Box::new(5);
    assert_eq!(map_value(2, move |value| value + *offset), 7);
}
