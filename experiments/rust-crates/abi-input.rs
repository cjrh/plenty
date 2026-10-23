//! Native layouts intentionally distinguish ABI queries from scalar guessing.
#[repr(C)]
pub struct Triple(pub u64, pub u64, pub u64);

pub fn probe_scalar(value: i64) -> i64 {
    value + 1
}

pub fn probe_text(text: &str) -> usize {
    text.len()
}

pub fn probe_rust_record(value: Triple) -> Triple {
    value
}

pub extern "C" fn probe_c_record(value: Triple) -> Triple {
    value
}
