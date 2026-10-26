//! Executable size budgets and per-module linking (issue #502). Programs link
//! the runtime with `--gc-sections`, and each standard module reaches its
//! native code only through its own entry symbols, so a program carries only
//! the native code it calls. These tests fail when a change breaks that.
#![cfg(target_os = "linux")]

mod support;

use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};
use std::ops::Range;

const EMPTY: &str = "def main() -> ():\n    pass\n";
const HELLO: &str =
    "def main() -> Result[(), AllocError | IoError]:\n    print(\"hello\")?\n    Ok(())\n";

/// Largest stripped file and `.text` section, in bytes, for small programs,
/// as `(name, source, file budget, .text budget)`.
///
/// Measured on 2026-10-26 at `89e5f826` on x86-64 Linux. Release and test
/// compilers give the same executables, since the runtime archive is always
/// built at opt-level 2. The `runtime-checks` feature, which CI's
/// `--all-features` suite enables, links a runtime with allocation accounting:
///
/// | Program | File | `.text` | File, checked | `.text`, checked |
/// |---|---:|---:|---:|---:|
/// | empty | 47,648 | 21,153 | 53,208 | 25,089 |
/// | hello | 113,544 | 69,137 | 122,064 | 75,953 |
///
/// Each budget allows about 13% growth. Raise one only as a deliberate,
/// reviewed change that records the new measurement here and explains it.
#[cfg(not(feature = "runtime-checks"))]
const BUDGETS: [(&str, &str, u64, u64); 2] = [
    ("empty", EMPTY, 54_000, 24_000),
    ("hello", HELLO, 128_000, 78_000),
];
#[cfg(feature = "runtime-checks")]
const BUDGETS: [(&str, &str, u64, u64); 2] = [
    ("empty", EMPTY, 60_000, 28_500),
    ("hello", HELLO, 138_000, 86_000),
];

#[test]
fn small_programs_stay_within_size_budgets() {
    let mut over = Vec::new();
    for (name, source, file_budget, text_budget) in BUDGETS {
        let (_dir, executable) = support::build_file(source);
        let data = std::fs::read(&executable).unwrap();
        let file = object::File::parse(&*data).unwrap();
        let text = file.section_by_name(".text").unwrap().size();
        let size = data.len() as u64;
        eprintln!(
            "{name}: file {size} (budget {file_budget}), .text {text} (budget {text_budget})"
        );
        if size > file_budget {
            over.push(format!("{name}: file {size} bytes > budget {file_budget}"));
        }
        if text > text_budget {
            over.push(format!("{name}: .text {text} bytes > budget {text_budget}"));
        }
    }
    assert!(
        over.is_empty(),
        "{}\nsee BUDGETS in tests/test_executable_size.rs",
        over.join("\n")
    );
}

/// Evidence that a module's native code is linked.
enum Pattern {
    /// Runtime operation entries `plenty_op_<N>` for these `N`.
    Operations(Range<u32>),
    /// A runtime entry symbol, such as a facility dispatcher.
    Entry(&'static str),
    /// Part of every symbol name of a vendored library.
    Library(&'static str),
}

impl Pattern {
    fn matches(&self, symbol: &str) -> bool {
        match self {
            Pattern::Operations(range) => support::operation_in(symbol, range.clone()),
            Pattern::Entry(name) => symbol == *name,
            Pattern::Library(part) => symbol.contains(part),
        }
    }

    fn describe(&self) -> String {
        match self {
            Pattern::Operations(range) => format!("plenty_op_{}..{}", range.start, range.end),
            Pattern::Entry(name) | Pattern::Library(name) => (*name).to_owned(),
        }
    }
}

/// A standard module with runtime native code, a program that uses it, and
/// the symbols that prove its code is linked. A new native wrapper adds a row
/// (see the native-wrapper checklist in
/// book/src/proposals/standard-library-scope.md).
struct NativeModule {
    module: &'static str,
    program: &'static str,
    patterns: &'static [Pattern],
}

const NATIVE_MODULES: &[NativeModule] = &[
    NativeModule {
        module: "std.cli.progress",
        program: r#"import std.cli.progress as progress
from std.cli.progress import ProgressError
def main() -> Result[(), ProgressError]:
    mut bar = progress.bar(1u64)
    bar.advance()?
    bar.finish()?
    Ok(())
"#,
        patterns: &[Pattern::Operations(511..512)],
    },
    NativeModule {
        module: "std.crypto",
        program: r#"import std.bytes
import std.crypto
from std.bytes import BytesError
from std.crypto import CryptoError

def main() -> Result[(), CryptoError | BytesError | AllocError | IoError]:
    print(std.crypto.sha256(&std.bytes.empty()?)?.len())?
    Ok(())
"#,
        // sha2's block function.
        patterns: &[
            Pattern::Operations(300..320),
            Pattern::Library("compress256"),
        ],
    },
    NativeModule {
        module: "std.compress",
        program: r#"import std.bytes
import std.compress
from std.bytes import BytesError
from std.compress import CompressError, Format

def main() -> Result[(), CompressError | BytesError | AllocError | IoError]:
    print(std.compress.compress(Format.Gzip, &std.bytes.empty()?, 6)?.len())?
    Ok(())
"#,
        patterns: &[Pattern::Operations(320..340), Pattern::Library("zlib_rs")],
    },
    NativeModule {
        module: "std.db.sqlite",
        program: r#"import std.db.sqlite
from std.db.sqlite import DbError

def main() -> Result[(), DbError]:
    mut db = std.db.sqlite.memory(4096)?
    db.execute("SELECT 1")?
    Ok(())
"#,
        patterns: &[Pattern::Operations(440..470), Pattern::Library("sqlite3_")],
    },
    NativeModule {
        module: "std.tls",
        program: r#"import std.tls
from std.tls import TlsError

def main() -> Result[(), TlsError]:
    roots = std.tls.system_roots()?
    _config = std.tls.client_config(&roots)?
    Ok(())
"#,
        patterns: &[
            Pattern::Operations(470..500),
            Pattern::Library("rustls"),
            Pattern::Library("ring_core_"),
        ],
    },
    NativeModule {
        module: "std.net",
        program: r#"import std.net
from std.net import NetError

def main() -> Result[(), NetError]:
    _listener = std.net.listen(&std.net.loopback_v4(0u16), 1)?
    Ok(())
"#,
        patterns: &[Pattern::Operations(410..440)],
    },
    NativeModule {
        module: "std.process",
        program: r#"import std.process

def main() -> Result[(), IoError]:
    _child = std.process.command("true")?.spawn()?
    Ok(())
"#,
        patterns: &[Pattern::Operations(193..200), Pattern::Operations(228..230)],
    },
    NativeModule {
        module: "std.fs",
        program: r#"import std.fs
import std.path

def main() -> Result[(), IoError]:
    _metadata = std.fs.metadata(&std.path.from_text(".")?)?
    Ok(())
"#,
        patterns: &[Pattern::Operations(150..165)],
    },
    NativeModule {
        module: "std.fs.watch",
        program: r#"import std.fs.watch
def main() -> Result[(), std.fs.watch.WatchError]:
    _watcher = std.fs.watch.watcher()?
    Ok(())
"#,
        patterns: &[Pattern::Operations(512..515)],
    },
    NativeModule {
        module: "std.regex",
        program: r#"import std.regex
from std.regex import RegexError

def main() -> Result[(), RegexError | AllocError | IoError]:
    pattern = std.regex.compile("\\w+", &std.regex.limits())?
    print(pattern.is_match("hi")?)?
    Ok(())
"#,
        patterns: &[Pattern::Operations(360..366)],
    },
    NativeModule {
        module: "std.signal",
        program: r#"import std.signal
from std.signal import SignalError

def main() -> Result[(), SignalError]:
    _signals = std.signal.listen()?
    Ok(())
"#,
        patterns: &[Pattern::Operations(500..501)],
    },
    NativeModule {
        module: "std.time",
        program: r#"import std.time

def main() -> ():
    _now = std.time.monotonic_now()
"#,
        patterns: &[Pattern::Operations(184..188)],
    },
    NativeModule {
        module: "std.sync",
        program: r#"from std.sync import CancellationToken

def main() -> Result[(), AllocError]:
    token = CancellationToken()?
    token.cancel()
    Ok(())
"#,
        patterns: &[Pattern::Entry("plenty_control")],
    },
    NativeModule {
        module: "std.concurrent",
        program: r#"from std.concurrent import PoolError, ThreadPoolExecutor

def main() -> Result[(), PoolError]:
    with ThreadPoolExecutor(1, 1)? as pool:
        pool.shutdown()
    Ok(())
"#,
        patterns: &[Pattern::Entry("plenty_executor")],
    },
    NativeModule {
        module: "std.channel",
        program: r#"from std.channel import ChannelError, channel

def main() -> Result[(), ChannelError]:
    _sender, _receiver = channel[i64](2)?
    Ok(())
"#,
        patterns: &[Pattern::Entry("plenty_channel")],
    },
];

/// Function symbols of `source` compiled with its symbol table. Functions
/// only: the linker also keeps a file symbol for every archive member it
/// opened, and mold names each GOT entry `NAME$got`.
fn functions(source: &str) -> Vec<String> {
    let (_dir, executable) = support::build_unstripped(source);
    let data = std::fs::read(&executable).unwrap();
    let file = object::File::parse(&*data).unwrap();
    file.symbols()
        .filter(|symbol| symbol.kind() == SymbolKind::Text)
        .filter_map(|symbol| symbol.name().ok().map(str::to_owned))
        .collect()
}

/// For each module whose symbols `functions` contains, the module and up to
/// three of those symbols.
fn linked_modules(functions: &[String]) -> Vec<(&'static str, Vec<&str>)> {
    NATIVE_MODULES
        .iter()
        .filter_map(|row| {
            let found: Vec<_> = functions
                .iter()
                .filter(|name| row.patterns.iter().any(|pattern| pattern.matches(name)))
                .take(3)
                .map(String::as_str)
                .collect();
            (!found.is_empty()).then_some((row.module, found))
        })
        .collect()
}

/// Hello links no module's native code, and each module's program links its
/// own: every pattern in its row matches at least one function.
#[test]
fn programs_link_only_the_native_modules_they_call() {
    let hello = functions(HELLO);
    let mut failures: Vec<_> = linked_modules(&hello)
        .into_iter()
        .map(|(module, symbols)| format!("hello links {module} {symbols:?}"))
        .collect();
    for row in NATIVE_MODULES {
        let linked = functions(row.program);
        for pattern in row.patterns {
            if !linked.iter().any(|name| pattern.matches(name)) {
                failures.push(format!(
                    "{} program links no {}",
                    row.module,
                    pattern.describe()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Importing a module, even one that imports native-backed modules, links
/// none of their code: only calls do.
#[test]
fn importing_a_module_links_no_native_code() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("stdlib");
    let mut modules = Vec::new();
    let mut pending = vec![root.join("std")];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "plenty") {
                let name = path.strip_prefix(&root).unwrap().with_extension("");
                modules.push(name.to_str().unwrap().replace('/', "."));
            }
        }
    }
    modules.sort();
    assert!(modules.len() > 30 && modules.contains(&"std.tls".to_owned()));
    let mut found = Vec::new();
    for module in &modules {
        let linked = functions(&format!("import {module}\n{HELLO}"));
        for (native, symbols) in linked_modules(&linked) {
            found.push((module.as_str(), native, format!("{symbols:?}")));
        }
    }
    let unexpected: Vec<_> = found
        .iter()
        .map(|(module, native, symbols)| format!("importing {module} links {native} {symbols}"))
        .collect();
    assert!(unexpected.is_empty(), "{}", unexpected.join("\n"));
}

/// A standard destructor links only when a live body arms its class (issue
/// #506). Naming the type in a live signature emits its descriptor, without
/// the hook, since no instance can exist.
#[test]
fn a_standard_destructor_links_only_when_its_class_is_constructed() {
    const DESTRUCTOR: &str = "__plenty_class_std.tempfile.TemporaryDirectory.__del__";
    const REMOVAL: &str = "std.fs.delete_directory_contents";
    let has = |functions: &[String], part: &str| functions.iter().any(|name| name.contains(part));
    let named = functions(&format!(
        "import std.tempfile\ndef take(directory: std.tempfile.TemporaryDirectory) -> ():\n    drop(directory)\n{HELLO}"
    ));
    assert!(
        !has(&named, DESTRUCTOR) && !has(&named, REMOVAL),
        "{named:?}"
    );
    assert!(
        linked_modules(&named).is_empty(),
        "{:?}",
        linked_modules(&named)
    );
    let constructed = functions(
        "import std.tempfile\nimport std.fs\ndef main() -> Result[(), std.fs.FsError]:\n    directory = std.tempfile.directory()?\n    drop(directory)\n    Ok(())\n",
    );
    assert!(has(&constructed, DESTRUCTOR) && has(&constructed, REMOVAL));
}
