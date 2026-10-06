//! Execute the learner's guide: `plenty` fences require `output` fences;
//! `plenty-error` fences require an `error` diagnostic substring. Preceding
//! `plenty-file path.plenty` fences supply modules for that one example.
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
    let binary = env!("CARGO_BIN_EXE_plenty");
    let mut examples = 0;
    let mut index = 0;
    let mut modules = Vec::new();
    while index < fences.len() {
        let source = &fences[index];
        if let Some(path) = source.language.strip_prefix("plenty-file ") {
            let path = PathBuf::from(path);
            assert!(
                !path.as_os_str().is_empty()
                    && path
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)))
                    && path.extension().is_some_and(|e| e == "plenty")
                    && path != std::path::Path::new("lesson.plenty"),
                "invalid module path at TUTORIAL.md:{}",
                source.line
            );
            assert!(
                !modules.iter().any(|(p, _)| p == &path),
                "duplicate tutorial module"
            );
            modules.push((path, source.body.clone()));
            index += 1;
            continue;
        }
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
        let example_dir = workspace.0.join(format!("example-{examples}"));
        std::fs::create_dir(&example_dir).unwrap();
        for (path, body) in modules.drain(..) {
            let path = example_dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let source_path = example_dir.join("lesson.plenty");
        std::fs::write(&source_path, &source.body).unwrap();
        // Runtime examples may create files. Give each execution a separate
        // empty working directory so examples neither touch the repo nor rely
        // on files left behind by the other execution mode.
        let run_dir = example_dir.join("run");
        let native_dir = example_dir.join("native");
        std::fs::create_dir(&run_dir).unwrap();
        std::fs::create_dir(&native_dir).unwrap();
        let run_output = Command::new(binary)
            .arg(&source_path)
            .current_dir(&run_dir)
            .output()
            .unwrap();
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
            let native = Command::new(&executable)
                .current_dir(&native_dir)
                .output()
                .unwrap();
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
    assert!(
        modules.is_empty(),
        "orphan tutorial module without an example"
    );
}
