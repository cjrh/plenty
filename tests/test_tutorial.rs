//! Execute the learner's guide in `book/src/tutorial`: `plenty` fences require
//! `output` fences; `plenty-error` fences require an `error` diagnostic
//! substring. Preceding `plenty-file path.plenty` fences on the same page
//! supply modules for that one example.
use std::path::{Path, PathBuf};
use std::process::Command;

struct Fence {
    language: String,
    body: String,
    /// `page:line`, for failure messages.
    line: String,
}

fn book_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("book/src")
}

/// Every page that `SUMMARY.md` links, in reading order.
fn summary_pages() -> Vec<String> {
    let summary = std::fs::read_to_string(book_src().join("SUMMARY.md")).unwrap();
    summary
        .split("](")
        .skip(1)
        .filter_map(|rest| rest.split_once(')'))
        .map(|(target, _)| target.to_string())
        .filter(|target| !target.is_empty())
        .collect()
}

fn markdown_files(dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            let relative = path.strip_prefix(book_src()).unwrap();
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// mdBook silently skips pages missing from `SUMMARY.md`; this test would then
/// still run their examples, but readers could not find them.
#[test]
fn summary_lists_every_book_page() {
    let listed = summary_pages();
    let mut files = Vec::new();
    markdown_files(&book_src(), &mut files);
    let missing: Vec<_> = files
        .iter()
        .filter(|f| *f != "SUMMARY.md" && !listed.contains(f))
        .collect();
    assert!(
        missing.is_empty(),
        "pages missing from SUMMARY.md: {missing:?}"
    );
    let dangling: Vec<_> = listed.iter().filter(|l| !files.contains(l)).collect();
    assert!(
        dangling.is_empty(),
        "SUMMARY.md links missing pages: {dangling:?}"
    );
}

fn fences(page: &str, markdown: &str) -> Vec<Fence> {
    let mut fences = Vec::new();
    let mut open: Option<Fence> = None;
    for (line, text) in markdown.lines().enumerate() {
        if let Some(language) = text.strip_prefix("```") {
            if let Some(fence) = open.take() {
                assert!(language.is_empty(), "{page}:{}: nested fence", line + 1);
                fences.push(fence);
            } else {
                open = Some(Fence {
                    language: language.into(),
                    body: String::new(),
                    line: format!("{page}:{}", line + 1),
                });
            }
        } else if let Some(fence) = open.as_mut() {
            fence.body.push_str(text);
            fence.body.push('\n');
        }
    }
    assert!(open.is_none(), "{page}: unterminated fence");
    fences
}

fn page_of(location: &str) -> &str {
    location.rsplit_once(':').unwrap().0
}

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn every_tutorial_program_and_diagnostic_matches_the_language() {
    let fences: Vec<Fence> = summary_pages()
        .into_iter()
        .filter(|page| page.starts_with("tutorial/"))
        .flat_map(|page| {
            let markdown = std::fs::read_to_string(book_src().join(&page)).unwrap();
            fences(&page, &markdown)
        })
        .collect();
    let workspace =
        Workspace(std::env::temp_dir().join(format!("plenty-tutorial-{}", std::process::id())));
    std::fs::create_dir(&workspace.0).unwrap();
    let binary = env!("CARGO_BIN_EXE_plenty");
    let mut examples = 0;
    let mut index = 0;
    let mut modules = Vec::new();
    let mut failures = Vec::new();
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
                "invalid module path at {}",
                source.line
            );
            assert!(
                !modules.iter().any(|(p, _, _)| p == &path),
                "duplicate tutorial module"
            );
            modules.push((path, source.body.clone(), page_of(&source.line)));
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
            "{}",
            source.line
        );
        let example_dir = workspace.0.join(format!("example-{examples}"));
        std::fs::create_dir(&example_dir).unwrap();
        for (path, body, page) in modules.drain(..) {
            assert_eq!(
                page,
                page_of(&source.line),
                "companion module for {} is on another page",
                source.line
            );
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
        let checked = std::panic::catch_unwind(|| {
            if valid {
                assert!(
                    run_output.status.success(),
                    "{} run command: {}",
                    source.line,
                    String::from_utf8_lossy(&run_output.stderr)
                );
                assert!(
                    compiled.status.success(),
                    "{} compiler: {}",
                    source.line,
                    String::from_utf8_lossy(&compiled.stderr)
                );
                let native = Command::new(&executable)
                    .current_dir(&native_dir)
                    .output()
                    .unwrap();
                assert!(
                    native.status.success(),
                    "{} native: {}",
                    source.line,
                    String::from_utf8_lossy(&native.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&run_output.stdout),
                    expected.body,
                    "{} run command output",
                    source.line
                );
                assert_eq!(
                    native.stdout, run_output.stdout,
                    "{} native output",
                    source.line
                );
            } else {
                let message = expected.body.trim();
                assert!(!message.is_empty());
                for (backend, output) in [("run command", run_output), ("compiler", compiled)] {
                    assert!(
                        !output.status.success(),
                        "{} {backend} accepted an invalid program",
                        source.line
                    );
                    assert!(output.stdout.is_empty(), "invalid program had effects");
                    assert!(
                        String::from_utf8_lossy(&output.stderr).contains(message),
                        "{} {backend}: expected {message:?}, got {}",
                        source.line,
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                assert!(!executable.exists());
            }
        });
        if checked.is_err() {
            failures.push(source.line.clone());
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
    assert!(
        failures.is_empty(),
        "tutorial examples failed at lines {failures:?}"
    );
}
