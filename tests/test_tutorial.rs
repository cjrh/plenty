//! Execute the learner's guide: `plenty` fences require `output` fences;
//! `plenty-error` fences require an `error` diagnostic substring.
use std::path::PathBuf;
use std::process::Command;

struct Fence {
    language: String,
    body: String,
    line: usize,
}

fn fences(markdown: &str) -> Vec<Fence> {
    let mut fences = Vec::new();
    let mut open: Option<Fence> = None;
    for (line, text) in markdown.lines().enumerate() {
        if let Some(language) = text.strip_prefix("```") {
            if let Some(fence) = open.take() {
                assert!(
                    language.is_empty(),
                    "TUTORIAL.md:{}: nested fence",
                    line + 1
                );
                fences.push(fence);
            } else {
                open = Some(Fence {
                    language: language.into(),
                    body: String::new(),
                    line: line + 1,
                });
            }
        } else if let Some(fence) = open.as_mut() {
            fence.body.push_str(text);
            fence.body.push('\n');
        }
    }
    assert!(open.is_none(), "TUTORIAL.md: unterminated fence");
    fences
}

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn every_tutorial_program_and_diagnostic_matches_the_language() {
    let fences = fences(include_str!("../TUTORIAL.md"));
    let workspace =
        Workspace(std::env::temp_dir().join(format!("plenty-tutorial-{}", std::process::id())));
    std::fs::create_dir(&workspace.0).unwrap();
    let source_path = workspace.0.join("lesson.plenty");
    let binary = env!("CARGO_BIN_EXE_plenty");
    let mut examples = 0;
    let mut index = 0;
    while index < fences.len() {
        let source = &fences[index];
        if !matches!(source.language.as_str(), "plenty" | "plenty-error") {
            assert!(
                !source.language.starts_with("plenty"),
                "unknown example kind at line {}",
                source.line
            );
            assert!(
                !matches!(source.language.as_str(), "output" | "error"),
                "orphan expectation at line {}",
                source.line
            );
            index += 1;
            continue;
        }
        let expected = fences
            .get(index + 1)
            .expect("each tutorial program needs an expectation");
        let valid = source.language == "plenty";
        assert_eq!(
            expected.language,
            if valid { "output" } else { "error" },
            "TUTORIAL.md:{}",
            source.line
        );
        std::fs::write(&source_path, &source.body).unwrap();
        let run_output = Command::new(binary).arg(&source_path).output().unwrap();
        let executable = workspace.0.join(format!("lesson-{examples}"));
        let compiled = Command::new(binary)
            .arg("--compile")
            .arg(&source_path)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        if valid {
            assert!(
                run_output.status.success(),
                "TUTORIAL.md:{} run command: {}",
                source.line,
                String::from_utf8_lossy(&run_output.stderr)
            );
            assert!(
                compiled.status.success(),
                "TUTORIAL.md:{} compiler: {}",
                source.line,
                String::from_utf8_lossy(&compiled.stderr)
            );
            let native = Command::new(&executable).output().unwrap();
            assert!(
                native.status.success(),
                "TUTORIAL.md:{} native: {}",
                source.line,
                String::from_utf8_lossy(&native.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&run_output.stdout),
                expected.body,
                "TUTORIAL.md:{} run command output",
                source.line
            );
            assert_eq!(
                native.stdout, run_output.stdout,
                "TUTORIAL.md:{} native output",
                source.line
            );
        } else {
            let message = expected.body.trim();
            assert!(!message.is_empty());
            for (backend, output) in [("run command", run_output), ("compiler", compiled)] {
                assert!(
                    !output.status.success(),
                    "TUTORIAL.md:{} {backend} accepted an invalid program",
                    source.line
                );
                assert!(output.stdout.is_empty(), "invalid program had effects");
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains(message),
                    "TUTORIAL.md:{} {backend}: expected {message:?}, got {}",
                    source.line,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            assert!(!executable.exists());
        }
        examples += 1;
        index += 2;
    }
    assert!(
        examples > 0,
        "the tutorial must teach with executable examples"
    );
}
