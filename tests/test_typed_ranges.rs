mod support;

#[test]
fn typed_ranges_infer_literals_and_contextual_comprehensions() {
    let out = support::run(
        r#"
def squares() -> list[u8]:
    [n * n for n in range(8) if n % 2 == 0].unwrap()
x: u8 = 2
y: f32 = 3.5
print(x + 1).unwrap()
print(1 + x).unwrap()
print(y * 2).unwrap()
print([n * n for n in range[u8](8) if n % 2 == 0].unwrap()).unwrap()
values: list[u16] = [n + 1 for n in range(3)].unwrap()
print(values).unwrap()
print(squares()).unwrap()
print(list(range[u8](5, 0, -2)).unwrap()).unwrap()
print(list(range[u64](18446744073709551612, 18446744073709551615)).unwrap()).unwrap()
print(list(range[i8](-128, -125)).unwrap()).unwrap()
print(range[u64](18446744073709551612, 18446744073709551615)[1]).unwrap()
print(18446744073709551614u64 in range[u64](18446744073709551612, 18446744073709551615)).unwrap()
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n3\n7.0\n[0, 4, 16, 36]\n[1, 2, 3]\n[0, 4, 16, 36]\n[5, 3, 1]\n[18446744073709551612, 18446744073709551613, 18446744073709551614]\n[-128, -127, -126]\n18446744073709551613\nTrue\n");
}

#[test]
fn numeric_context_never_silently_narrows_typed_values() {
    for source in [
        "x: u8 = 256",
        "x: u8 = -1",
        "x: u8 = 2i64",
        "x = 2\ny: u8 = x",
        "x: u8 = 2\ny: u16 = 3\nprint(x + y).unwrap()",
        "print(range[f32](3)).unwrap()",
        "print(range[u8](256)).unwrap()",
        "x: list[u8] = [n for n in range[i64](3)]",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}
