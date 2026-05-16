//! The Plenty REPL.
//!
//! Line editing is delegated to `rustyline`: pure-Rust, cross-platform,
//! Emacs-style bindings (ctrl-a/e for line ends, ctrl-p/n for history,
//! ctrl-l to clear, ctrl-r reverse-search). On top of that, this file
//! adds three things:
//!
//! * **Multi-line input.** Enter always inserts a newline; the buffer is
//!   submitted only when a function definition closes with a balanced `;`,
//!   or when the user presses a force-submit key.
//! * **Force-submit keys.** Shift-Enter, Alt-Enter, and Ctrl-J all bypass
//!   the validator. The first two depend on the terminal sending a
//!   distinguishable sequence (modern terminals usually do); Ctrl-J always
//!   works because it is the literal LF byte.
//! * **Ctrl-G to edit in `$EDITOR`.** The current buffer is written to a
//!   tempfile, `$EDITOR` (or `$VISUAL`) is spawned on it, and the saved
//!   content is what gets run. Useful for composing a long definition or
//!   recovering one fished out of history.

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
    Cmd, ConditionalEventHandler, Context, Editor, Event, EventContext, EventHandler, KeyCode,
    KeyEvent, Modifiers, RepeatCount,
};
use rustyline::{Helper, Highlighter, Hinter};

const BANNER: &str = r#"
:::::::::  :::        :::::::::: ::::    ::: ::::::::::: :::   :::
:+:    :+: :+:        :+:        :+:+:   :+:     :+:     :+:   :+:
+:+    +:+ +:+        +:+        :+:+:+  +:+     +:+      +:+ +:+
+#++:++#+  +#+        +#++:++#   +#+ +:+ +#+     +#+       +#++:
+#+        +#+        +#+        +#+  +#+#+#     +#+        +#+
#+#        #+#        #+#        #+#   #+#+#     #+#        #+#
###        ########## ########## ###    ####     ###        ###
"#;

const HELP: &str = "\
Enter wraps. `;` (after a balanced `:`) submits. Ctrl-J (or Shift/Alt-Enter)
force-submits. Ctrl-G edits the buffer in $EDITOR. Tab completes function
names and builtins. `quit` or Ctrl-D exits.
";

const PROMPT: &str = "---> ";

/// Words to offer for tab completion that are *not* in the runtime
/// function dictionary — builtins, operators, keywords, type names.
const STATIC_WORDS: &[&str] = &[
    "true", "false", "match", "end", "not", "Int", "Str", "Bool", ".", "+", "-", "*", "/", "=",
    "<", ">", ":clear", ":listdir", "exit", "quit",
];

#[derive(Helper, Highlighter, Hinter)]
struct PlentyHelper {
    /// Function names from the VM dictionary, refreshed before each
    /// `readline` so newly-defined functions show up in completion.
    fn_names: Vec<String>,
}

impl Validator for PlentyHelper {
    /// Submit only when the input is *structurally* complete — empty,
    /// or all `:` definitions closed by `;`. Anything inside an open
    /// `:` or an unterminated `"..."` keeps editing. A force-submit key
    /// (Ctrl-J etc.) bypasses this entirely via `Cmd::AcceptLine`.
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        let input = ctx.input();
        if input.trim().is_empty() {
            return Ok(ValidationResult::Valid(None));
        }
        let depth = match definition_depth(input) {
            Some(d) => d,
            // Mid-string: definitely not done.
            None => return Ok(ValidationResult::Incomplete),
        };
        if depth > 0 {
            return Ok(ValidationResult::Incomplete);
        }
        if input.trim_end().ends_with(';') {
            return Ok(ValidationResult::Valid(None));
        }
        Ok(ValidationResult::Incomplete)
    }
}

impl Completer for PlentyHelper {
    type Candidate = Pair;

    /// Complete the word immediately before the cursor. A leading `:`
    /// flips us into "function call" mode and we only offer dictionary
    /// names (and the `:clear`/`:listdir` builtins). Otherwise we offer
    /// the static word list. We only consider the word the cursor sits
    /// in; everything left of the previous whitespace is preserved.
    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let start = line[..pos]
            .rfind(char::is_whitespace)
            .map(|i| i + 1)
            .unwrap_or(0);
        let prefix = &line[start..pos];

        let mut out: Vec<Pair> = Vec::new();
        if let Some(rest) = prefix.strip_prefix(':') {
            for name in &self.fn_names {
                if name.starts_with(rest) {
                    let s = format!(":{name}");
                    out.push(Pair { display: s.clone(), replacement: s });
                }
            }
            for w in STATIC_WORDS.iter().filter(|w| w.starts_with(':')) {
                if w[1..].starts_with(rest) {
                    out.push(Pair { display: (*w).into(), replacement: (*w).into() });
                }
            }
        } else if !prefix.is_empty() {
            for w in STATIC_WORDS.iter().filter(|w| !w.starts_with(':')) {
                if w.starts_with(prefix) {
                    out.push(Pair { display: (*w).into(), replacement: (*w).into() });
                }
            }
        }
        Ok((start, out))
    }
}

/// Shared flag set when the user presses Ctrl-G. The event handler runs
/// inside rustyline's input loop and cannot itself spawn `$EDITOR` (the
/// terminal is in raw mode); instead it flips the flag and force-submits
/// the buffer, and the main loop — back in cooked mode — handles the
/// editor invocation.
#[derive(Clone, Default)]
struct EditorTrigger(Arc<AtomicBool>);

impl ConditionalEventHandler for EditorTrigger {
    fn handle(&self, _: &Event, _: RepeatCount, _: bool, _: &EventContext) -> Option<Cmd> {
        self.0.store(true, Ordering::Relaxed);
        Some(Cmd::AcceptLine)
    }
}

const USAGE: &str = "\
Usage: plenty [FILE]
       plenty --compile FILE -o OUT.o
       plenty -h | --help

With no arguments, starts the interactive REPL. With a file path, lexes,
compiles, type-checks, and runs the file, then exits — stdout is the
program's, stderr is for diagnostics. Exit status is 0 on success and
non-zero on any compile, type, or runtime error.

`--compile FILE -o OUT.o` lowers FILE to a native object file at OUT.o
(AOT, §11.1). Link it with the runtime C file shipped at
`runtime/plenty_runtime.c` to produce an executable, e.g.
    cc OUT.o runtime/plenty_runtime.c -o myprog
Phase c.1 supports integer-only top-level programs; functions, `match`,
and strings are not yet lowered — those programs still run under the
interpreter.
";

fn main() -> ExitCode {
    pretty_env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.as_slice() {
        [] => repl(),
        [flag] if flag == "-h" || flag == "--help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        [flag, source, dash_o, out]
            if flag == "--compile" && (dash_o == "-o" || dash_o == "--output") =>
        {
            compile_file(Path::new(source), Path::new(out))
        }
        [path] if !path.starts_with('-') => run_file(Path::new(path)),
        _ => {
            eprintln!("plenty: unrecognised arguments");
            eprint!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Read `path` as a single Plenty source and run it on a fresh [`Vm`].
/// Used by the binary's file-execution mode (DESIGN.md §12.4); the REPL
/// uses [`Vm::run`] directly so its state persists across inputs.
fn run_file(path: &Path) -> Result<(), Box<dyn Error>> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| -> Box<dyn Error> { format!("reading {}: {e}", path.display()).into() })?;
    let mut vm = Vm::new();
    vm.run(&source)
}

/// Read `source` and emit a native object file at `output` (DESIGN.md
/// §11.1, §12.3 — phase c.1). The user is responsible for linking the
/// result with the C runtime to produce an executable.
fn compile_file(source: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let text = std::fs::read_to_string(source)
        .map_err(|e| -> Box<dyn Error> { format!("reading {}: {e}", source.display()).into() })?;
    plenty::compile_source_to_object(&text, output)
}

fn repl() -> Result<(), Box<dyn Error>> {
    println!("{BANNER}");
    println!("{HELP}");

    let mut vm = Vm::new();
    let mut rl: Editor<PlentyHelper, _> = Editor::new()?;
    rl.set_helper(Some(PlentyHelper { fn_names: Vec::new() }));

    let editor_trigger = EditorTrigger::default();
    rl.bind_sequence(
        KeyEvent::ctrl('G'),
        EventHandler::Conditional(Box::new(editor_trigger.clone())),
    );
    // Force-submit keys. Ctrl-J is the universal one (it is the literal
    // LF byte; every terminal emits it for Ctrl-J). Shift-Enter and
    // Alt-Enter need terminal cooperation — kitty/wezterm/iTerm2 send
    // distinct sequences; xterm needs modifyOtherKeys=2.
    rl.bind_sequence(KeyEvent::ctrl('J'), EventHandler::Simple(Cmd::AcceptLine));
    rl.bind_sequence(
        KeyEvent(KeyCode::Enter, Modifiers::SHIFT),
        EventHandler::Simple(Cmd::AcceptLine),
    );
    rl.bind_sequence(
        KeyEvent(KeyCode::Enter, Modifiers::ALT),
        EventHandler::Simple(Cmd::AcceptLine),
    );

    loop {
        // Refresh the completer's view of the dictionary so functions
        // defined since the last prompt show up under Tab.
        if let Some(h) = rl.helper_mut() {
            h.fn_names = vm.function_names().into_iter().map(str::to_string).collect();
        }

        let raw = match rl.readline(PROMPT) {
            Ok(line) => line,
            // Ctrl-C: drop the current line, keep the session — matches
            // Python and most other REPLs. Quitting on a single Ctrl-C
            // would be a surprise.
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("readline error: {e}");
                break;
            }
        };

        let source = if editor_trigger.0.swap(false, Ordering::Relaxed) {
            match open_in_editor(&raw) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("editor: {e}");
                    continue;
                }
            }
        } else {
            raw
        };

        let trimmed = source.trim();
        if trimmed.is_empty() {
            continue;
        }
        if matches!(trimmed, "exit" | "q" | "quit") {
            break;
        }
        rl.add_history_entry(source.as_str())?;
        if let Err(e) = vm.run(&source) {
            eprintln!("error: {e}");
        }
    }
    Ok(())
}

/// Count `:` definition-openers minus `;` closers in `input`, ignoring
/// any inside a `"..."` literal. Returns `None` if the input ends mid
/// string literal, since the buffer is then known-incomplete regardless
/// of bracket depth.
///
/// This is a structural check, not a full parse — it does not validate
/// that `:` is followed by a name, or that the closing `;` is in a sane
/// place. The compiler catches those when the buffer is submitted.
fn definition_depth(input: &str) -> Option<i32> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut depth = 0i32;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'"' {
            i += 1;
            loop {
                if i >= bytes.len() {
                    return None;
                }
                match bytes[i] {
                    b'\\' => {
                        if i + 1 >= bytes.len() {
                            return None;
                        }
                        i += 2;
                    }
                    b'"' => {
                        i += 1;
                        break;
                    }
                    _ => i += 1,
                }
            }
            continue;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'"' {
            i += 1;
        }
        match &input[start..i] {
            ":" => depth += 1,
            ";" => depth -= 1,
            _ => {}
        }
    }
    Some(depth)
}

/// Open `initial` in `$EDITOR` (or `$VISUAL`, or a platform default),
/// wait for the editor to exit, and return whatever was saved. The
/// tempfile is named `.plenty` so an editor with syntax-aware modes can
/// pick the right one if you ever add a Plenty mode.
fn open_in_editor(initial: &str) -> std::io::Result<String> {
    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| OsString::from(if cfg!(windows) { "notepad" } else { "vi" }));

    let path = std::env::temp_dir().join(format!("plenty-{}.plenty", std::process::id()));
    std::fs::write(&path, initial)?;
    let status = Command::new(&editor).arg(&path).status()?;
    let edited = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "{} exited with {}",
            editor.to_string_lossy(),
            status
        )));
    }
    edited
}
