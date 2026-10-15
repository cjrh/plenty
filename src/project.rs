//! Packages: `plenty.toml` manifests, local dependency graphs, `plenty.lock`,
//! and the project commands behind `plenty build`, `run`, `check`, and `test`.
//!
//! The compiler sees a project only as a [`SourceGraph`]: one source root per
//! package and each package's direct dependency aliases. Dependency resolution
//! never happens while evaluating imports.
pub mod manifest;
mod toml;

use manifest::{Dependency, Manifest, MANIFEST};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub const LOCKFILE: &str = "plenty.lock";

/// One package's source tree as the compiler sees it.
#[derive(Clone, Debug)]
pub struct SourcePackage {
    /// Unique within a graph. Qualifies the declarations of every package
    /// except the entry package, so it must not be a caller's alias.
    pub identity: String,
    /// Imports in this package resolve below this directory.
    pub root: PathBuf,
    /// The module that `import ALIAS` names, when another package depends on
    /// this one.
    pub library: Option<PathBuf>,
    /// `(alias, index into SourceGraph::packages)` for direct dependencies.
    pub dependencies: Vec<(String, usize)>,
}

/// The packages of one compilation. `packages[0]` contains the entry file;
/// its modules keep their unqualified source names.
#[derive(Clone, Debug)]
pub struct SourceGraph {
    pub packages: Vec<SourcePackage>,
}

/// Selects the generated code configuration, not the language rules: bounds,
/// overflow, and ownership checks are the same in both profiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    pub fn name(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

/// A resolved package and its direct dependencies.
#[derive(Clone, Debug)]
pub struct Package {
    pub manifest: Manifest,
    /// Canonical package directory, containing `plenty.toml`.
    pub directory: PathBuf,
    pub dependencies: Vec<(String, usize)>,
}

/// A root package and its transitive dependency graph. `packages[0]` is root.
#[derive(Debug)]
pub struct Project {
    pub packages: Vec<Package>,
}

/// The nearest directory at or above `start` that contains `plenty.toml`.
pub fn discover(start: &Path) -> Result<PathBuf> {
    let start = start.canonicalize()?;
    start
        .ancestors()
        .find(|dir| dir.join(MANIFEST).is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "could not find {MANIFEST} in {} or any parent directory",
                start.display()
            )
            .into()
        })
}

fn read_manifest(directory: &Path) -> Result<Manifest> {
    let path = directory.join(MANIFEST);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let manifest = manifest::parse(&text, &path.display().to_string(), directory)?;
    if let Some(minimum) = &manifest.plenty_version {
        let required = manifest::parse_version(minimum).unwrap();
        let current = manifest::parse_version(env!("CARGO_PKG_VERSION")).unwrap();
        if required > current {
            return Err(format!(
                "package `{}` requires Plenty {minimum} or newer; this toolchain is {}",
                manifest.name,
                env!("CARGO_PKG_VERSION")
            )
            .into());
        }
    }
    Ok(manifest)
}

impl Project {
    /// Read the root manifest in `directory` and every path dependency.
    pub fn load(directory: &Path) -> Result<Project> {
        let directory = directory
            .canonicalize()
            .map_err(|e| format!("{}: {e}", directory.display()))?;
        let mut project = Project { packages: vec![] };
        project.visit(directory, &mut vec![])?;
        Ok(project)
    }

    /// Depth-first resolution. `chain` holds the dependency path from the
    /// root, so cycles and conflicts can name how each package was reached.
    fn visit(&mut self, directory: PathBuf, chain: &mut Vec<String>) -> Result<usize> {
        // Callers report cycles before revisiting, so a known directory here
        // is a finished diamond dependency.
        if let Some(index) = self.packages.iter().position(|p| p.directory == directory) {
            return Ok(index);
        }
        let manifest = read_manifest(&directory)?;
        if chain.contains(&manifest.name) {
            return Err(format!(
                "dependency cycle: {} -> {}",
                chain.join(" -> "),
                manifest.name
            )
            .into());
        }
        if let Some(other) = self
            .packages
            .iter()
            .find(|p| p.manifest.name == manifest.name)
        {
            return Err(format!(
                "two different packages are named `{}`: {} and {} (reached through {}); a graph may contain one package per name",
                manifest.name,
                other.directory.display(),
                directory.display(),
                chain.join(" -> ")
            )
            .into());
        }
        let index = self.packages.len();
        self.packages.push(Package {
            manifest: manifest.clone(),
            directory: directory.clone(),
            dependencies: vec![],
        });
        chain.push(manifest.name.clone());
        let mut dependencies = vec![];
        for (alias, dependency) in &manifest.dependencies {
            let path = match dependency {
                Dependency::Path(path) => directory.join(path),
                Dependency::Git { url, .. } => {
                    return Err(format!(
                        "package `{}`: dependency `{alias}` uses git ({url}); git dependencies are not supported yet, use a local `path` dependency",
                        manifest.name
                    )
                    .into())
                }
            };
            if !path.join(MANIFEST).is_file() {
                return Err(format!(
                    "package `{}`: dependency `{alias}` at {} has no {MANIFEST}",
                    manifest.name,
                    path.display()
                )
                .into());
            }
            let path = path.canonicalize()?;
            if chain_directories(self, chain).contains(&path) {
                let name = self
                    .packages
                    .iter()
                    .find(|p| p.directory == path)
                    .map(|p| p.manifest.name.clone())
                    .unwrap_or_default();
                return Err(format!("dependency cycle: {} -> {name}", chain.join(" -> ")).into());
            }
            let target = self.visit(path, chain)?;
            if self.packages[target].manifest.library.is_none() {
                return Err(format!(
                    "package `{}`: dependency `{alias}` (package `{}`) has no library target",
                    manifest.name, self.packages[target].manifest.name
                )
                .into());
            }
            dependencies.push((alias.clone(), target));
        }
        chain.pop();
        self.packages[index].dependencies = dependencies;
        Ok(index)
    }

    pub fn root(&self) -> &Package {
        &self.packages[0]
    }

    fn source_package(&self, index: usize) -> SourcePackage {
        let package = &self.packages[index];
        SourcePackage {
            identity: package.manifest.name.clone(),
            root: package.directory.join("src"),
            library: package
                .manifest
                .library
                .as_ref()
                .map(|library| package.directory.join(library)),
            dependencies: package.dependencies.clone(),
        }
    }

    /// The compiler's view, with package `entry` first.
    pub fn source_graph(&self, entry: usize) -> SourceGraph {
        let mut packages: Vec<_> = (0..self.packages.len())
            .map(|i| self.source_package(i))
            .collect();
        packages.swap(0, entry);
        let remap = |i: usize| {
            if i == 0 {
                entry
            } else if i == entry {
                0
            } else {
                i
            }
        };
        for package in &mut packages {
            for (_, target) in &mut package.dependencies {
                *target = remap(*target);
            }
        }
        SourceGraph { packages }
    }

    /// Integration tests form their own entry package rooted at `tests/`.
    /// They see the root package's dependencies and the root package itself,
    /// under its package name.
    pub fn test_graph(&self) -> SourceGraph {
        let mut graph = self.source_graph(0);
        let root = &self.root();
        let mut dependencies: Vec<_> = root
            .dependencies
            .iter()
            .map(|(alias, target)| (alias.clone(), target + 1))
            .collect();
        if root.manifest.library.is_some() {
            dependencies.push((root.manifest.name.clone(), 1));
        }
        for package in &mut graph.packages {
            for (_, target) in &mut package.dependencies {
                *target += 1;
            }
        }
        graph.packages.insert(
            0,
            SourcePackage {
                identity: format!("{}-tests", root.manifest.name),
                root: root.directory.join("tests"),
                library: None,
                dependencies,
            },
        );
        graph
    }

    /// The lockfile text for this graph: deterministic, and independent of
    /// where the project is checked out.
    pub fn lockfile(&self) -> String {
        let mut order: Vec<usize> = (1..self.packages.len()).collect();
        order.sort_by(|a, b| {
            self.packages[*a]
                .manifest
                .name
                .cmp(&self.packages[*b].manifest.name)
        });
        order.insert(0, 0);
        let mut out = String::from(
            "# Generated by `plenty`. Commit this file; do not edit it by hand.\nlock-version = 1\n",
        );
        for index in order {
            let package = &self.packages[index];
            let source = if index == 0 {
                "root".to_string()
            } else {
                format!(
                    "path+{}",
                    relative_path(&self.root().directory, &package.directory)
                )
            };
            let dependencies = package
                .dependencies
                .iter()
                .map(|(alias, target)| {
                    format!(
                        "{{ alias = {}, package = {} }}",
                        toml::quote(alias),
                        toml::quote(&self.packages[*target].manifest.name)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "\n[[package]]\nname = {}\nversion = {}\nsource = {}\ndependencies = [{dependencies}]\n",
                toml::quote(&package.manifest.name),
                toml::quote(&package.manifest.version),
                toml::quote(&source),
            ));
        }
        out
    }

    /// Bring `plenty.lock` up to date, or with `locked` require that it is.
    pub fn sync_lockfile(&self, locked: bool) -> Result<()> {
        let path = self.root().directory.join(LOCKFILE);
        let expected = self.lockfile();
        let current = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("reading {}: {e}", path.display()).into()),
        };
        if current.as_deref() == Some(expected.as_str()) {
            return Ok(());
        }
        if locked {
            return Err(match current {
                None => format!("{LOCKFILE} is missing and --locked forbids creating it"),
                Some(_) => format!(
                    "{LOCKFILE} does not match the dependencies in {MANIFEST}, and --locked forbids updating it"
                ),
            }
            .into());
        }
        write_atomically(&path, expected.as_bytes())
    }
}

fn chain_directories(project: &Project, chain: &[String]) -> Vec<PathBuf> {
    project
        .packages
        .iter()
        .filter(|p| chain.contains(&p.manifest.name))
        .map(|p| p.directory.clone())
        .collect()
}

/// `to` relative to `from`, both canonical, with `/` separators.
fn relative_path(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// Write beside the destination, then rename, so an interruption never
/// leaves a partially written file.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path.parent().unwrap();
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| format!("writing {}: {}", path.display(), e.error))?;
    Ok(())
}

/// Options shared by the commands that compile a project.
#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub profile: Profile,
    /// Refuse to create or change `plenty.lock`.
    pub locked: bool,
    pub compile: crate::CompileOptions,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            profile: Profile::Debug,
            locked: false,
            compile: crate::CompileOptions::default(),
        }
    }
}

fn open(directory: &Path, options: &BuildOptions) -> Result<Project> {
    let project = Project::load(&discover(directory)?)?;
    project.sync_lockfile(options.locked)?;
    Ok(project)
}

fn check_entry(entry: &Path, graph: &SourceGraph, require_main: bool) -> Result<()> {
    let mut heap = crate::value::Heap::default();
    let program = crate::frontend::compile_graph(entry, graph, require_main, &mut heap)?;
    crate::op::check(&program.ops)
}

fn status(verb: &str, message: impl std::fmt::Display) {
    eprintln!("{verb:>10} {message}");
}

/// Check every target of the project at or above `directory`.
pub fn check(directory: &Path, options: &BuildOptions) -> Result<()> {
    let project = open(directory, options)?;
    let root = project.root();
    status(
        "Checking",
        format!("{} v{}", root.manifest.name, root.manifest.version),
    );
    let graph = project.source_graph(0);
    if let Some(library) = &root.manifest.library {
        check_entry(&root.directory.join(library), &graph, false)?;
    }
    for (_, entry) in &root.manifest.binaries {
        check_entry(&root.directory.join(entry), &graph, true)?;
    }
    Ok(())
}

fn output_directory(project: &Project, profile: Profile) -> PathBuf {
    project.root().directory.join("target").join(profile.name())
}

fn compile_options(options: &BuildOptions) -> crate::CompileOptions {
    let mut compile = options.compile.clone();
    compile.optimize = options.profile == Profile::Release;
    compile
}

fn build_binary(
    project: &Project,
    name: &str,
    entry: &Path,
    options: &BuildOptions,
) -> Result<PathBuf> {
    let directory = output_directory(project, options.profile);
    std::fs::create_dir_all(&directory)?;
    let output = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    crate::compile_package_to_executable(
        &project.root().directory.join(entry),
        &project.source_graph(0),
        &output,
        &compile_options(options),
    )?;
    Ok(output)
}

/// Build every executable target into `target/PROFILE/`, and check the
/// library target. Returns the executables' paths.
pub fn build(directory: &Path, options: &BuildOptions) -> Result<Vec<PathBuf>> {
    let project = open(directory, options)?;
    let root = project.root();
    status(
        "Compiling",
        format!(
            "{} v{} ({})",
            root.manifest.name,
            root.manifest.version,
            options.profile.name()
        ),
    );
    if let Some(library) = &root.manifest.library {
        check_entry(
            &root.directory.join(library),
            &project.source_graph(0),
            false,
        )?;
    }
    root.manifest
        .binaries
        .iter()
        .map(|(name, entry)| build_binary(&project, name, entry, options))
        .collect()
}

/// Build one executable target and run it with `arguments`, inheriting
/// standard streams, environment, and working directory. Returns the child's
/// exit status.
pub fn run(
    directory: &Path,
    binary: Option<&str>,
    arguments: &[String],
    options: &BuildOptions,
) -> Result<ExitCode> {
    let project = open(directory, options)?;
    let root = project.root();
    let names = || {
        root.manifest
            .binaries
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (name, entry) = match binary {
        Some(wanted) => root
            .manifest
            .binaries
            .iter()
            .find(|(name, _)| name == wanted)
            .ok_or_else(|| format!("no executable target `{wanted}`; available: {}", names()))?,
        None => match root.manifest.binaries.as_slice() {
            [only] => only,
            [] => {
                return Err(format!(
                    "package `{}` has no executable target to run",
                    root.manifest.name
                )
                .into())
            }
            _ => {
                return Err(format!(
                    "package `{}` has several executables; choose one with --bin NAME: {}",
                    root.manifest.name,
                    names()
                )
                .into())
            }
        },
    };
    status(
        "Compiling",
        format!(
            "{} v{} ({})",
            root.manifest.name,
            root.manifest.version,
            options.profile.name()
        ),
    );
    let executable = build_binary(&project, name, entry, options)?;
    status("Running", relative_path(&root.directory, &executable));
    let status = Command::new(&executable)
        .args(arguments)
        .status()
        .map_err(|e| format!("running {}: {e}", executable.display()))?;
    Ok(exit_code(status))
}

/// A child's exit status as this process's: Unix signal termination becomes
/// 128 plus the signal number.
pub fn exit_code(status: std::process::ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return ExitCode::from(code as u8);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return ExitCode::from((128 + signal) as u8);
        }
    }
    ExitCode::FAILURE
}

/// The outcome of [`test`].
#[derive(Debug, Default)]
pub struct TestSummary {
    pub passed: Vec<String>,
    pub failed: Vec<String>,
    /// Tests that a filter excluded.
    pub filtered: usize,
}

/// Options for [`test`].
#[derive(Clone, Debug, Default)]
pub struct TestOptions {
    pub build: BuildOptions,
    /// Run only tests whose name contains one of these; all when empty.
    pub filters: Vec<String>,
    /// Let tests write straight to the terminal instead of capturing their
    /// output and showing it only when they fail.
    pub no_capture: bool,
    /// Passed to every test as `PLENTY_TEST_SEED`, which `std.test.seed`
    /// reads.
    pub seed: Option<u64>,
    /// Print the selected tests' names instead of running them.
    pub list: bool,
}

/// One test: a whole program, or one function of a file of test functions.
struct TestCase {
    name: String,
    file: usize,
    function: Option<String>,
}

/// A test file and what it defines.
type TestSource = (PathBuf, crate::frontend::TestFile);

/// Collect the tests in `tests/`. A file that defines `main` is one test
/// named after the file; otherwise each `test_*` function is a test named
/// `FILE::FUNCTION`. Files in subdirectories are helper modules.
fn discover_tests(tests_directory: &Path) -> Result<(Vec<TestSource>, Vec<TestCase>)> {
    let mut paths = vec![];
    if tests_directory.is_dir() {
        for entry in std::fs::read_dir(tests_directory)? {
            let path = entry?.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "plenty") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    let mut files = vec![];
    let mut cases = vec![];
    for (index, path) in paths.into_iter().enumerate() {
        let kind = crate::frontend::discover_tests(&path)?;
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        match &kind {
            crate::frontend::TestFile::Program => cases.push(TestCase {
                name: stem,
                file: index,
                function: None,
            }),
            crate::frontend::TestFile::Functions(functions) => {
                for (function, _) in functions {
                    cases.push(TestCase {
                        name: format!("{stem}::{function}"),
                        file: index,
                        function: Some(function.clone()),
                    });
                }
            }
        }
        files.push((path, kind));
    }
    Ok((files, cases))
}

/// Compile the selected tests and run each in its own process from the
/// package directory. A test passes when its process exits with status zero.
pub fn test(directory: &Path, options: &TestOptions) -> Result<TestSummary> {
    let project = open(directory, &options.build)?;
    let root = project.root();
    let (files, cases) = discover_tests(&root.directory.join("tests"))?;
    let total = cases.len();
    let cases: Vec<_> = cases
        .into_iter()
        .filter(|case| {
            options.filters.is_empty() || options.filters.iter().any(|f| case.name.contains(f))
        })
        .collect();
    let mut summary = TestSummary {
        filtered: total - cases.len(),
        ..TestSummary::default()
    };
    if options.list {
        for case in &cases {
            println!("{}", case.name);
        }
        return Ok(summary);
    }
    status(
        "Compiling",
        format!(
            "{} v{} tests ({})",
            root.manifest.name,
            root.manifest.version,
            options.build.profile.name()
        ),
    );
    let output = output_directory(&project, options.build.profile).join("tests");
    std::fs::create_dir_all(&output)?;
    let graph = project.test_graph();
    let compile = compile_options(&options.build);
    let mut executables = vec![None; files.len()];
    for case in &cases {
        if executables[case.file].is_some() {
            continue;
        }
        let (path, kind) = &files[case.file];
        let stem = path.file_stem().unwrap().to_string_lossy();
        let executable = output.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX));
        match kind {
            crate::frontend::TestFile::Program => {
                crate::compile_package_to_executable(path, &graph, &executable, &compile)?
            }
            crate::frontend::TestFile::Functions(functions) => {
                crate::codegen::compile_tests_to_executable(
                    path,
                    &graph,
                    functions,
                    &executable,
                    &compile,
                )?
            }
        }
        executables[case.file] = Some(executable);
    }
    let plural = if cases.len() == 1 { "" } else { "s" };
    println!("running {} test{plural}", cases.len());
    let mut failures = vec![];
    for case in &cases {
        let executable = executables[case.file].as_ref().unwrap();
        let mut command = Command::new(executable);
        command
            .current_dir(&root.directory)
            .stdin(std::process::Stdio::null());
        if let Some(function) = &case.function {
            command.arg(function);
        }
        if let Some(seed) = options.seed {
            command.env("PLENTY_TEST_SEED", seed.to_string());
        }
        let run = |e: std::io::Error| format!("running {}: {e}", executable.display());
        let (status, captured) = if options.no_capture {
            (command.status().map_err(run)?, vec![])
        } else {
            // One file for both streams keeps their interleaving, and unlike
            // a pipe it cannot fill up and stall the test.
            let mut log = tempfile::tempfile()?;
            command.stdout(log.try_clone()?).stderr(log.try_clone()?);
            let status = command.status().map_err(run)?;
            let mut captured = vec![];
            std::io::Seek::rewind(&mut log)?;
            std::io::Read::read_to_end(&mut log, &mut captured)?;
            (status, captured)
        };
        if status.success() {
            println!("test {} ... ok", case.name);
            summary.passed.push(case.name.clone());
        } else {
            println!("test {} ... FAILED ({})", case.name, describe(status));
            summary.failed.push(case.name.clone());
            failures.push((case.name.clone(), captured));
        }
    }
    if !options.no_capture && failures.iter().any(|(_, output)| !output.is_empty()) {
        println!("\nfailures:");
        for (name, captured) in &failures {
            if captured.is_empty() {
                continue;
            }
            println!("\n---- {name} output ----");
            print!("{}", String::from_utf8_lossy(captured));
            if !captured.ends_with(b"\n") {
                println!();
            }
        }
    }
    let filtered = if options.filters.is_empty() {
        String::new()
    } else {
        format!("; {} filtered out", summary.filtered)
    };
    println!(
        "\ntest result: {}. {} passed; {} failed{filtered}",
        if summary.failed.is_empty() {
            "ok"
        } else {
            "FAILED"
        },
        summary.passed.len(),
        summary.failed.len()
    );
    Ok(summary)
}

fn describe(status: std::process::ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit status {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("terminated by signal {signal}");
        }
    }
    "terminated abnormally".into()
}

/// Remove `target/` from the project at or above `directory`.
pub fn clean(directory: &Path) -> Result<()> {
    let root = discover(directory)?;
    let target = root.join("target");
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .map_err(|e| format!("removing {}: {e}", target.display()))?;
    }
    Ok(())
}

/// The kind of package `plenty new` and `plenty init` create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    Application,
    Library,
}

/// Create a package in the existing, manifest-free `directory`.
pub fn init(directory: &Path, name: Option<&str>, template: Template) -> Result<()> {
    let name = match name {
        Some(name) => name.to_owned(),
        None => directory
            .canonicalize()?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    if !manifest::valid_name(&name) || name == "std" {
        return Err(format!(
            "package name {name:?} must be a lowercase identifier other than `std`; choose one with --name NAME"
        )
        .into());
    }
    let manifest_path = directory.join(MANIFEST);
    if manifest_path.exists() {
        return Err(format!("{} already exists", manifest_path.display()).into());
    }
    let (file, source) = match template {
        Template::Application => (
            "src/main.plenty",
            format!(
                "def main() -> Result[(), Failure]:\n    print(\"Hello from {name}!\")?\n    Ok(())\n"
            ),
        ),
        Template::Library => (
            "src/lib.plenty",
            "pub def add(left: i64, right: i64) -> i64:\n    left + right\n".to_string(),
        ),
    };
    let source_path = directory.join(file);
    if source_path.exists() {
        return Err(format!("{} already exists", source_path.display()).into());
    }
    std::fs::create_dir_all(directory.join("src"))?;
    std::fs::write(&source_path, source)?;
    std::fs::write(&manifest_path, manifest::template(&name))?;
    let ignore = directory.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, "/target\n/.pkg\n")?;
    }
    Ok(())
}

/// Create `directory` and a package inside it.
pub fn new(directory: &Path, name: Option<&str>, template: Template) -> Result<()> {
    if directory.exists() {
        return Err(format!("{} already exists", directory.display()).into());
    }
    std::fs::create_dir_all(directory)?;
    init(directory, name, template)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/a/c/d")),
            "../c/d"
        );
        assert_eq!(relative_path(Path::new("/a"), Path::new("/a/b")), "b");
        assert_eq!(relative_path(Path::new("/a"), Path::new("/a")), ".");
    }
}
