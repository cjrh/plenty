//! The fannkuch-redux benchmark example must print the published checksums.
mod support;
use std::process::Command;

#[test]
fn fannkuch_redux_matches_reference_output() {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("fannkuch_redux");
    support::compile_source_to_executable(
        include_str!("../examples/fannkuch_redux.plenty"),
        &executable,
    )
    .unwrap();
    // No argument runs n = 7, the benchmark description's sample size.
    let cases: [(&[&str], &str); 4] = [
        (&[], "228\nPfannkuchen(7) = 16\n"),
        (&["1"], "0\nPfannkuchen(1) = 0\n"),
        (&["3"], "2\nPfannkuchen(3) = 2\n"),
        (&["10"], "73196\nPfannkuchen(10) = 38\n"),
    ];
    for (arguments, expected) in cases {
        let output = Command::new(&executable).args(arguments).output().unwrap();
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{arguments:?}"
        );
    }
    let output = Command::new(&executable).arg("17").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

/// `flips`'s counter stays in a register in `block`, into which it is
/// inlined (issue #771). `block`'s `?` failures stage their errors in memory,
/// so the error built from `flips`'s result does not join every exit's value
/// in one register-allocation bundle, whose splitting spilled the counter.
#[test]
fn flip_counter_stays_in_a_register() {
    let source = include_str!("../examples/fannkuch_redux.plenty");
    let line = source
        .lines()
        .position(|line| line.trim() == "count += 1")
        .expect("flips's counter increment")
        + 1;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.plenty"), source).unwrap();
    let output = Command::new(support::plenty_bin())
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args(["--dump", "asm", "--dump-filter", "block", "f.plenty"])
        .args(["-o", "out"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = std::fs::read_to_string(dir.path().join("out/asm.txt")).unwrap();
    let body = support::asm_body(&asm, "block");
    let marker = format!("# line {line}");
    let mut at_line = false;
    let mut increments = 0;
    for text in body.lines().map(str::trim) {
        if text.starts_with("# line") {
            at_line = text == marker;
        } else if at_line {
            increments += usize::from(text.contains("add "));
            assert!(
                !text.contains("(%rsp)"),
                "line {line} accesses the stack: {text}\n{body}"
            );
        }
    }
    assert!(increments > 0, "no code for line {line}\n{body}");
}
