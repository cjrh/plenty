fn main() {
    let owner = vec![1_i64];
    drop(owner);
    println!("{}", owner.len());
}
