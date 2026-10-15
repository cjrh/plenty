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

/// Source appended to a file of test functions. With one argument naming a
/// test, `main` runs it and exits 0 when it passes and 1 when it fails;
/// anything else exits 2. A failing test's error is reported the way the
/// entrypoint reports one returned by `main` (without its explanation of
/// standard errors), naming the test instead. The
/// generated code declares nothing but `main`, so it cannot collide with a
/// name in the file.
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
                        drop(write_stderr(\"error: {name} returned \"))
                        match str.repr(&error):
                            case Ok(text):
                                drop(write_stderr(text))
                            case Err(_):
                                drop(write_stderr(\"Err\"))
                        drop(write_stderr(\"\\n\"))
                        return 1
"
            )),
        }
    }
    out.push_str("            2\n        case Err(_):\n            2\n");
    out
}
