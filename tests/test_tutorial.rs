//! Execute the learner's guide and supplementary reference examples:
//! `plenty` fences require
//! `output` fences; `plenty-error` fences require an `error` fence whose every
//! line is a substring of the diagnostic, so a lesson can show a diagnostic's
//! note without the temporary path between its lines. Preceding `plenty-file path.plenty` fences on the same page
//! supply modules for that one example.
//! `sh plenty-build` fences run explicit library/interface build commands.
//! `LANG project-file DIR/PATH` fences write files of a package project, and a
//! following `sh plenty-project DIR` fence runs one `plenty` project command in
//! `DIR`; the next `output` fence is its exact standard output. Each command
//! runs in a fresh copy of every project file written so far on its page.
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

/// One independently runnable check: an example with its companion modules and
/// library builds, or a project command with the project files written before
/// it on its page.
enum Job<'a> {
    Example {
        source: &'a Fence,
        expected: &'a Fence,
        modules: Vec<(PathBuf, &'a Fence)>,
        builds: Vec<&'a Fence>,
    },
    Project {
        command: &'a Fence,
        directory: &'a str,
        expected: &'a Fence,
        files: Vec<(PathBuf, &'a Fence)>,
    },
}

fn tutorial_fences() -> Vec<Fence> {
    summary_pages()
        .into_iter()
        .filter(|page| page.starts_with("tutorial/") || page.starts_with("design/examples/"))
        .flat_map(|page| {
            let markdown = std::fs::read_to_string(book_src().join(&page)).unwrap();
            fences(&page, &markdown)
        })
        .collect()
}

/// Check the structure of every fence in reading order and pair each example
/// or project command with its expectation and companion files.
fn jobs(fences: &[Fence]) -> Vec<Job<'_>> {
    let mut jobs = Vec::new();
    let mut index = 0;
    let mut modules: Vec<(PathBuf, &Fence)> = Vec::new();
    let mut builds = Vec::new();
    let mut project_files: Vec<(PathBuf, &Fence)> = Vec::new();
    let mut project_used = false;
    while index < fences.len() {
        let source = &fences[index];
        if let Some((_, path)) = source.language.split_once(" project-file ") {
            assert!(
                Path::new(path)
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
                "invalid project file path at {}",
                source.line
            );
            if project_files
                .first()
                .is_some_and(|(_, file)| page_of(&file.line) != page_of(&source.line))
            {
                assert!(project_used, "orphan project files before {}", source.line);
                project_files.clear();
                project_used = false;
            }
            project_files.push((PathBuf::from(path), source));
            index += 1;
            continue;
        }
        if let Some(directory) = source.language.strip_prefix("sh plenty-project ") {
            let expected = fences
                .get(index + 1)
                .filter(|fence| fence.language == "output")
                .unwrap_or_else(|| {
                    panic!("{}: project command needs an output fence", source.line)
                });
            for (_, file) in &project_files {
                assert_eq!(page_of(&file.line), page_of(&source.line), "{}", file.line);
            }
            project_used = true;
            // One fixed command, without a shell, so lessons show the exact
            // public interface.
            let words: Vec<_> = source.body.split_whitespace().collect();
            assert!(
                matches!(
                    words.as_slice(),
                    ["plenty", "run" | "test" | "check" | "build"]
                ),
                "unsupported project command at {}",
                source.line
            );
            jobs.push(Job::Project {
                command: source,
                directory,
                expected,
                files: project_files.clone(),
            });
            index += 2;
            continue;
        }
        if let Some(path) = source.language.strip_prefix("plenty-file ") {
            let path = PathBuf::from(path);
            assert!(
                !path.as_os_str().is_empty()
                    && path
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)))
                    && path
                        .extension()
                        .is_some_and(|e| e == "plenty" || e == "plentyi")
                    && path != std::path::Path::new("lesson.plenty"),
                "invalid module path at {}",
                source.line
            );
            assert!(
                !modules.iter().any(|(p, _)| p == &path),
                "duplicate tutorial module"
            );
            modules.push((path, source));
            index += 1;
            continue;
        }
        if source.language == "sh plenty-build" {
            builds.push(source);
            index += 1;
            continue;
        }
        if !matches!(
            source.language.as_str(),
            "plenty" | "plenty dataframe" | "plenty-error"
        ) {
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
        let valid = matches!(source.language.as_str(), "plenty" | "plenty dataframe");
        assert_eq!(
            expected.language,
            if valid { "output" } else { "error" },
            "{}",
            source.line
        );
        for (_, module) in &modules {
            assert_eq!(
                page_of(&module.line),
                page_of(&source.line),
                "companion module for {} is on another page",
                source.line
            );
        }
        for build in &builds {
            assert_eq!(
                page_of(&build.line),
                page_of(&source.line),
                "library build is on another page"
            );
            build_commands(build);
        }
        jobs.push(Job::Example {
            source,
            expected,
            modules: std::mem::take(&mut modules),
            builds: std::mem::take(&mut builds),
        });
        index += 2;
    }
    assert!(
        jobs.iter().any(|job| matches!(job, Job::Example { .. })),
        "the tutorial must teach with executable examples"
    );
    assert!(
        modules.is_empty(),
        "orphan tutorial module without an example"
    );
    assert!(
        builds.is_empty(),
        "orphan tutorial library build without an example"
    );
    assert!(
        project_files.is_empty() || project_used,
        "orphan tutorial project file without a project command"
    );
    jobs
}

/// The words of each command in a `sh plenty-build` fence.
fn build_commands(build: &Fence) -> Vec<Vec<&str>> {
    build
        .body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            // Intentionally no shell: lessons use the exact public CLI and
            // cannot substitute arbitrary scripts for a language example.
            let words: Vec<_> = line.split_whitespace().collect();
            assert!(
                words.len() == 7
                    && words[0] == "plenty"
                    && matches!(words[1], "--shared-library" | "--runtime-interface")
                    && words[3] == "--library-name"
                    && words[5] == "-o",
                "unsupported tutorial build at {}: {line}",
                build.line
            );
            for path in [words[2], words[6]] {
                assert!(
                    Path::new(path)
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                    "invalid build path at {}",
                    build.line
                );
            }
            words
        })
        .collect()
}

/// Run job `number` (its position in reading order, which names its files)
/// in `workspace`, returning whether its output matched.
///
/// Every runnable example is built once with `plenty --compile` and its
/// binary's output checked. Linking each one a second time through
/// `plenty FILE` adds little, since both share the whole compiler, so only
/// the first runnable example in each book directory also runs that way:
/// `run_command` is set for it (issue #411). Invalid examples, which do not
/// link, go through both.
fn run_job(job: &Job, number: usize, workspace: &Path, run_command: bool) -> bool {
    let binary = env!("CARGO_BIN_EXE_plenty");
    let (source, expected, modules, builds) = match job {
        Job::Project {
            command,
            directory,
            expected,
            files,
        } => {
            let project_dir = workspace.join(format!("project-{number}"));
            for (path, file) in files {
                let path = project_dir.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, &file.body).unwrap();
            }
            let words: Vec<_> = command.body.split_whitespace().collect();
            let output = Command::new(binary)
                .arg(words[1])
                .current_dir(project_dir.join(directory))
                .output()
                .unwrap();
            if !output.status.success() || String::from_utf8_lossy(&output.stdout) != expected.body
            {
                eprintln!(
                    "{}: status {}\nstdout:\n{}\nstderr:\n{}",
                    command.line,
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                return false;
            }
            return true;
        }
        Job::Example {
            source,
            expected,
            modules,
            builds,
        } => (source, expected, modules, builds),
    };
    let valid = matches!(source.language.as_str(), "plenty" | "plenty dataframe");
    let example_dir = workspace.join(format!("example-{number}"));
    std::fs::create_dir(&example_dir).unwrap();
    for (path, module) in modules {
        let path = example_dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &module.body).unwrap();
    }
    let mut libraries = Vec::new();
    for build in builds {
        for words in build_commands(build) {
            let output = Command::new(binary)
                .args(&words[1..])
                .current_dir(&example_dir)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}: {}\n{}",
                build.line,
                words.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
            if words[1] == "--shared-library" {
                libraries.push(PathBuf::from(words[6]));
            }
        }
    }
    let source_path = example_dir.join("lesson.plenty");
    std::fs::write(&source_path, &source.body).unwrap();
    if source.language == "plenty dataframe" && std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none()
    {
        let checked = Command::new(binary)
            .arg("--check")
            .arg(&source_path)
            .output()
            .unwrap();
        assert!(
            checked.status.success(),
            "{} type check: {}",
            source.line,
            String::from_utf8_lossy(&checked.stderr)
        );
        eprintln!(
            "{}: dataframe example type-checked; execution requires PLENTY_DATAFRAME_TESTS=1",
            source.line
        );
        return true;
    }
    // Runtime examples may create files. Give each execution a separate
    // empty working directory so examples neither touch the repo nor rely
    // on files left behind by the other execution mode.
    let run_command = run_command || !valid;
    let run_dir = example_dir.join("run");
    let native_dir = example_dir.join("native");
    std::fs::create_dir(&run_dir).unwrap();
    std::fs::create_dir(&native_dir).unwrap();
    for library in libraries {
        for directory in [&run_dir, &native_dir] {
            let destination = directory.join(&library);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::copy(example_dir.join(&library), destination).unwrap();
        }
    }
    let run_output = run_command.then(|| {
        Command::new(binary)
            .arg(&source_path)
            .current_dir(&run_dir)
            .output()
            .unwrap()
    });
    let executable = workspace.join(format!("lesson-{number}"));
    let compiled = Command::new(binary)
        .arg("--compile")
        .arg(&source_path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    std::panic::catch_unwind(|| {
        if valid {
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
                String::from_utf8_lossy(&native.stdout),
                expected.body,
                "{} native output",
                source.line
            );
            if let Some(run_output) = run_output {
                assert!(
                    run_output.status.success(),
                    "{} run command: {}",
                    source.line,
                    String::from_utf8_lossy(&run_output.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&run_output.stdout),
                    expected.body,
                    "{} run command output",
                    source.line
                );
            }
        } else {
            let message = expected.body.trim();
            assert!(!message.is_empty());
            let run_output = run_output.expect("invalid examples use the run command");
            for (backend, output) in [("run command", run_output), ("compiler", compiled)] {
                assert!(
                    !output.status.success(),
                    "{} {backend} accepted an invalid program",
                    source.line
                );
                assert!(output.stdout.is_empty(), "invalid program had effects");
                let stderr = String::from_utf8_lossy(&output.stderr);
                for line in message.lines().map(str::trim) {
                    assert!(
                        stderr.contains(line),
                        "{} {backend}: expected {line:?}, got {stderr}",
                        source.line
                    );
                }
            }
            assert!(!executable.exists());
        }
    })
    .is_ok()
}

/// For each job, whether it is the first runnable example in its book
/// directory, which [`run_job`] also checks through `plenty FILE`.
fn first_runnable_example_per_directory(jobs: &[Job]) -> Vec<bool> {
    let mut seen = std::collections::HashSet::new();
    jobs.iter()
        .map(|job| match job {
            Job::Example { source, .. }
                if matches!(source.language.as_str(), "plenty" | "plenty dataframe") =>
            {
                let (page, _) = source.line.rsplit_once(':').unwrap();
                seen.insert(page.rsplit_once('/').map_or("", |(directory, _)| directory))
            }
            _ => false,
        })
        .collect()
}

/// Run every `shards`th job in reading order, starting at `shard`. Every
/// shard checks the structure of the whole tutorial.
fn run_shard(shard: usize, shards: usize) {
    let fences = tutorial_fences();
    let jobs = jobs(&fences);
    let workspace = Workspace(
        std::env::temp_dir().join(format!("plenty-tutorial-{}-{shard}", std::process::id())),
    );
    std::fs::create_dir(&workspace.0).unwrap();
    let run_command = first_runnable_example_per_directory(&jobs);
    let failures: Vec<&str> = jobs
        .iter()
        .enumerate()
        .skip(shard)
        .step_by(shards)
        .filter(|(number, job)| !run_job(job, *number, &workspace.0, run_command[*number]))
        .map(|(_, job)| match job {
            Job::Example { source, .. } => source.line.as_str(),
            Job::Project { command, .. } => command.line.as_str(),
        })
        .collect();
    assert!(
        failures.is_empty(),
        "tutorial examples failed at lines {failures:?}"
    );
}

#[test]
fn dataframe_examples_with_optional_engine() {
    if std::env::var_os("PLENTY_DATAFRAME_TESTS").is_none() {
        return;
    }
    let fences = tutorial_fences();
    let jobs = jobs(&fences);
    let temp = tempfile::tempdir().unwrap();
    let mut count = 0;
    for (number, job) in jobs.iter().enumerate() {
        if matches!(job, Job::Example { source, .. } if source.language == "plenty dataframe") {
            assert!(run_job(job, number, temp.path(), true));
            count += 1;
        }
    }
    assert!(
        count > 0,
        "the optional engine job must exercise tutorial examples"
    );
}

/// The examples run as several tests, so the harness runs them in parallel
/// with each other and with the rest of the suite (issue #139).
mod every_tutorial_program_and_diagnostic_matches_the_language {
    macro_rules! shards {
        ($($name:ident: $shard:literal,)*) => {
            const SHARDS: usize = [$($shard),*].len();
            $(
                #[test]
                fn $name() {
                    super::run_shard($shard, SHARDS);
                }
            )*
        };
    }

    shards! {
        shard_00: 0,
        shard_01: 1,
        shard_02: 2,
        shard_03: 3,
        shard_04: 4,
        shard_05: 5,
        shard_06: 6,
        shard_07: 7,
        shard_08: 8,
        shard_09: 9,
        shard_10: 10,
        shard_11: 11,
        shard_12: 12,
        shard_13: 13,
        shard_14: 14,
        shard_15: 15,
    }
}
