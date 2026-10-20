//! One very large function is compiled by Cranelift's single-pass register
//! allocator (issue #56; `SINGLE_PASS_BRANCH_ARGUMENTS` in
//! `src/codegen/output.rs`). Each function below passes about 3,000 values to
//! block parameters, well over that limit, and must still compute what the
//! same source computes at ordinary size.

mod support;
use support::run_stdout as stdout;

#[test]
fn conditional_assignments_keep_their_values() {
    const STEPS: i64 = 1500;
    let mut source = String::from("def big(x: i64) -> i64:\n    mut y = x\n");
    for i in 0..STEPS {
        source.push_str(&format!(
            "    y = y + {i}\n    if y > {}:\n        y = y - 3\n",
            i * 7
        ));
    }
    source.push_str(
        "    y

def main() -> Result[(), IoError]:
    print(big(0))?
    print(big(-2000000))?
    print(big(5000))?
    Ok(())
",
    );
    let expected = |x: i64| {
        (0..STEPS).fold(x, |y, i| {
            let y = y + i;
            if y > i * 7 {
                y - 3
            } else {
                y
            }
        })
    };
    // The three inputs take the conditional sometimes, never, and always.
    assert_eq!(
        stdout(&source),
        format!(
            "{}\n{}\n{}\n",
            expected(0),
            expected(-2_000_000),
            expected(5000)
        )
    );
}

#[test]
fn list_initializer_keeps_every_element() {
    const ELEMENTS: i64 = 1500;
    let elements: Vec<String> = (0..ELEMENTS).map(|i| (i * 3).to_string()).collect();
    let source = format!(
        "def big() -> Result[list[i64], AllocError]:
    xs = [{}]?
    Ok(xs)

def main() -> Result[(), AllocError | IoError]:
    xs = big()?
    mut total = 0
    for x in xs:
        total += x
    print(total)?
    Ok(())
",
        elements.join(", ")
    );
    assert_eq!(
        stdout(&source),
        format!("{}\n", (0..ELEMENTS).map(|i| i * 3).sum::<i64>())
    );
}

#[test]
fn match_table_selects_each_arm() {
    const VARIANTS: usize = 3000;
    let mut source = String::from("enum Kind:\n");
    for i in 0..VARIANTS {
        source.push_str(&format!("    K{i}\n"));
    }
    source.push_str("\ndef big(k: Kind) -> i64:\n    match k:\n");
    for i in 0..VARIANTS {
        source.push_str(&format!(
            "        case Kind.K{i}:\n            {}\n",
            i * 3 + 1
        ));
    }
    source.push_str(&format!(
        "
def main() -> Result[(), IoError]:
    print(big(Kind.K0))?
    print(big(Kind.K{}))?
    print(big(Kind.K{}))?
    Ok(())
",
        VARIANTS / 2,
        VARIANTS - 1
    ));
    assert_eq!(
        stdout(&source),
        format!(
            "1\n{}\n{}\n",
            (VARIANTS / 2) * 3 + 1,
            (VARIANTS - 1) * 3 + 1
        )
    );
}
