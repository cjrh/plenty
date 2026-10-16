//! Static test discovery for `plenty test`.
//!
//! Plenty has no reflection or attributes, so tests are found by parsing: a
//! module-level function named `test_*` is a test. The runner appends a
//! generated `main` that calls the test named by the program's one argument,
//! so each test runs in its own process and a trap fails only that test.
use super::*;

/// The return shape of a test function. Only these spellings are accepted;
/// an alias is rejected because discovery runs before name resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestReturn {
    Unit,
    Result,
}

/// What `plenty test` runs from one file in `tests/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestFile {
    /// The file defines `main`: one test that passes on exit status zero.
    Program,
    /// The file's `test_*` functions, in source order.
    Functions(Vec<(String, TestReturn)>),
}

const PREFIX: &str = "test_";

pub fn discover(path: &Path) -> Result<TestFile> {
    let source = ModuleSource::File(path.to_path_buf()).read()?;
    let (module, _) = parse(&source, Some(path), String::new())?;
    let mut main = None;
    let mut tests = vec![];
    for function in &module.functions {
        if function.name == "main" {
            main = Some(function.at.clone());
            continue;
        }
        if !function.name.starts_with(PREFIX) {
            continue;
        }
        let output = &function.output;
        let returns = match output.name.as_deref() {
            None => Some(TestReturn::Unit),
            Some("Result") if output.args.len() == 2 && output.args[0].name.is_none() => {
                Some(TestReturn::Result)
            }
            _ => None,
        };
        match returns {
            Some(returns)
                if function.inputs.is_empty()
                    && function.type_params.is_empty()
                    && function.foreign.is_none() =>
            {
                tests.push((function.name.clone(), returns));
            }
            _ => {
                return Err(function.at.error(format!(
                    "test function `{}` must take no parameters and return `()` or `Result[(), E]`; rename it if it is not a test",
                    function.name
                )))
            }
        }
    }
    match main {
        Some(at) if !tests.is_empty() => Err(at.error(format!(
            "a test file defines either `main` or `{PREFIX}` functions, not both; move `{}` to its own file",
            tests[0].0
        ))),
        Some(_) => Ok(TestFile::Program),
        None if tests.is_empty() => Err(format!(
            "{}: a test file must define `main` or at least one `{PREFIX}` function; put helper modules in a subdirectory of tests/",
            path.display()
        )
        .into()),
        None => Ok(TestFile::Functions(tests)),
    }
}

/// Exit status of a test that returned `Err`. A trap exits 1, so the runner
/// can tell the two apart.
pub const RETURNED_ERR: i32 = 3;

/// Errors whose repr says nothing beyond what std.test already wrote to
/// standard error, or nothing at all, so naming them would only add noise.
const UNINFORMATIVE: [&str; 2] = ["Failure.Unspecified", "std.test.TestError.Assertion"];

/// Source appended to a file of test functions. With one argument naming a
/// test, `main` runs it and exits 0 when it passes and [`RETURNED_ERR`] when it
/// returns `Err`; anything else exits 2. A returned error is written to
/// standard error as `error: NAME returned ERROR` unless it is
/// [`UNINFORMATIVE`]. The generated code declares nothing but `main`, so it
/// cannot collide with a name in the file.
pub fn appendix(tests: &[(String, TestReturn)]) -> String {
    let mut out = String::from(
        "\n\ndef main() -> i32:
    match args():
        case Ok(arguments):
            if len(arguments) != 2:
                return 2
",
    );
    for (name, returns) in tests {
        out.push_str(&format!("            if arguments[1] == \"{name}\":\n"));
        match returns {
            TestReturn::Unit => out.push_str(&format!(
                "                {name}()\n                return 0\n"
            )),
            TestReturn::Result => out.push_str(&format!(
                "                match {name}():
                    case Ok(_):
                        return 0
                    case Err(error):
                        drop(flush_stdout())
                        match str.repr(&error):
                            case Ok(text):
                                if text != \"{}\" and text != \"{}\":
                                    drop(write_stderr(\"error: {name} returned \"))
                                    drop(write_stderr(text))
                                    drop(write_stderr(\"\\n\"))
                            case Err(_):
                                pass
                        return {RETURNED_ERR}
",
                UNINFORMATIVE[0], UNINFORMATIVE[1]
            )),
        }
    }
    out.push_str("            2\n        case Err(_):\n            2\n");
    out
}
