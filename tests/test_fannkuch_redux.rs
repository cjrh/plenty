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
    check_counter_increment(body, line).unwrap_or_else(|error| panic!("{error}\n{body}"));
}

/// Both decoders retain the instruction mnemonic after the address. Checked
/// AArch64 addition sets flags with `adds`; x86's `add` already sets them.
fn check_counter_increment(body: &str, line: usize) -> Result<(), String> {
    let marker = format!("# line {line}");
    let mut at_line = false;
    let mut increments = 0;
    for text in body.lines().map(str::trim) {
        if text.starts_with("# line") {
            at_line = text == marker;
        } else if at_line && let Some((_, instruction)) = text.split_once(':') {
            increments += usize::from(matches!(
                instruction.split_whitespace().next(),
                Some("add" | "adds")
            ));
            // Cover the stack and frame pointer forms on both supported ISAs,
            // including indexed x86 operands and AArch64's frame-pointer alias.
            if ["(%rsp", "(%rbp", "[sp", "[x29", "[fp"]
                .iter()
                .any(|stack| instruction.contains(stack))
            {
                return Err(format!("line {line} accesses the stack: {text}"));
            }
        }
    }
    if increments == 0 {
        return Err(format!("no counter increment for line {line}"));
    }
    Ok(())
}

#[test]
fn counter_check_recognizes_both_instruction_sets() {
    for instruction in ["add $1,%r14", "add x0, x0, #0x1", "adds x0, x0, x2"] {
        let body = format!(
            "# line 46\n    998: mov x2, #0x1\n# line 47\n    99c: {instruction}\n# line 48\n    9a0: ldr x0, [sp, #0x10]"
        );
        assert_eq!(check_counter_increment(&body, 47), Ok(()));
        assert!(check_counter_increment(&body, 46).is_err());
        assert!(check_counter_increment(&body, 99).is_err());
    }
}

#[test]
fn counter_check_rejects_spills_on_both_instruction_sets() {
    for spill in [
        "mov 0x10(%rsp),%rax",
        "mov %rax,-0x8(%rbp)",
        "mov (%rsp,%rax,8),%rcx",
        "ldr x0, [sp, #0x10]",
        "stur x0, [x29, #-0x8]",
        "str x0, [fp, #-0x8]",
    ] {
        let body = format!("# line 47\n    99c: adds x0, x0, x2\n    9a0: {spill}");
        let error = check_counter_increment(&body, 47).unwrap_err();
        assert!(error.contains("accesses the stack"), "{error}");
    }
}
