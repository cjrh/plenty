#[inline(never)]
fn windows(xs: &[f64], width: usize) -> f64 {
    let mut total = 0.0;
    if width == 0 { return total; }
    for window in xs.windows(width) {
        for x in window { total += x; }
    }
    total
}
fn main() {
    let xs: Vec<f64> = (0..1024).map(|i| (i % 16) as f64).collect();
    let mut total = 0.0;
    for _ in 0..100000 {
        total += windows(std::hint::black_box(&xs), std::hint::black_box(4));
    }
    println!("{total:.1}");
}
