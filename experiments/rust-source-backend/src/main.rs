use plenty_rust_source_spike::{overflowing_trace, panicking_trace, semantic_trace};

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("semantics") => semantic_trace(),
        Some("overflow") => overflowing_trace(),
        Some("panic") => panicking_trace(),
        _ => panic!("expected semantics, overflow or panic"),
    }
}
