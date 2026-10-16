//! The compiler resolves large modules on several threads (issue #59). It must
//! still report the error a serial pass reports: the first in source order.

/// Units numbered below `units`, each declaring a field and a method; the
/// ones in `broken` name an undeclared type.
fn program(units: usize, unit: impl Fn(usize, &str) -> String, broken: &[usize]) -> String {
    let mut source: String = (0..units)
        .map(|i| {
            let ty = if broken.contains(&i) {
                format!("Missing{i}")
            } else {
                "i64".to_string()
            };
            unit(i, &ty)
        })
        .collect();
    source.push_str("def main() -> ():\n    pass\n");
    source
}

fn first_error(source: &str) -> String {
    let error = plenty::check_source(source).unwrap_err().to_string();
    // Scheduling differs between runs; every run must agree.
    for _ in 0..4 {
        assert_eq!(plenty::check_source(source).unwrap_err().to_string(), error);
    }
    error
}

#[test]
fn function_resolution_reports_the_first_error_in_source_order() {
    let unit = |i: usize, ty: &str| format!("def work{i}(x: {ty}) -> i64:\n    {i}\n\n");
    let error = first_error(&program(2000, unit, &[1900, 7, 1200]));
    assert!(error.contains("unknown type `Missing7`"), "{error}");
}

#[test]
fn class_resolution_reports_the_first_error_in_source_order() {
    let field = |i: usize, ty: &str| {
        format!("class Point{i}:\n    x: {ty}\n    def get(self) -> i64:\n        {i}\n\n")
    };
    let error = first_error(&program(500, field, &[400, 30]));
    assert!(error.contains("unknown type `Missing30`"), "{error}");
    let method = |i: usize, ty: &str| {
        format!("class Point{i}:\n    x: i64\n    def get(self, y: {ty}) -> i64:\n        {i}\n\n")
    };
    let error = first_error(&program(500, method, &[450, 60]));
    assert!(error.contains("unknown type `Missing60`"), "{error}");
}
