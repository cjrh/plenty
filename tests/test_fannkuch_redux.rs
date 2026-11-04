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
