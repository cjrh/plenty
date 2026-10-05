//! Plenty file runner, native compiler, and multiline REPL.
use std::error::Error;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use plenty::Vm;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{
    Cmd, ConditionalEventHandler, Context, Editor, Event, EventContext, EventHandler, Helper,
    Highlighter, Hinter, KeyCode, KeyEvent, Modifiers, RepeatCount,
};

const USAGE: &str = "\
Usage: plenty [FILE]
       plenty --check FILE
       plenty --compile FILE -o OUT
       plenty -h | --help

No arguments: start the REPL. FILE: check and run modern Plenty.
--check: parse and type-check without executing the program.
--compile: emit a native executable with Cranelift and the system cc linker.
--legacy before FILE or --compile selects the historical stack syntax.
";

const WORDS: &[&str] = &[
    "def", "return", "if", "elif", "else", "mut", "pass", "True", "False", "and", "or", "not",
    "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "int", "bool", "str", "print",
    "contains", "quit",
];

#[derive(Helper, Highlighter, Hinter)]
struct PlentyHelper {
    functions: Vec<String>,
}

impl Validator for PlentyHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        Ok(if plenty::input_complete(ctx.input()) {
            ValidationResult::Valid(None)
        } else {
            ValidationResult::Incomplete
        })
    }
}

impl Completer for PlentyHelper {
    type Candidate = Pair;
    fn complete(
        &self,
        line: &str,
        pos: usize,
        _: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let start = line[..pos]
            .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .map(|i| i + line[i..].chars().next().unwrap().len_utf8())
            .unwrap_or(0);
        let prefix = &line[start..pos];
        let mut names: Vec<&str> = WORDS
            .iter()
            .copied()
            .chain(self.functions.iter().map(String::as_str))
            .filter(|name| !prefix.is_empty() && name.starts_with(prefix))
            .collect();
        names.sort_unstable();
        names.dedup();
        Ok((
            start,
            names
                .into_iter()
                .map(|s| Pair {
                    display: s.into(),
                    replacement: s.into(),
                })
                .collect(),
        ))
    }
}

#[derive(Clone, Default)]
struct EditorTrigger(Arc<AtomicBool>);
impl ConditionalEventHandler for EditorTrigger {
    fn handle(&self, _: &Event, _: RepeatCount, _: bool, _: &EventContext) -> Option<Cmd> {
        self.0.store(true, Ordering::Relaxed);
        Some(Cmd::AcceptLine)
    }
}

fn main() -> ExitCode {
    pretty_env_logger::init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let legacy = args.first().is_some_and(|s| s == "--legacy");
    if legacy {
        args.remove(0);
    }
    let result = match args.as_slice() {
        [] if !legacy => repl(),
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        [flag, source] if flag == "--check" && !legacy => {
            read_source(source).and_then(|source| plenty::check_source(&source))
        }
        [flag, source, option, output]
            if flag == "--compile" && (option == "-o" || option == "--output") =>
        {
            read_source(source).and_then(|source| {
                if legacy {
                    plenty::compile_legacy_source_to_executable(&source, Path::new(output))
                } else {
                    plenty::compile_source_to_executable(&source, Path::new(output))
                }
            })
        }
        [source] if !source.starts_with('-') => read_source(source).and_then(|source| {
            let mut vm = Vm::new();
            if legacy {
                vm.run_legacy(&source)
            } else {
                vm.run(&source)
            }
        }),
        _ => Err(format!("unrecognised arguments\n{USAGE}").into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn read_source(path: &str) -> Result<String, Box<dyn Error>> {
    std::fs::read_to_string(path).map_err(|error| format!("reading {path}: {error}").into())
}

fn repl() -> Result<(), Box<dyn Error>> {
    println!("Plenty — typed expressions, Cranelift native compilation");
    println!("Enter submits expressions. A blank line finishes a block. Ctrl-J force-submits.");
    println!("Ctrl-G edits in $EDITOR. Tab completes. `quit` or Ctrl-D exits.");
    let mut vm = Vm::new();
    let mut editor: Editor<PlentyHelper, _> = Editor::new()?;
    editor.set_helper(Some(PlentyHelper {
        functions: Vec::new(),
    }));
    let trigger = EditorTrigger::default();
    editor.bind_sequence(
        KeyEvent::ctrl('G'),
        EventHandler::Conditional(Box::new(trigger.clone())),
    );
    editor.bind_sequence(KeyEvent::ctrl('J'), EventHandler::Simple(Cmd::AcceptLine));
    for modifier in [Modifiers::SHIFT, Modifiers::ALT] {
        editor.bind_sequence(
            KeyEvent(KeyCode::Enter, modifier),
            EventHandler::Simple(Cmd::AcceptLine),
        );
    }
    loop {
        if let Some(helper) = editor.helper_mut() {
            helper.functions = vm.function_names().into_iter().map(str::to_owned).collect();
        }
        let raw = match editor.readline(">>> ") {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(error) => return Err(error.into()),
        };
        let source = if trigger.0.swap(false, Ordering::Relaxed) {
            match open_in_editor(&raw) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("editor: {error}");
                    continue;
                }
            }
        } else {
            raw
        };
        if source.trim().is_empty() {
            continue;
        }
        if matches!(source.trim(), "quit" | "exit" | "q") {
            break;
        }
        editor.add_history_entry(source.as_str())?;
        match vm.eval(&source) {
            Ok(()) => {
                let value = vm.stack_repr();
                if value != "[]" {
                    println!("{}", &value[1..value.len() - 1]);
                }
            }
            Err(error) => eprintln!("error: {error}"),
        }
    }
    Ok(())
}

fn open_in_editor(initial: &str) -> std::io::Result<String> {
    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| OsString::from(if cfg!(windows) { "notepad" } else { "vi" }));
    let path = std::env::temp_dir().join(format!("plenty-{}.plenty", std::process::id()));
    std::fs::write(&path, initial)?;
    let result = (|| {
        let status = Command::new(&editor).arg(&path).status()?;
        if !status.success() {
            return Err(std::io::Error::other(format!(
                "{} exited with {status}",
                editor.to_string_lossy()
            )));
        }
        std::fs::read_to_string(&path)
    })();
    let _ = std::fs::remove_file(&path);
    result
}
