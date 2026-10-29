//! `plenty lsp` driven over stdio with scripted messages (issue #155).

mod support;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use support::write_file as write;

struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next: u64,
    /// Diagnostics published so far, by document URI.
    diagnostics: Vec<(String, Value)>,
}

impl Client {
    fn start(cache: &Path) -> Client {
        Self::start_with_encodings(cache, json!([]))
    }

    fn start_with_encodings(cache: &Path, encodings: Value) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .arg("lsp")
            .env("PLENTY_CACHE_DIR", cache)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut client = Client {
            child,
            input,
            output,
            next: 0,
            diagnostics: vec![],
        };
        let result = client.request(
            "initialize",
            json!({"capabilities": {
                "general": {"positionEncodings": encodings},
                "textDocument": {"inlayHint": {"dynamicRegistration": false}},
            }}),
        );
        assert_eq!(
            result["capabilities"]["inlayHintProvider"],
            json!({"resolveProvider": false})
        );
        assert_eq!(
            result["capabilities"]["positionEncoding"],
            encodings
                .as_array()
                .and_then(|e| e.first())
                .cloned()
                .unwrap_or(json!("utf-16"))
        );
        assert_eq!(result["capabilities"]["hoverProvider"], true);
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, message: Value) {
        self.send_raw(&message.to_string());
    }

    fn send_raw(&mut self, body: &str) {
        write!(self.input, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.input.flush().unwrap();
    }

    fn receive(&mut self) -> Value {
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "server exited"
            );
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        self.output.read_exact(&mut body).unwrap();
        let message: Value = serde_json::from_slice(&body).unwrap();
        if message["method"] == "textDocument/publishDiagnostics" {
            let params = &message["params"];
            self.diagnostics.push((
                params["uri"].as_str().unwrap().to_owned(),
                params["diagnostics"].clone(),
            ));
        }
        message
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self.receive();
            if message["id"] == id {
                assert!(message.get("error").is_none(), "{message}");
                return message["result"].clone();
            }
        }
    }

    /// Open a document and return the diagnostics published for it.
    fn open(&mut self, path: &Path) -> Value {
        let text = std::fs::read_to_string(path).unwrap();
        let uri = uri(path);
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "plenty", "version": 1, "text": text}}),
        );
        self.published(&uri)
    }

    fn published(&mut self, uri: &str) -> Value {
        loop {
            self.receive();
            if let Some(index) = self.diagnostics.iter().position(|(u, _)| u == uri) {
                return self.diagnostics.remove(index).1;
            }
        }
    }

    fn at(&mut self, method: &str, path: &Path, line: u32, character: u32) -> Value {
        self.request(
            method,
            json!({"textDocument": {"uri": uri(path)}, "position": {"line": line, "character": character}}),
        )
    }

    fn hover(&mut self, path: &Path, line: u32, character: u32) -> String {
        let result = self.at("textDocument/hover", path, line, character);
        result["contents"]["value"]
            .as_str()
            .unwrap_or_else(|| panic!("no hover at {line}:{character}: {result}"))
            .to_owned()
    }

    /// The definition's file path and 0-based line and character.
    fn definition(&mut self, path: &Path, line: u32, character: u32) -> (String, u64, u64) {
        let result = self.at("textDocument/definition", path, line, character);
        let uri = result["uri"]
            .as_str()
            .unwrap_or_else(|| panic!("no definition at {line}:{character}: {result}"));
        let start = &result["range"]["start"];
        (
            uri.strip_prefix("file://").unwrap().to_owned(),
            start["line"].as_u64().unwrap(),
            start["character"].as_u64().unwrap(),
        )
    }

    fn completion(&mut self, path: &Path, line: u32, character: u32) -> Vec<String> {
        let result = self.at("textDocument/completion", path, line, character);
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["label"].as_str().unwrap().to_owned())
            .collect()
    }

    fn exit(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn a_body_that_is_not_json_gets_a_parse_error_and_the_session_continues() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("cache"));
    client.send_raw("{not json");
    let error = client.receive();
    assert_eq!(error["id"], Value::Null, "{error}");
    assert_eq!(error["error"]["code"], -32700, "{error}");
    client.exit();
}

#[test]
fn a_request_after_shutdown_gets_an_invalid_request_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("cache"));
    client.request("shutdown", Value::Null);
    client.send(json!({"jsonrpc": "2.0", "id": 99, "method": "initialize", "params": {}}));
    let error = client.receive();
    assert_eq!(error["id"], 99, "{error}");
    assert_eq!(error["error"]["code"], -32600, "{error}");
    client.notify("exit", Value::Null);
    assert!(client.child.wait().unwrap().success());
}

#[test]
fn a_header_without_content_length_ends_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("cache"));
    write!(client.input, "Content-Type: x\r\n\r\n{{}}").unwrap();
    client.input.flush().unwrap();
    assert!(client.child.wait().unwrap().success());
}

fn uri(path: &Path) -> String {
    format!("file://{}", path.canonicalize().unwrap().display())
}

const MAIN: &str = r#"from std.math import gcd
from shapes import Point, area

enum Color:
    Red
    Green(i64)

def broken() -> i64:
    "Returns the wrong type."
    "text"

def scale(p: &Point, by: i64) -> i64:
    """Scale a point's x.

    Multiplies by `by`."""
    p.x * by

def main() -> ():
    p = Point(3, 4)
    s = scale(&p, 2)
    d = p.dist()
    c = Color.Red
    g = gcd(6u64, 4u64)
"#;

const SHAPES: &str = r#"pub class Point:
    pub x: i64
    pub y: i64

    pub def dist(self) -> i64:
        "Taxicab distance from the origin."
        self.x + self.y

pub def area(p: &Point) -> i64:
    p.x * p.y
"#;

#[test]
fn hover_definition_and_diagnostics_in_a_file_with_an_error_elsewhere() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    let shapes = dir.path().join("shapes.plenty");
    write(&main, MAIN);
    write(&shapes, SHAPES);
    let mut client = Client::start(&dir.path().join("cache"));
    let diagnostics = client.open(&main);
    let diagnostics = diagnostics.as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 9);
    assert_eq!(diagnostics[0]["message"], "expected i64, got str");

    // A local function: signature with its docstring.
    let hover = client.hover(&main, 19, 8);
    assert!(
        hover.starts_with("```plenty\ndef scale(p: &Point, by: i64) -> i64\n```"),
        "{hover}"
    );
    assert!(
        hover.contains("Scale a point's x.\n\nMultiplies by `by`."),
        "{hover}"
    );
    assert_eq!(client.definition(&main, 19, 8).1, 11);

    // A method resolved through the receiver's type, in another module.
    let hover = client.hover(&main, 20, 10);
    assert!(hover.contains("def dist(self) -> i64"), "{hover}");
    assert!(
        hover.contains("Taxicab distance from the origin."),
        "{hover}"
    );
    let (file, line, character) = client.definition(&main, 20, 10);
    assert_eq!(Path::new(&file), shapes.canonicalize().unwrap());
    assert_eq!((line, character), (4, 12));

    // A field, a parameter, a local with its type, an imported class.
    assert!(client.hover(&main, 15, 6).contains("pub x: i64"));
    assert!(client.hover(&main, 15, 4).contains("p: &shapes.Point"));
    assert_eq!(client.definition(&main, 15, 4), (file_of(&main), 11, 10));
    assert!(client.hover(&main, 18, 4).contains("p: shapes.Point"));
    assert!(client.hover(&main, 18, 8).contains("pub class Point"));
    assert_eq!(client.definition(&main, 1, 21).1, 0);

    // An enum variant.
    let hover = client.hover(&main, 21, 14);
    assert!(
        hover.contains("Red") && hover.contains("Color.Red"),
        "{hover}"
    );
    assert_eq!(client.definition(&main, 21, 14).1, 4);

    // An embedded std function opens as a read-only copy of its source.
    let hover = client.hover(&main, 22, 8);
    assert!(hover.contains("def gcd(a: u64, b: u64) -> u64"), "{hover}");
    let (file, line, _) = client.definition(&main, 22, 8);
    assert!(
        file.ends_with(concat!(
            "/std-sources/",
            env!("CARGO_PKG_VERSION"),
            "/std/math.plenty"
        )),
        "{file}"
    );
    let copy = std::fs::read_to_string(&file).unwrap();
    assert!(copy.lines().nth(line as usize).unwrap().contains("def gcd"));
    assert!(std::fs::metadata(&file).unwrap().permissions().readonly());
    client.exit();
}

fn file_of(path: &Path) -> String {
    path.canonicalize().unwrap().display().to_string()
}

#[test]
fn completion_and_changes() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(&main, MAIN);
    write(&dir.path().join("shapes.plenty"), SHAPES);
    let mut client = Client::start(&dir.path().join("cache"));
    client.open(&main);

    let members = client.completion(&main, 20, 10);
    for expected in ["dist", "x", "y"] {
        assert!(members.contains(&expected.to_owned()), "{members:?}");
    }
    let variants = client.completion(&main, 21, 14);
    assert!(variants.contains(&"Red".to_owned()) && variants.contains(&"Green".to_owned()));
    let names = client.completion(&main, 22, 4);
    for expected in ["scale", "Point", "gcd", "p", "s", "Color", "while"] {
        assert!(
            names.contains(&expected.to_owned()),
            "{expected}: {names:?}"
        );
    }
    // Locals of another function are not offered.
    assert!(!names.contains(&"by".to_owned()), "{names:?}");

    // An edit fixes the error and adds a call; the unsaved text is checked.
    let fixed = MAIN.replace("    \"text\"\n", "    1\n") + "    a = area(&p)\n";
    let uri = uri(&main);
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": fixed}]}),
    );
    assert_eq!(client.published(&uri), json!([]));
    let hover = client.hover(&main, 23, 8);
    assert!(hover.contains("pub def area(p: &Point) -> i64"), "{hover}");
    client.exit();
}

#[test]
fn a_dependency_of_a_package_resolves() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = |name: &str, dependencies: &str| {
        format!(
            "manifest-version = 1\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[dependencies]\n{dependencies}"
        )
    };
    write(&dir.path().join("geo/plenty.toml"), &manifest("geo", ""));
    write(&dir.path().join("geo/src/lib.plenty"), SHAPES);
    write(
        &dir.path().join("app/plenty.toml"),
        &manifest("app", "geo = { path = \"../geo\" }\n"),
    );
    let main = dir.path().join("app/src/main.plenty");
    write(
        &main,
        "import geo\n\ndef main() -> ():\n    p = geo.Point(1, 2)\n    n = geo.area(&p)\n",
    );
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(client.open(&main), json!([]));
    let hover = client.hover(&main, 4, 12);
    assert!(hover.contains("pub def area(p: &Point) -> i64"), "{hover}");
    let (file, line, _) = client.definition(&main, 4, 12);
    assert_eq!(
        Path::new(&file),
        dir.path()
            .join("geo/src/lib.plenty")
            .canonicalize()
            .unwrap()
    );
    assert_eq!(line, 8);
    let members = client.completion(&main, 4, 12);
    assert!(members.contains(&"area".to_owned()), "{members:?}");
    client.exit();
}

/// The Neovim configuration in `editors/nvim`, through Neovim's built-in
/// client, when `nvim` is installed.
#[test]
fn neovim_uses_the_server() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let available = Command::new("nvim")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        eprintln!("skipped: nvim is not installed");
        return;
    }
    let output = Command::new("nvim")
        .args(["--headless", "-u", "NONE", "-i", "NONE", "-n", "-l"])
        .arg("editors/nvim/tests/lsp.lua")
        .current_dir(root)
        .env("PLENTY_BIN", env!("CARGO_BIN_EXE_plenty"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success() && text.contains("ok"), "{text}");
}

fn hints(client: &mut Client, path: &Path, start: (u32, u32), end: (u32, u32)) -> Value {
    client.request(
        "textDocument/inlayHint",
        json!({"textDocument": {"uri": uri(path)}, "range": {
            "start": {"line": start.0, "character": start.1},
            "end": {"line": end.0, "character": end.1},
        }}),
    )
}

fn type_hint(line: u32, character: u32, ty: &str) -> Value {
    json!({"position": {"line": line, "character": character},
        "label": format!(": {ty}"), "kind": 1, "paddingLeft": false, "paddingRight": false})
}

#[test]
fn inlay_hints_use_checked_simple_bindings_and_imports() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(&dir.path().join("shapes.plenty"), SHAPES);
    write(
        &main,
        r#"from shapes import Point
from std.math import gcd

def main() -> Result[(), AllocError]:
    count = 42
    mut small = 7u8
    small = 8u8
    ratio = 1.5
    ready = True
    text = "hello"
    data = b"hi"
    pair = (1, True)
    values = [1, 2]?
    borrowed = &values
    maybe = Some(3)
    point = Point(1, 2)
    divisor = gcd(6u64, 4u64)
    explicit: i64 = 9
    mut annotated: bool = False
    multiline = (
        1 + 2
    )
    Ok(())
"#,
    );
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(client.open(&main), json!([]));
    let expected = json!([
        type_hint(4, 9, "i64"),
        type_hint(5, 13, "u8"),
        type_hint(7, 9, "f64"),
        type_hint(8, 9, "bool"),
        type_hint(9, 8, "str"),
        type_hint(10, 8, "bytes"),
        type_hint(11, 8, "tuple[i64, bool]"),
        type_hint(12, 10, "list[i64]"),
        type_hint(13, 12, "&list[i64]"),
        type_hint(14, 9, "Option[i64]"),
        type_hint(15, 9, "shapes.Point"),
        type_hint(16, 11, "u64"),
        type_hint(19, 13, "i64"),
    ]);
    assert_eq!(hints(&mut client, &main, (0, 0), (30, 0)), expected);
    // A second request reuses the check (and does not publish diagnostics).
    let published = client.diagnostics.len();
    assert_eq!(hints(&mut client, &main, (0, 0), (30, 0)), expected);
    assert_eq!(client.diagnostics.len(), published);
    assert!(client.hover(&main, 12, 4).contains("values: list[i64]"));
    // Range starts are inclusive and ends exclusive, at the hint's position.
    assert_eq!(
        hints(&mut client, &main, (4, 9), (5, 13)),
        json!([type_hint(4, 9, "i64")])
    );
    assert_eq!(hints(&mut client, &main, (4, 10), (5, 13)), json!([]));
    assert_eq!(hints(&mut client, &main, (4, 9), (4, 9)), json!([]));
    assert_eq!(hints(&mut client, &main, (9, 0), (4, 0)), json!([]));
    assert_eq!(hints(&mut client, &main, (100, 0), (101, 0)), json!([]));
    client.exit();
}

#[test]
fn inlay_hints_refresh_after_edits_and_never_reuse_failed_check_spans() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(&main, "def main() -> ():\n    value = 1\n");
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(client.open(&main), json!([]));
    assert_eq!(
        hints(&mut client, &main, (0, 0), (10, 0)),
        json!([type_hint(1, 9, "i64")])
    );
    for (i, (text, expected)) in [
        (
            "def main() -> ():\n    # moved\n    renamed = True\n",
            json!([type_hint(2, 11, "bool")]),
        ),
        ("def main() -> ():\n    value = (\n", json!([])),
        ("def main() -> ():\n    value = missing\n", json!([])),
        (
            "def main() -> ():\n    known = 1\n    wrong: bool = 3\n",
            json!([]),
        ),
        ("def main() -> ():\n    value: i64 = 2\n", json!([])),
        ("def main() -> ():\n    pass\n", json!([])),
        ("", json!([])),
        (
            "def main() -> ():\n    value = 2.0\n",
            json!([type_hint(1, 9, "f64")]),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        client.notify("textDocument/didChange", json!({"textDocument": {"uri": uri(&main), "version": i + 2}, "contentChanges": [{"text": text}]}));
        assert_eq!(
            hints(&mut client, &main, (0, 0), (10, 0)),
            expected,
            "{text}"
        );
    }
    client.exit();
}

#[test]
fn inlay_hints_respect_negotiated_unicode_positions() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    // Identifiers are ASCII, but preceding lines and multiline values can contain Unicode.
    write(
        &main,
        "# 𐐀é\ndef main() -> ():\n    text = (\n        \"𐐀é\"\n    )\n",
    );
    for encoding in ["utf-8", "utf-16", "utf-32"] {
        let end = 8;
        let mut client = Client::start_with_encodings(&dir.path().join("cache"), json!([encoding]));
        assert_eq!(client.open(&main), json!([]));
        assert_eq!(
            hints(&mut client, &main, (2, end), (3, 0)),
            json!([type_hint(2, end, "str")])
        );
        assert_eq!(hints(&mut client, &main, (0, 0), (2, end)), json!([]));
        assert_eq!(hints(&mut client, &main, (2, end + 1), (3, 0)), json!([]));
        client.exit();
    }
}

#[test]
fn inlay_hints_omit_ambiguous_generics_and_internal_types() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(
        &main,
        r#"def identity[T](value: T) -> T:
    local = value
    local

def unused[T](value: T) -> T:
    unresolved = value
    unresolved

def main() -> ():
    integer = identity(1)
    text = identity("hello")
    pipeline = iter(range(3))
    callback = (def [integer](x: i64) -> i64: x + integer)
    left, right = (1, 2)
    for i in range(2):
        pass
"#,
    );
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(client.open(&main), json!([]));
    assert_eq!(
        hints(&mut client, &main, (0, 0), (30, 0)),
        json!([type_hint(9, 11, "i64"), type_hint(10, 8, "str"),])
    );
    client.exit();
}

#[test]
fn inlay_hints_handle_unopened_unknown_and_invalid_requests() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(&main, "def main() -> ():\n    value = 1\n");
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(
        hints(&mut client, &main, (0, 0), (10, 0)),
        json!([type_hint(1, 9, "i64")])
    );
    for range in [
        json!(null),
        json!({"start": {"line": -1, "character": 0}, "end": {"line": 9, "character": 0}}),
    ] {
        assert_eq!(
            client.request(
                "textDocument/inlayHint",
                json!({"textDocument": {"uri": uri(&main)}, "range": range})
            ),
            json!([])
        );
    }
    for (uri, expected) in [
        ("untitled:unknown".to_owned(), Value::Null),
        (
            format!("file://{}/missing.plenty", dir.path().display()),
            json!([]),
        ),
    ] {
        assert_eq!(client.request("textDocument/inlayHint", json!({"textDocument": {"uri": uri}, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}}})), expected);
    }
    client.exit();
}

#[test]
fn inlay_hints_do_not_locate_generated_bindings_in_source_strings() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(
        &main,
        r#"class Point:
    x: i64
    def __init__(self, x: i64) -> ():
        self.x = x

def main() -> ():
    marker = "__plenty_instance"
    point = Point(1)
"#,
    );
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(client.open(&main), json!([]));
    assert_eq!(
        hints(&mut client, &main, (0, 0), (20, 0)),
        json!([type_hint(6, 10, "str"), type_hint(7, 9, "Point"),])
    );
    client.exit();
}
