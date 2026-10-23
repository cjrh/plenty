fn main() {
    let mut values = vec![10_i64, 20];
    let first = &mut values[0];
    let second = &mut values[1];
    *first += 1;
    *second += 2;
}
