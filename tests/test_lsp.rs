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
        Self::start_with(cache, encodings, json!({}))
    }

    /// Start with the client's `workspace.workspaceEdit` capabilities.
    fn start_with(cache: &Path, encodings: Value, workspace_edit: Value) -> Client {
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
                "workspace": {"workspaceEdit": workspace_edit},
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

    /// The error reply to a request that the server refuses.
    fn refused(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self.receive();
            if message["id"] == id {
                assert!(message.get("result").is_none(), "{message}");
                return message["error"].clone();
            }
        }
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

def scale(p: &Point, by: i64) -> Result[i64, OverflowError[i64]]:
    """Scale a point's x.

    Multiplies by `by`."""
    Ok(p.x * by)

def main() -> Result[(), OverflowError[i64]]:
    p = Point(3, 4)
    s = scale(&p, 2)?
    d = p.dist()?
    c = Color.Red
    g = gcd(6u64, 4u64)
    Ok(())
"#;

const SHAPES: &str = r#"pub class Point:
    pub x: i64
    pub y: i64

    pub def dist(self) -> Result[i64, OverflowError[i64]]:
        "Taxicab distance from the origin."
        Ok(self.x + self.y)

pub def area(p: &Point) -> Result[i64, OverflowError[i64]]:
    Ok(p.x * p.y)
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
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 9);
    assert_eq!(diagnostics[0]["message"], "expected i64, got str");
    // Editor recovery keeps independently established import warnings too.
    assert_eq!(diagnostics[1]["code"], "unused-import");
    assert_eq!(diagnostics[1]["severity"], 2);
    assert!(
        diagnostics[1]["message"]
            .as_str()
            .unwrap()
            .contains("`area`")
    );

    // A local function: signature with its docstring.
    let hover = client.hover(&main, 19, 8);
    assert!(
        hover.starts_with(
            "```plenty\ndef scale(p: &Point, by: i64) -> Result[i64, OverflowError[i64]]\n```"
        ),
        "{hover}"
    );
    assert!(
        hover.contains("Scale a point's x.\n\nMultiplies by `by`."),
        "{hover}"
    );
    assert_eq!(client.definition(&main, 19, 8).1, 11);

    // A method resolved through the receiver's type, in another module.
    let hover = client.hover(&main, 20, 10);
    assert!(
        hover.contains("def dist(self) -> Result[i64, OverflowError[i64]]"),
        "{hover}"
    );
    assert!(
        hover.contains("Taxicab distance from the origin."),
        "{hover}"
    );
    let (file, line, character) = client.definition(&main, 20, 10);
    assert_eq!(Path::new(&file), shapes.canonicalize().unwrap());
    assert_eq!((line, character), (4, 12));

    // A field, a parameter, a local with its type, an imported class.
    assert!(client.hover(&main, 15, 9).contains("pub x: i64"));
    assert!(client.hover(&main, 15, 7).contains("p: &shapes.Point"));
    assert_eq!(client.definition(&main, 15, 7), (file_of(&main), 11, 10));
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
    for expected in ["scale", "Point", "gcd", "p", "s", "Color", "while", "const"] {
        assert!(
            names.contains(&expected.to_owned()),
            "{expected}: {names:?}"
        );
    }
    // Keywords come from the parser's reserved words (issue #637): words
    // reserved only to be rejected, and non-keywords, are not offered.
    for absent in ["lambda", "try", "None", "is"] {
        assert!(!names.contains(&absent.to_owned()), "{absent}: {names:?}");
    }
    // Locals of another function are not offered.
    assert!(!names.contains(&"by".to_owned()), "{names:?}");

    // An edit fixes the error and adds a call; the unsaved text is checked.
    let fixed = MAIN
        .replace("    \"text\"\n", "    1\n")
        .replace("    Ok(())\n", "    a = area(&p)?\n    Ok(())\n");
    let uri = uri(&main);
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": fixed}]}),
    );
    assert_eq!(client.published(&uri), json!([]));
    let hover = client.hover(&main, 23, 8);
    assert!(
        hover.contains("pub def area(p: &Point) -> Result[i64, OverflowError[i64]]"),
        "{hover}"
    );
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
        "import geo\n\ndef main() -> Result[(), OverflowError[i64]]:\n    p = geo.Point(1, 2)\n    n = geo.area(&p)?\n    Ok(())\n",
    );
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(client.open(&main), json!([]));
    let hover = client.hover(&main, 4, 12);
    assert!(
        hover.contains("pub def area(p: &Point) -> Result[i64, OverflowError[i64]]"),
        "{hover}"
    );
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

def main() -> Result[(), AllocError | OverflowError[i64]]:
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
    callback = (def [integer](x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + integer))
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

#[test]
fn boxed_protocol_method_navigates_to_its_requirement() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(
        &main,
        "protocol Read:\n    def read(self) -> i64:\n        pass\nclass Value:\n    def read(self) -> i64:\n        7\ndef main() -> Result[(), AllocError | IoError]:\n    value = Box[Read](Value())?\n    print(value.read())?\n    Ok(())\n",
    );
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(client.open(&main), json!([]));
    let (file, line, _) = client.definition(&main, 8, 17);
    assert_eq!(file, main.to_str().unwrap());
    assert_eq!(line, 1);
    client.exit();
}

#[test]
fn warnings_are_published_with_warning_severity_and_their_lint_name() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(
        &main,
        "from helper import twice\n\ndef main() -> Result[(), AllocError | IoError]:\n    s = \"hi\"\n    t = copy(s)?\n    print(t, twice(2))?\n    Ok(())\n",
    );
    // A warning in an imported module belongs to that module's diagnostics.
    write(
        &dir.path().join("helper.plenty"),
        "pub def twice(n: i64) -> i64:\n    m = n\n    m\n\ndef unused() -> Result[str, AllocError]:\n    copy(\"x\")\n\npub def again(n: i64) -> Result[i64, AllocError]:\n    Ok(copy(n)?)\n",
    );
    let mut client = Client::start(&dir.path().join("cache"));
    let diagnostics = client.open(&main);
    let diagnostics = diagnostics.as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let warning = &diagnostics[0];
    assert_eq!(warning["severity"], 2);
    assert_eq!(warning["code"], "redundant-copy");
    assert_eq!(warning["range"]["start"]["line"], 4);
    assert_eq!(warning["range"]["start"]["character"], 8);
    assert!(
        warning["message"]
            .as_str()
            .unwrap()
            .starts_with("`copy(s)?` is redundant: `str` copies"),
        "{warning}"
    );
    client.exit();
}

#[test]
fn a_declaration_moves_through_a_resolved_code_action() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("plenty.toml"),
        "manifest-version = 1\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    );
    let shapes = root.join("src/shapes.plenty");
    let geometry = root.join("src/geometry.plenty");
    let main = root.join("src/main.plenty");
    write(&shapes, "pub def one() -> i64:\n    1\n");
    write(&geometry, "pub def two() -> i64:\n    2\n");
    write(
        &main,
        "import shapes\n\ndef main() -> ():\n    x = shapes.one()\n",
    );
    let mut client = Client::start(&root.join("cache"));
    assert_eq!(client.open(&shapes), json!([]));
    // The unsaved text is what moves.
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri(&shapes), "version": 2},
            "contentChanges": [{"text": "pub def one() -> i64:\n    3\n"}]}),
    );
    let actions = client.request(
        "textDocument/codeAction",
        json!({"textDocument": {"uri": uri(&shapes)},
            "range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 4}},
            "context": {"diagnostics": []}}),
    );
    assert_eq!(
        actions,
        json!([
            move_action(&uri(&shapes), &uri(&geometry), "geometry", "one", 2, 2),
            move_action(&uri(&shapes), &uri(&main), "main", "one", 2, 2),
        ])
    );
    let resolved = client.request("codeAction/resolve", actions[0].clone());
    let mut action = actions[0].clone();
    action["edit"] = json!({"changes": {
        uri(&geometry): [{"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 0}},
            "newText": "\npub def one() -> i64:\n    3\n"}],
        uri(&shapes): [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 0}},
            "newText": ""}],
        uri(&main): [
            {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 1, "character": 0}},
                "newText": ""},
            {"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}},
                "newText": "import geometry\n"},
            {"range": {"start": {"line": 3, "character": 8}, "end": {"line": 3, "character": 18}},
                "newText": "geometry.one"},
        ],
    }});
    assert_eq!(resolved, action);
    // Moving into `main` is also allowed: `shapes.one()` becomes `one()`.
    let into_main = client.request("codeAction/resolve", actions[1].clone());
    let mut action = actions[1].clone();
    action["edit"] = json!({"changes": {
        uri(&shapes): [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 0}},
            "newText": ""}],
        uri(&main): [
            {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 0}},
                "newText": ""},
            {"range": {"start": {"line": 3, "character": 8}, "end": {"line": 3, "character": 18}},
                "newText": "one"},
            {"range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 0}},
                "newText": "\npub def one() -> i64:\n    3\n"},
        ],
    }});
    assert_eq!(into_main, action);
    // A refused move is a request error, and nothing is edited. Asking
    // again gives the same error.
    client.open(&geometry);
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri(&geometry), "version": 2},
            "contentChanges": [{"text": "pub def one() -> i64:\n    2\n"}]}),
    );
    let on_disk = |paths: &[&Path]| -> Vec<String> {
        paths
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect()
    };
    let files_before = on_disk(&[&shapes, &geometry, &main]);
    for _ in 0..2 {
        let error = client.refused("codeAction/resolve", actions[0].clone());
        assert_eq!(
            error,
            json!({"code": -32803, "message": "`geometry` already declares `one`"})
        );
        assert_eq!(on_disk(&[&shapes, &geometry, &main]), files_before);
    }
    // A client without change annotations is told about API changes.
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri(&shapes), "version": 3},
            "contentChanges": [{"text": "def base() -> i64:\n    3\n\npub def one() -> i64:\n    base()\n"}]}),
    );
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri(&geometry), "version": 3},
            "contentChanges": [{"text": "pub def two() -> i64:\n    2\n"}]}),
    );
    let actions = client.request(
        "textDocument/codeAction",
        json!({"textDocument": {"uri": uri(&shapes)},
            "range": {"start": {"line": 3, "character": 0}, "end": {"line": 3, "character": 0}},
            "context": {"diagnostics": []}}),
    );
    client.next += 1;
    let id = client.next;
    client.send(
        json!({"jsonrpc": "2.0", "id": id, "method": "codeAction/resolve",
        "params": actions[0].clone()}),
    );
    let mut shown = None;
    loop {
        let message = client.receive();
        if message["method"] == "window/showMessage" {
            shown = message["params"]["message"].as_str().map(str::to_owned);
        }
        if message["id"] == id {
            let at = |line: u32, start: u32, end_line: u32, end: u32| json!({"start": {"line": line, "character": start}, "end": {"line": end_line, "character": end}});
            assert_eq!(
                message["result"]["edit"],
                json!({"changes": {
                    uri(&geometry): [
                        {"range": at(0, 0, 0, 0), "newText": "from shapes import base\n\n"},
                        {"range": at(2, 0, 2, 0), "newText": "\npub def one() -> i64:\n    base()\n"},
                    ],
                    uri(&main): [
                        {"range": at(0, 0, 1, 0), "newText": ""},
                        {"range": at(1, 0, 1, 0), "newText": "import geometry\n"},
                        {"range": at(3, 8, 3, 18), "newText": "geometry.one"},
                    ],
                    uri(&shapes): [
                        {"range": at(0, 0, 0, 0), "newText": "pub "},
                        {"range": at(2, 0, 5, 0), "newText": ""},
                    ],
                }})
            );
            break;
        }
    }
    assert_eq!(
        shown.as_deref(),
        Some("makes `shapes.base` public, because the moved code uses it")
    );
    // Outside a declaration there is nothing to move.
    let none = client.request(
        "textDocument/codeAction",
        json!({"textDocument": {"uri": uri(&main)},
            "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}},
            "context": {"diagnostics": []}}),
    );
    assert_eq!(none, json!([]));
    client.exit();
}

fn app_package(root: &Path, files: &[(&str, &str)]) {
    write(
        &root.join("plenty.toml"),
        "manifest-version = 1\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    );
    for (path, text) in files {
        write(&root.join(path), text);
    }
}

/// An unresolved move action, as the server offers it.
fn move_action(
    source: &str,
    destination: &str,
    module: &str,
    name: &str,
    line: u32,
    version: u32,
) -> Value {
    json!({
        "title": format!("Move `{name}` to module `{module}`"),
        "kind": "refactor.move",
        "data": {"uri": source, "destination": destination, "line": line, "name": name, "version": version},
    })
}

fn code_actions(client: &mut Client, uri: &str, line: u32) -> Value {
    client.request(
        "textDocument/codeAction",
        json!({"textDocument": {"uri": uri},
            "range": {"start": {"line": line, "character": 0}, "end": {"line": line, "character": 0}},
            "context": {"diagnostics": []}}),
    )
}

#[test]
fn a_move_is_a_versioned_annotated_edit_in_utf16_columns() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    app_package(
        root,
        &[
            (
                "src/shapes.plenty",
                "def base() -> i64:\n    3\n\ndef more() -> i64:\n    4\n\npub def one() -> (i64, i64):\n    (base(), more())\n",
            ),
            ("src/geometry.plenty", "pub def two() -> i64:\n    2\n"),
            (
                "src/main.plenty",
                "import shapes\n\ndef main() -> ():\n    x = (\"\u{1F600}\", shapes.one())\n",
            ),
        ],
    );
    let (shapes, geometry, main) = (
        root.join("src/shapes.plenty"),
        root.join("src/geometry.plenty"),
        root.join("src/main.plenty"),
    );
    let mut client = Client::start_with(
        &root.join("cache"),
        json!([]),
        json!({"documentChanges": true, "changeAnnotationSupport": {"groupsOnLabel": false}}),
    );
    client.open(&shapes);
    client.open(&main);
    let actions = code_actions(&mut client, &uri(&shapes), 7);
    assert_eq!(
        actions,
        json!([
            move_action(&uri(&shapes), &uri(&geometry), "geometry", "one", 8, 1),
            move_action(&uri(&shapes), &uri(&main), "main", "one", 8, 1),
        ])
    );
    let resolved = client.request("codeAction/resolve", actions[0].clone());
    let api =
        |range: Value, text: &str| json!({"range": range, "newText": text, "annotationId": "api"});
    let at = |line: u32, start: u32, end_line: u32, end: u32| json!({"start": {"line": line, "character": start}, "end": {"line": end_line, "character": end}});
    assert_eq!(
        resolved["edit"],
        json!({
            "documentChanges": [
                {"textDocument": {"uri": uri(&geometry), "version": null}, "edits": [
                    api(at(0, 0, 0, 0), "from shapes import base, more\n\n"),
                    api(at(2, 0, 2, 0), "\npub def one() -> (i64, i64):\n    (base(), more())\n"),
                ]},
                {"textDocument": {"uri": uri(&main), "version": 1}, "edits": [
                    api(at(0, 0, 1, 0), ""),
                    api(at(1, 0, 1, 0), "import geometry\n"),
                    // The emoji is two UTF-16 units: `shapes.one` starts at
                    // character 14 and unit 15.
                    api(at(3, 15, 3, 25), "geometry.one"),
                ]},
                {"textDocument": {"uri": uri(&shapes), "version": 1}, "edits": [
                    api(at(0, 0, 0, 0), "pub "),
                    api(at(3, 0, 3, 0), "pub "),
                    api(at(5, 0, 8, 0), ""),
                ]},
            ],
            "changeAnnotations": {"api": {
                "label": "Move `one` to `geometry`",
                "needsConfirmation": true,
                "description": "makes `shapes.base` public, because the moved code uses it; makes `shapes.more` public, because the moved code uses it",
            }},
        })
    );
    client.exit();
}

#[test]
fn an_action_offered_before_an_edit_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    app_package(
        root,
        &[
            ("src/shapes.plenty", "pub def one() -> i64:\n    1\n"),
            ("src/geometry.plenty", "pub def two() -> i64:\n    2\n"),
            (
                "src/main.plenty",
                "import shapes\n\ndef main() -> ():\n    x = shapes.one()\n",
            ),
        ],
    );
    let shapes = root.join("src/shapes.plenty");
    let mut client = Client::start(&root.join("cache"));
    client.open(&shapes);
    let actions = code_actions(&mut client, &uri(&shapes), 0);
    let stale = "the file has changed since this move was offered; ask for code actions again";
    // Another declaration now starts on the offered line.
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri(&shapes), "version": 2},
            "contentChanges": [{"text": "pub def other() -> i64:\n    1\n\npub def one() -> i64:\n    1\n"}]}),
    );
    let stale_error = json!({"code": -32803, "message": stale});
    for _ in 0..2 {
        let error = client.refused("codeAction/resolve", actions[0].clone());
        assert_eq!(error, stale_error);
    }
    // The same declaration, edited since.
    let actions = code_actions(&mut client, &uri(&shapes), 0);
    let geometry = root.join("src/geometry.plenty");
    let main = root.join("src/main.plenty");
    assert_eq!(
        actions,
        json!([
            move_action(&uri(&shapes), &uri(&geometry), "geometry", "other", 1, 2),
            move_action(&uri(&shapes), &uri(&main), "main", "other", 1, 2),
        ])
    );
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri(&shapes), "version": 3},
            "contentChanges": [{"text": "pub def other() -> i64:\n    2\n\npub def one() -> i64:\n    1\n"}]}),
    );
    for _ in 0..2 {
        let error = client.refused("codeAction/resolve", actions[0].clone());
        assert_eq!(error, stale_error);
    }
    assert_eq!(
        std::fs::read_to_string(&shapes).unwrap(),
        "pub def one() -> i64:\n    1\n"
    );
    client.exit();
}

#[test]
fn edits_name_open_documents_by_the_clients_uri() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    app_package(
        &real,
        &[
            ("src/shapes.plenty", "pub def one() -> i64:\n    1\n"),
            ("src/geometry.plenty", "pub def two() -> i64:\n    2\n"),
            (
                "src/main.plenty",
                "import shapes\n\ndef main() -> ():\n    x = shapes.one()\n",
            ),
        ],
    );
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let linked = |path: &str| format!("file://{}", link.join(path).display());
    let mut client = Client::start(&dir.path().join("cache"));
    let shapes = linked("src/shapes.plenty");
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": shapes, "languageId": "plenty", "version": 1,
            "text": "pub def one() -> i64:\n    1\n"}}),
    );
    client.published(&shapes);
    let actions = code_actions(&mut client, &shapes, 0);
    let resolved = client.request("codeAction/resolve", actions[0].clone());
    // Closed files are named by their canonical path.
    let at = |line: u32, start: u32, end_line: u32, end: u32| json!({"start": {"line": line, "character": start}, "end": {"line": end_line, "character": end}});
    assert_eq!(
        resolved["edit"],
        json!({"changes": {
            shapes.clone(): [{"range": at(0, 0, 2, 0), "newText": ""}],
            uri(&real.join("src/geometry.plenty")): [
                {"range": at(2, 0, 2, 0), "newText": "\npub def one() -> i64:\n    1\n"},
            ],
            uri(&real.join("src/main.plenty")): [
                {"range": at(0, 0, 1, 0), "newText": ""},
                {"range": at(1, 0, 1, 0), "newText": "import geometry\n"},
                {"range": at(3, 8, 3, 18), "newText": "geometry.one"},
            ],
        }})
    );
    client.exit();
}

#[test]
fn unused_imports_are_published_as_lsp_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(&main, "import std.math as m\ndef main() -> ():\n    pass\n");
    let mut client = Client::start(&dir.path().join("cache"));
    let diagnostics = client.open(&main);
    let diagnostics = diagnostics.as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["severity"], 2);
    assert_eq!(diagnostics[0]["code"], "unused-import");
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 0);
    assert_eq!(diagnostics[0]["range"]["start"]["character"], 19);
    client.exit();
}

/// A library member with tests, and an application member that depends on
/// it (issue #821).
const REFERENCES_WORKSPACE: &[(&str, &str)] = &[
    (
        "plenty.toml",
        "manifest-version = 1\n\n[workspace]\nmembers = [\"tools\", \"app\"]\n",
    ),
    (
        "tools/plenty.toml",
        "manifest-version = 1\n\n[package]\nname = \"tools\"\nversion = \"0.1.0\"\n",
    ),
    (
        "tools/src/lib.plenty",
        "from shapes import area\n\npub def total(n: i64) -> i64:\n    unchecked:\n        area(n) + area(1)\n",
    ),
    (
        "tools/src/shapes.plenty",
        "pub def area(n: i64) -> i64:\n    unchecked:\n        n * 2\n\npub def twice(area: i64) -> i64:\n    unchecked:\n        area + area\n",
    ),
    (
        "tools/tests/test_shapes.plenty",
        "import std.test as t\nfrom tools.shapes import area as size\n\ndef test_area() -> Result[(), AllocError | t.TestError]:\n    t.equal(&size(2), &4)?\n    Ok(())\n",
    ),
    (
        "app/plenty.toml",
        "manifest-version = 1\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\ntools = { path = \"../tools\" }\n",
    ),
    (
        "app/src/main.plenty",
        "import tools.shapes as s\n\ndef main() -> Result[(), IoError]:\n    print((\"𝄞\", s.area(2)))?\n    Ok(())\n",
    ),
];

/// Each reference as (path below `root`, line, character, text at the range).
#[track_caller]
fn reference_list(root: &Path, result: &Value) -> Vec<(String, u64, u64, String)> {
    let Some(found) = result.as_array() else {
        panic!("no references: {result}");
    };
    let mut out: Vec<(String, u64, u64, String)> = found
        .iter()
        .map(|r| {
            let path = Path::new(r["uri"].as_str().unwrap().strip_prefix("file://").unwrap());
            let start = &r["range"]["start"];
            let end = &r["range"]["end"];
            let line = start["line"].as_u64().unwrap();
            let (from, to) = (
                start["character"].as_u64().unwrap() as usize,
                end["character"].as_u64().unwrap() as usize,
            );
            let text = std::fs::read_to_string(path).unwrap();
            let units: Vec<u16> = text
                .lines()
                .nth(line as usize)
                .unwrap()
                .encode_utf16()
                .collect();
            (
                path.strip_prefix(root)
                    .map_or_else(|_| path.display().to_string(), |p| p.display().to_string()),
                line,
                from as u64,
                String::from_utf16(&units[from..to]).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

/// A references result, and the warnings the server showed while finding it.
fn references_and_warnings(
    client: &mut Client,
    path: &Path,
    line: u32,
    character: u32,
    declaration: bool,
) -> (Value, Vec<String>) {
    client.next += 1;
    let id = client.next;
    client.send(
        json!({"jsonrpc": "2.0", "id": id, "method": "textDocument/references",
        "params": {"textDocument": {"uri": uri(path)},
            "position": {"line": line, "character": character},
            "context": {"includeDeclaration": declaration}}}),
    );
    let mut warnings = vec![];
    loop {
        let message = client.receive();
        if message["method"] == "window/showMessage" {
            warnings.push(message["params"]["message"].as_str().unwrap().to_owned());
        }
        if message["id"] == id {
            assert!(message.get("error").is_none(), "{message}");
            return (message["result"].clone(), warnings);
        }
    }
}

/// A references result, when every entry checks.
#[track_caller]
fn references(
    client: &mut Client,
    path: &Path,
    line: u32,
    character: u32,
    declaration: bool,
) -> Value {
    let (result, warnings) = references_and_warnings(client, path, line, character, declaration);
    assert!(warnings.is_empty(), "{warnings:?}");
    result
}

#[test]
fn references_span_members_targets_aliases_and_unsaved_text() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in REFERENCES_WORKSPACE {
        write(&dir.path().join(path), text);
    }
    let root = dir.path().canonicalize().unwrap();
    let shapes = root.join("tools/src/shapes.plenty");
    let main = root.join("app/src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    // The library, its test (through an alias), and the application
    // member, where a character outside the BMP counts two UTF-16 units.
    let r = |path: &str, line: u64, character: u64, text: &str| {
        (path.to_owned(), line, character, text.to_owned())
    };
    let expected = vec![
        r("app/src/main.plenty", 3, 19, "area"),
        r("tools/src/lib.plenty", 0, 19, "area"),
        r("tools/src/lib.plenty", 4, 8, "area"),
        r("tools/src/lib.plenty", 4, 18, "area"),
        r("tools/src/shapes.plenty", 0, 8, "area"),
        r("tools/tests/test_shapes.plenty", 1, 25, "area"),
        r("tools/tests/test_shapes.plenty", 4, 13, "size"),
    ];
    let found = reference_list(&root, &references(&mut client, &shapes, 0, 8, true));
    assert_eq!(found, expected);
    // The same declaration from a use in another member and from an alias.
    let found = reference_list(&root, &references(&mut client, &main, 3, 21, true));
    assert_eq!(found, expected);
    let test = root.join("tools/tests/test_shapes.plenty");
    let found = reference_list(&root, &references(&mut client, &test, 4, 14, true));
    assert_eq!(found, expected);
    let without: Vec<_> = expected
        .iter()
        .filter(|e| e.0 != "tools/src/shapes.plenty")
        .cloned()
        .collect();
    let found = reference_list(&root, &references(&mut client, &shapes, 0, 8, false));
    assert_eq!(found, without);
    // A `std` declaration is listed from its read-only copy, and never
    // searched inside `std`.
    let found = reference_list(&root, &references(&mut client, &test, 4, 6, true));
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[1], r("tools/tests/test_shapes.plenty", 4, 6, "equal"));
    assert!(
        found[0].0.contains("std-sources") && found[0].0.ends_with("std/test.plenty"),
        "{found:?}"
    );
    // A parameter that shadows the function is a different declaration.
    let found = reference_list(&root, &references(&mut client, &shapes, 6, 8, true));
    assert_eq!(
        found,
        vec![
            r("tools/src/shapes.plenty", 4, 14, "area"),
            r("tools/src/shapes.plenty", 6, 8, "area"),
            r("tools/src/shapes.plenty", 6, 15, "area"),
        ]
    );
    // Unsaved text of an open document replaces the file on disk.
    client.open(&main);
    let edited = "import tools.shapes as s\n\ndef main() -> Result[(), IoError]:\n    print(s.area(1), s.area(2))?\n    Ok(())\n";
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri(&main), "version": 2},
            "contentChanges": [{"text": edited}]}),
    );
    let result = references(&mut client, &shapes, 0, 8, true);
    let found: Vec<_> = result
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["uri"] == uri(&main))
        .map(|l| l["range"]["start"]["character"].as_u64().unwrap())
        .collect();
    assert_eq!(found, vec![12, 23]);
    // Unsaved text in another open document moves the declaration; the
    // search from main still finds it, from a check of the same text.
    client.open(&shapes);
    let moved = format!("\n\n{}", std::fs::read_to_string(&shapes).unwrap());
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri(&shapes), "version": 2},
            "contentChanges": [{"text": moved}]}),
    );
    let found = positions(&root, &references(&mut client, &main, 3, 13, true));
    assert_eq!(
        found,
        vec![
            ("app/src/main.plenty".to_owned(), 3, 12),
            ("app/src/main.plenty".to_owned(), 3, 23),
            ("tools/src/lib.plenty".to_owned(), 0, 19),
            ("tools/src/lib.plenty".to_owned(), 4, 8),
            ("tools/src/lib.plenty".to_owned(), 4, 18),
            ("tools/src/shapes.plenty".to_owned(), 2, 8),
            ("tools/tests/test_shapes.plenty".to_owned(), 1, 25),
            ("tools/tests/test_shapes.plenty".to_owned(), 4, 13),
        ]
    );
    client.exit();
}

/// Each reference as (path below `root`, line, character), for text that
/// differs from the file on disk.
fn positions(root: &Path, result: &Value) -> Vec<(String, u64, u64)> {
    let Some(found) = result.as_array() else {
        panic!("no references: {result}");
    };
    let mut out: Vec<_> = found
        .iter()
        .map(|r| {
            let path = Path::new(r["uri"].as_str().unwrap().strip_prefix("file://").unwrap());
            let start = &r["range"]["start"];
            (
                path.strip_prefix(root).unwrap().display().to_string(),
                start["line"].as_u64().unwrap(),
                start["character"].as_u64().unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn references_reread_unopened_files_and_include_files_no_entry_reads() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let other = "from util import helper\n\npub def g() -> i64:\n    helper()\n";
    app_package(
        &root,
        &[
            ("src/util.plenty", "pub def helper() -> i64:\n    1\n"),
            (
                "src/main.plenty",
                "from util import helper\n\ndef main() -> ():\n    n = helper()\n",
            ),
            ("src/other.plenty", other),
            (
                "examples/demo.plenty",
                "def one() -> i64:\n    1\n\ndef main() -> ():\n    a = one()\n    b = one()\n",
            ),
        ],
    );
    let util = root.join("src/util.plenty");
    let other_path = root.join("src/other.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let lines = |found: Vec<(String, u64, u64)>| -> Vec<(String, u64)> {
        found.into_iter().map(|(p, l, _)| (p, l)).collect()
    };
    let expected = |other_line: u64| {
        vec![
            ("src/main.plenty".to_owned(), 0),
            ("src/main.plenty".to_owned(), 3),
            ("src/other.plenty".to_owned(), other_line - 3),
            ("src/other.plenty".to_owned(), other_line),
            ("src/util.plenty".to_owned(), 0),
        ]
    };
    let found = positions(&root, &references(&mut client, &other_path, 3, 4, true));
    assert_eq!(lines(found), expected(3));
    // A file read for one request is not unsaved text: a later change on
    // disk is seen.
    write(&other_path, &format!("\n\n{other}"));
    let found = positions(&root, &references(&mut client, &util, 0, 8, true));
    assert_eq!(lines(found), expected(5));
    // No entry reads an example, so its own check supplies its references.
    let demo = root.join("examples/demo.plenty");
    let found = positions(&root, &references(&mut client, &demo, 0, 4, true));
    assert_eq!(
        lines(found),
        vec![
            ("examples/demo.plenty".to_owned(), 0),
            ("examples/demo.plenty".to_owned(), 4),
            ("examples/demo.plenty".to_owned(), 5),
        ]
    );
    client.exit();
}

#[test]
fn references_warn_when_an_entry_does_not_check_and_work_outside_a_package() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(
        &root,
        &[
            ("src/lib.plenty", "pub def version() -> i64:\n    1\n"),
            ("src/util.plenty", "pub def helper() -> i64:\n    1\n"),
            (
                "src/main.plenty",
                "from util import helper\n\ndef main() -> ():\n    n = helper()\n",
            ),
            (
                "tests/test_util.plenty",
                "from app.util import helper\n\ndef test_helper() -> ():\n    n: str = helper()\n",
            ),
        ],
    );
    let util = root.join("src/util.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let (result, warnings) = references_and_warnings(&mut client, &util, 0, 8, true);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = &warnings[0];
    assert!(
        warning
            .starts_with("references may be incomplete: 1 of the package's entries do not check")
            && warning.contains("tests/test_util.plenty:4:"),
        "{warning}"
    );
    // The test's check still records its references.
    let found: Vec<_> = reference_list(&root, &result)
        .into_iter()
        .map(|(path, line, _, _)| (path, line))
        .collect();
    assert_eq!(
        found,
        vec![
            ("src/main.plenty".to_owned(), 0),
            ("src/main.plenty".to_owned(), 3),
            ("src/util.plenty".to_owned(), 0),
            ("tests/test_util.plenty".to_owned(), 0),
            ("tests/test_util.plenty".to_owned(), 3),
        ]
    );
    // Outside a package, the file's own check answers.
    let outside = tempfile::tempdir().unwrap();
    let single = outside.path().canonicalize().unwrap().join("script.plenty");
    write(
        &single,
        "def one() -> i64:\n    1\n\ndef main() -> ():\n    a = one()\n    b = one()\n",
    );
    let found = reference_list(&root, &references(&mut client, &single, 0, 4, true));
    let lines: Vec<u64> = found.iter().map(|f| f.1).collect();
    assert_eq!(lines, vec![0, 4, 5]);
    client.exit();
}

/// An entry module's `def iter` does not capture builtin `iter` calls in
/// a module it imports (issue #898).
#[test]
fn references_and_renames_of_an_entry_iter_skip_imported_builtin_calls() {
    let dir = tempfile::tempdir().unwrap();
    let main_text = "from helper import count\n\ndef iter(n: i64) -> i64:\n    n\n\ndef main() -> Result[(), AllocError | IoError]:\n    xs = [1, 2].unwrap()\n    print(iter(count(&xs)))?\n    Ok(())\n";
    let helper_text = "pub def count(xs: &list[i64]) -> i64:\n    iter(xs).count()\n";
    for (path, text) in [
        (
            "plenty.toml",
            "manifest-version = 1\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        ),
        ("src/main.plenty", main_text),
        ("src/helper.plenty", helper_text),
    ] {
        write(&dir.path().join(path), text);
    }
    let root = dir.path().canonicalize().unwrap();
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let r = |line: u64, character: u64| {
        (
            "src/main.plenty".to_owned(),
            line,
            character,
            "iter".to_owned(),
        )
    };
    let found = reference_list(&root, &references(&mut client, &main, 2, 4, true));
    assert_eq!(found, vec![r(2, 4), r(7, 10)]);
    let (edit, _) = rename_reply(&mut client, &main, 2, 4, "walk").unwrap();
    let edited = renamed(&root, &edit, &[]);
    assert_eq!(
        edited.keys().collect::<Vec<_>>(),
        vec!["src/main.plenty"],
        "{edited:?}"
    );
    assert_eq!(edited["src/main.plenty"], main_text.replace("iter", "walk"));
    client.exit();
}

/// Named arguments, constructor keywords, `case` patterns and variant
/// constructors refer to the parameter, field or variant they name
/// (issue #876).
const CALL_AND_PATTERN_NAMES: &str = "class Point:
    x: i64
    y: i64

enum Shape:
    Circle(i64)
    Square(i64)

def area(width: i64) -> i64:
    width

def size(s: &Shape) -> i64:
    match s:
        case Shape.Circle(r):
            *r
        case Shape.Square(n):
            *n

def origin(p: &Point) -> bool:
    match p:
        case Point(x=0, y=0):
            True
        case Point(x=_, y=_):
            False

def main() -> ():
    p = Point(x=1, y=2)
    a = area(width=p.x)
    s = size(&Shape.Circle(a))
    o = origin(&p)
";

#[test]
fn references_cover_named_arguments_patterns_and_variant_constructors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(&root, &[("src/main.plenty", CALL_AND_PATTERN_NAMES)]);
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let at = |found: Vec<(String, u64, u64)>| -> Vec<(u64, u64)> {
        found.into_iter().map(|(_, l, c)| (l, c)).collect()
    };
    for ((line, character), expected) in [
        ((1, 4), vec![(1, 4), (20, 19), (22, 19), (26, 14), (27, 21)]),
        ((8, 9), vec![(8, 9), (9, 4), (27, 13)]),
        ((5, 4), vec![(5, 4), (13, 19), (28, 20)]),
    ] {
        let found = positions(
            &root,
            &references(&mut client, &main, line, character, true),
        );
        assert_eq!(at(found), expected, "from {line}:{character}");
    }
    // Definition uses the same references: each use opens its declaration.
    for ((line, character), declared) in
        [((27, 21), (1, 4)), ((27, 13), (8, 9)), ((28, 20), (5, 4))]
    {
        let (_, l, c) = client.definition(&main, line, character);
        assert_eq!((l, c), declared, "from {line}:{character}");
    }
    client.exit();
}

/// Named arguments of methods, written initializers and generic functions.
const NAMED_ARGUMENTS: &str = "class Counter:
    count: i64

    def __init__(self: &mut Counter, start: i64) -> ():
        self.count = start

    def scaled(self: &Counter, by: i64) -> i64:
        unchecked:
            self.count * by

def pick[T](first: T, second: T) -> T:
    first

def main() -> ():
    c = Counter(start=1)
    n = c.scaled(by=2)
    m = pick(second=n, first=3)
";

#[test]
fn references_cover_method_initializer_and_generic_named_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(&root, &[("src/main.plenty", NAMED_ARGUMENTS)]);
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let at = |found: Vec<(String, u64, u64)>| -> Vec<(u64, u64)> {
        found.into_iter().map(|(_, l, c)| (l, c)).collect()
    };
    for ((line, character), expected) in [
        ((3, 37), vec![(3, 37), (4, 21), (14, 16)]),
        ((6, 31), vec![(6, 31), (8, 25), (15, 17)]),
        ((10, 12), vec![(10, 12), (11, 4), (16, 23)]),
        ((10, 22), vec![(10, 22), (16, 13)]),
    ] {
        let found = positions(
            &root,
            &references(&mut client, &main, line, character, true),
        );
        assert_eq!(at(found), expected, "from {line}:{character}");
    }
    client.exit();
}

/// Generic bodies no call instantiates (`total`, and `Holder.left` of a
/// generic class never used), beside one that is instantiated (`one`).
const GENERIC_BODIES: &str = "protocol Sized:
    def size(self) -> i64:
        pass

class Point:
    x: i64

    def norm(self, scale: i64) -> i64:
        unchecked:
            self.x * scale

class Crate:
    width: i64

    def size(self) -> i64:
        self.width

enum Shape:
    Dot
    Circle(i64)

class Holder[T]:
    item: T
    origin: Point

    def left(self) -> i64:
        self.origin.norm(scale=self.origin.x)

def total[T: Sized](a: &T, p: &Point) -> i64:
    q = p.norm(2)
    match Shape.Circle(q):
        case Shape.Circle(r):
            unchecked:
                a.size() + p.x + r
        case Shape.Dot:
            0

def one[T: Sized](a: &T) -> i64:
    a.size()

def main() -> ():
    n = one(&Crate(width=1))
";

#[test]
fn references_cover_members_in_uninstantiated_generic_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(&root, &[("src/main.plenty", GENERIC_BODIES)]);
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let at = |found: Vec<(String, u64, u64)>| -> Vec<(u64, u64)> {
        found.into_iter().map(|(_, l, c)| (l, c)).collect()
    };
    for ((line, character), expected) in [
        // A protocol-bounded receiver refers to the requirement, except
        // where an instantiation resolved it to the class's method.
        ((1, 8), vec![(1, 8), (33, 18)]),
        ((14, 8), vec![(14, 8), (38, 6)]),
        ((5, 4), vec![(5, 4), (9, 17), (26, 43), (33, 29)]),
        ((7, 8), vec![(7, 8), (26, 20), (29, 10)]),
        ((7, 19), vec![(7, 19), (9, 21), (26, 25)]),
        ((23, 4), vec![(23, 4), (26, 13), (26, 36)]),
        ((19, 4), vec![(19, 4), (30, 16), (31, 19)]),
        ((18, 4), vec![(18, 4), (34, 19)]),
    ] {
        let found = positions(
            &root,
            &references(&mut client, &main, line, character, true),
        );
        assert_eq!(at(found), expected, "from {line}:{character}");
    }
    let (_, line, _) = client.definition(&main, 33, 18);
    assert_eq!(line, 1);
    client.exit();
}

#[test]
fn references_cover_module_qualified_variants_and_named_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(
        &root,
        &[
            (
                "src/shapes.plenty",
                "pub enum Shape:\n    Circle(i64)\n    Dot\n\npub def area(width: i64) -> i64:\n    width\n",
            ),
            (
                "src/main.plenty",
                "import shapes as s\n\ndef size(v: &s.Shape) -> i64:\n    match v:\n        case s.Shape.Circle(r):\n            *r\n        case s.Shape.Dot:\n            0\n\ndef main() -> ():\n    n = size(&s.Shape.Circle(s.area(width=2)))\n",
            ),
        ],
    );
    let shapes = root.join("src/shapes.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let r = |path: &str, line: u64, character: u64| (path.to_owned(), line, character);
    for ((line, character), expected) in [
        (
            (1, 4),
            vec![
                r("src/main.plenty", 4, 21),
                r("src/main.plenty", 10, 22),
                r("src/shapes.plenty", 1, 4),
            ],
        ),
        (
            (2, 4),
            vec![r("src/main.plenty", 6, 21), r("src/shapes.plenty", 2, 4)],
        ),
        (
            (4, 13),
            vec![
                r("src/main.plenty", 10, 36),
                r("src/shapes.plenty", 4, 13),
                r("src/shapes.plenty", 5, 4),
            ],
        ),
    ] {
        let found = positions(
            &root,
            &references(&mut client, &shapes, line, character, true),
        );
        assert_eq!(found, expected, "from {line}:{character}");
    }
    client.exit();
}

/// Only a callable another module can call is searched across the package
/// for its parameters' named arguments; the failing test entry makes that
/// search warn, and a file-local answer does not.
#[test]
fn private_parameters_are_answered_from_their_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let util = "pub def helper(width: i64) -> i64:\n    secret(width=width)\n\ndef secret(width: i64) -> i64:\n    width\n\npub class Meter:\n    pub v: i64\n\n    pub def shown(self, by: i64) -> i64:\n        self.hidden(by=by)\n\n    def hidden(self, by: i64) -> i64:\n        by\n";
    app_package(
        &root,
        &[
            ("src/lib.plenty", "pub def version() -> i64:\n    1\n"),
            ("src/util.plenty", util),
            (
                "src/main.plenty",
                "from util import helper, Meter\n\ndef main() -> ():\n    n = helper(width=1)\n    m = Meter(v=1).shown(by=2)\n",
            ),
            (
                "tests/test_util.plenty",
                "from app.util import helper\n\ndef test_helper() -> ():\n    n: str = helper(width=1)\n",
            ),
        ],
    );
    let util = root.join("src/util.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    for ((line, character), searched, expected) in [
        (
            (0, 15),
            true,
            vec![
                ("src/main.plenty", 3),
                ("src/util.plenty", 0),
                ("src/util.plenty", 1),
                ("tests/test_util.plenty", 3),
            ],
        ),
        (
            (3, 11),
            false,
            vec![
                ("src/util.plenty", 1),
                ("src/util.plenty", 3),
                ("src/util.plenty", 4),
            ],
        ),
        (
            (9, 24),
            true,
            vec![
                ("src/main.plenty", 4),
                ("src/util.plenty", 9),
                ("src/util.plenty", 10),
            ],
        ),
        (
            (12, 21),
            false,
            vec![
                ("src/util.plenty", 10),
                ("src/util.plenty", 12),
                ("src/util.plenty", 13),
            ],
        ),
    ] {
        let (result, warnings) = references_and_warnings(&mut client, &util, line, character, true);
        assert_eq!(
            warnings.len(),
            usize::from(searched),
            "from {line}:{character}: {warnings:?}"
        );
        let mut found: Vec<_> = reference_list(&root, &result)
            .into_iter()
            .map(|(path, line, _, _)| (path, line))
            .collect();
        found.dedup();
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(p, l)| (p.to_owned(), l))
            .collect();
        assert_eq!(found, expected, "from {line}:{character}");
    }
    client.exit();
}

/// A rename's reply: the workspace edit and the warnings the server showed,
/// or the reason it was refused.
fn rename_reply(
    client: &mut Client,
    path: &Path,
    line: u32,
    character: u32,
    new_name: &str,
) -> Result<(Value, Vec<String>), String> {
    client.next += 1;
    let id = client.next;
    client.send(
        json!({"jsonrpc": "2.0", "id": id, "method": "textDocument/rename",
        "params": {"textDocument": {"uri": uri(path)},
            "position": {"line": line, "character": character}, "newName": new_name}}),
    );
    let mut warnings = vec![];
    loop {
        let message = client.receive();
        if message["method"] == "window/showMessage" {
            warnings.push(message["params"]["message"].as_str().unwrap().to_owned());
        }
        if message["id"] == id {
            if let Some(error) = message.get("error") {
                assert_eq!(error["code"], -32803, "{message}");
                return Err(error["message"].as_str().unwrap().to_owned());
            }
            return Ok((message["result"].clone(), warnings));
        }
    }
}

/// The reason a rename is refused.
#[track_caller]
fn rename_refused(
    client: &mut Client,
    path: &Path,
    line: u32,
    character: u32,
    new_name: &str,
) -> String {
    match rename_reply(client, path, line, character, new_name) {
        Ok((edit, _)) => panic!("the rename was accepted: {edit}"),
        Err(message) => message,
    }
}

/// The reason `textDocument/prepareRename` refuses the position, or its
/// placeholder and range.
fn prepare_rename(
    client: &mut Client,
    path: &Path,
    line: u32,
    character: u32,
) -> Result<Value, String> {
    let params = json!({"textDocument": {"uri": uri(path)},
        "position": {"line": line, "character": character}});
    client.next += 1;
    let id = client.next;
    client.send(
        json!({"jsonrpc": "2.0", "id": id, "method": "textDocument/prepareRename",
        "params": params}),
    );
    loop {
        let message = client.receive();
        if message["id"] == id {
            return match message.get("error") {
                Some(error) => Err(error["message"].as_str().unwrap().to_owned()),
                None => Ok(message["result"].clone()),
            };
        }
    }
}

/// Each edited file's text below `root` after applying `edit`, whose
/// columns count UTF-16 units, to `texts` or the file on disk.
#[track_caller]
fn renamed(
    root: &Path,
    edit: &Value,
    texts: &[(&Path, &str)],
) -> std::collections::BTreeMap<String, String> {
    let files: Vec<(String, Value)> = match edit["documentChanges"].as_array() {
        Some(changes) => changes
            .iter()
            .map(|c| {
                (
                    c["textDocument"]["uri"].as_str().unwrap().to_owned(),
                    c["edits"].clone(),
                )
            })
            .collect(),
        None => edit["changes"]
            .as_object()
            .unwrap_or_else(|| panic!("no edit: {edit}"))
            .iter()
            .map(|(u, e)| (u.clone(), e.clone()))
            .collect(),
    };
    let mut out = std::collections::BTreeMap::new();
    for (file, edits) in files {
        let path = Path::new(file.strip_prefix("file://").unwrap());
        let text = texts.iter().find(|(p, _)| *p == path).map_or_else(
            || std::fs::read_to_string(path).unwrap(),
            |(_, t)| (*t).to_owned(),
        );
        let mut lines: Vec<Vec<u16>> = text
            .split('\n')
            .map(|l| l.encode_utf16().collect())
            .collect();
        let mut edits = edits.as_array().unwrap().clone();
        edits.sort_by_key(|e| {
            let start = &e["range"]["start"];
            (start["line"].as_u64(), start["character"].as_u64())
        });
        for e in edits.iter().rev() {
            let (start, end) = (&e["range"]["start"], &e["range"]["end"]);
            assert_eq!(start["line"], end["line"], "{e}");
            let line = &mut lines[start["line"].as_u64().unwrap() as usize];
            let (from, to) = (
                start["character"].as_u64().unwrap() as usize,
                end["character"].as_u64().unwrap() as usize,
            );
            line.splice(from..to, e["newText"].as_str().unwrap().encode_utf16());
        }
        let text: Vec<String> = lines
            .iter()
            .map(|l| String::from_utf16(l).unwrap())
            .collect();
        let relative = path.strip_prefix(root).unwrap().display().to_string();
        out.insert(relative, text.join("\n"));
    }
    out
}

#[test]
fn a_rename_spans_members_tests_and_qualified_uses_and_keeps_aliases() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in REFERENCES_WORKSPACE {
        write(&dir.path().join(path), text);
    }
    let root = dir.path().canonicalize().unwrap();
    let shapes = root.join("tools/src/shapes.plenty");
    let main = root.join("app/src/main.plenty");
    let test = root.join("tools/tests/test_shapes.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let expected: std::collections::BTreeMap<String, String> = [
        (
            "app/src/main.plenty",
            "import tools.shapes as s\n\ndef main() -> Result[(), IoError]:\n    print((\"𝄞\", s.measure(2)))?\n    Ok(())\n",
        ),
        (
            "tools/src/lib.plenty",
            "from shapes import measure\n\npub def total(n: i64) -> i64:\n    unchecked:\n        measure(n) + measure(1)\n",
        ),
        // The parameter `area` is another declaration.
        (
            "tools/src/shapes.plenty",
            "pub def measure(n: i64) -> i64:\n    unchecked:\n        n * 2\n\npub def twice(area: i64) -> i64:\n    unchecked:\n        area + area\n",
        ),
        // The alias and its uses stay.
        (
            "tools/tests/test_shapes.plenty",
            "import std.test as t\nfrom tools.shapes import measure as size\n\ndef test_area() -> Result[(), AllocError | t.TestError]:\n    t.equal(&size(2), &4)?\n    Ok(())\n",
        ),
    ]
    .into_iter()
    .map(|(p, t)| (p.to_owned(), t.to_owned()))
    .collect();
    let api = "renames public `tools.shapes.area` to `measure`, so packages outside this workspace that import it must change";
    // From the declaration and from a qualified use in another member,
    // after a character outside the BMP, which counts two UTF-16 units.
    for (path, line, character) in [(&shapes, 0, 8), (&main, 3, 21)] {
        let (edit, warnings) = rename_reply(&mut client, path, line, character, "measure").unwrap();
        assert_eq!(renamed(&root, &edit, &[]), expected);
        // The client shows no change annotations, so the API change is a
        // warning.
        assert_eq!(warnings, vec![api.to_owned()]);
    }
    assert_eq!(
        prepare_rename(&mut client, &main, 3, 21),
        Ok(
            json!({"range": {"start": {"line": 3, "character": 19}, "end": {"line": 3, "character": 23}},
            "placeholder": "area"})
        )
    );
    // Renaming the alias, from a use or from the import, changes only the
    // test file.
    let aliased = "import std.test as t\nfrom tools.shapes import area as sized\n\ndef test_area() -> Result[(), AllocError | t.TestError]:\n    t.equal(&sized(2), &4)?\n    Ok(())\n";
    for (line, character) in [(4, 14), (1, 34)] {
        assert_eq!(
            prepare_rename(&mut client, &test, line, character).unwrap()["placeholder"],
            "size"
        );
        let (edit, warnings) = rename_reply(&mut client, &test, line, character, "sized").unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let files = renamed(&root, &edit, &[]);
        assert_eq!(
            files,
            [(
                "tools/tests/test_shapes.plenty".to_owned(),
                aliased.to_owned()
            )]
            .into()
        );
    }
    // Nothing is written.
    assert_eq!(
        std::fs::read_to_string(&shapes).unwrap(),
        REFERENCES_WORKSPACE[3].1
    );
    client.exit();
}

const MEMBERS_SHAPES: &str = "pub class Point:
    pub x: i64

    pub def norm(self, scale: i64) -> i64:
        unchecked:
            self.x * scale

pub enum Shape:
    Circle(i64)
    Square(i64)

protocol Sized:
    def size(self) -> i64:
        pass

class Holder[T]:
    item: T
    origin: Point

    def left(self) -> i64:
        self.origin.norm(scale=self.origin.x)

def total[T: Sized](a: &T, p: &Point) -> i64:
    match Shape.Circle(p.x):
        case Shape.Circle(r):
            unchecked:
                a.size() + p.norm(r)
        case Shape.Square(s):
            s
";

const MEMBERS_MAIN: &str = "from shapes import Point, Shape

def area(s: Shape) -> i64:
    match s:
        case Shape.Circle(r):
            r
        case Shape.Square(w):
            w

def main() -> Result[(), AllocError | IoError]:
    p = Point(x=1)
    n = p.norm(scale=2)
    print(f\"{p.x} {n} {area(Shape.Circle(3))}\"?)?
    Ok(())
";

/// Fields, methods and variants are renamed across files, including their
/// uses in generic bodies that no call instantiates (issue #876).
#[test]
fn a_rename_edits_fields_methods_and_variants() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(
        &root,
        &[
            ("src/shapes.plenty", MEMBERS_SHAPES),
            ("src/main.plenty", MEMBERS_MAIN),
            // A library, so a public member is part of its API.
            ("src/lib.plenty", "pub def one() -> i64:\n    1\n"),
        ],
    );
    let shapes = root.join("src/shapes.plenty");
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let files = |shapes: String, main: String| -> std::collections::BTreeMap<String, String> {
        [
            ("src/main.plenty".to_owned(), main),
            ("src/shapes.plenty".to_owned(), shapes),
        ]
        .into()
    };
    let public = |old: &str, new: &str| {
        vec![format!(
            "renames public `app.shapes.{old}` to `{new}`, so packages outside this workspace that import it must change"
        )]
    };
    // The field, from its declaration and from a use in a generic body.
    for (line, character) in [(1, 8), (23, 25)] {
        let (edit, warnings) = rename_reply(&mut client, &shapes, line, character, "y").unwrap();
        assert_eq!(
            renamed(&root, &edit, &[]),
            files(
                MEMBERS_SHAPES
                    .replace("pub x:", "pub y:")
                    .replace(".x", ".y"),
                MEMBERS_MAIN.replace("x=1", "y=1").replace("p.x", "p.y"),
            ),
            "from {line}:{character}"
        );
        assert_eq!(warnings, public("Point.x", "y"));
    }
    let (edit, warnings) = rename_reply(&mut client, &main, 11, 10, "length").unwrap();
    assert_eq!(
        renamed(&root, &edit, &[]),
        files(
            MEMBERS_SHAPES
                .replace("def norm", "def length")
                .replace(".norm(", ".length("),
            MEMBERS_MAIN.replace(".norm(", ".length("),
        )
    );
    assert_eq!(warnings, public("Point.norm", "length"));
    let (edit, warnings) = rename_reply(&mut client, &main, 4, 19, "Round").unwrap();
    assert_eq!(
        renamed(&root, &edit, &[]),
        files(
            MEMBERS_SHAPES.replace("Circle", "Round"),
            MEMBERS_MAIN.replace("Circle", "Round"),
        )
    );
    assert_eq!(warnings, public("Shape.Circle", "Round"));
    for (path, line, character, new_name, message) in [
        (
            &shapes,
            1,
            8,
            "norm",
            "`Point` already has a member named `norm`",
        ),
        (
            &main,
            4,
            19,
            "Square",
            "`Shape` already has a member named `Square`",
        ),
        (
            &main,
            11,
            10,
            "display",
            "`display` is a method name the language calls implicitly, so a method cannot be renamed to it",
        ),
        (
            &main,
            11,
            10,
            "__eq__",
            "`__eq__` is a method name the language calls implicitly, so a method cannot be renamed to it",
        ),
        (
            &shapes,
            12,
            8,
            "extent",
            "renaming a protocol requirement is not supported: a class or enum satisfies it by declaring a method of that name, which a rename does not edit",
        ),
    ] {
        assert_eq!(
            rename_refused(&mut client, path, line, character, new_name),
            message,
            "{line}:{character} to {new_name}"
        );
    }
    client.exit();
}

/// A member name written on a value whose type an uninstantiated generic
/// body does not tell refuses the rename (issue #876).
#[test]
fn a_member_rename_is_refused_when_a_generic_body_may_use_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(
        &root,
        &[(
            "src/main.plenty",
            "class Point:\n    x: i64\n\nclass Pair[T]:\n    first: T\n\n    def pick(self, points: &list[Point]) -> i64:\n        points[0].x\n\ndef main() -> ():\n    p = Point(x=1)\n",
        )],
    );
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(
        rename_refused(&mut client, &main, 1, 4, "y"),
        "`x` is written at src/main.plenty:8:19 in a generic body that no call instantiates, on a value whose type the editor analysis cannot tell, so a rename cannot tell whether it names `Point.x`"
    );
    client.exit();
}

#[test]
fn a_rename_is_refused_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(
        &root,
        &[
            (
                "src/shapes.plenty",
                "pub class Point:\n    pub x: i64\n\n    pub def norm(self) -> i64:\n        self.x\n\npub enum Shape:\n    Circle(i64)\n    Square(i64)\n\npub def area(width: i64) -> i64:\n    width\n\npub def other() -> i64:\n    1\n",
            ),
            (
                "src/main.plenty",
                "from shapes import Point, area, other\nfrom std.math import gcd\n\ndef main() -> ():\n    p = Point(x=1)\n    n = p.norm()\n    a = area(p.x)\n    g = gcd(4, 6)\n",
            ),
        ],
    );
    let shapes = root.join("src/shapes.plenty");
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(
        prepare_rename(&mut client, &shapes, 3, 17).unwrap_err(),
        "`self` names the receiver, so it cannot be renamed"
    );
    let message = prepare_rename(&mut client, &main, 7, 8).unwrap_err();
    assert!(
        message.starts_with("`gcd` is declared in the standard library"),
        "{message}"
    );
    assert_eq!(
        prepare_rename(&mut client, &main, 1, 10).unwrap_err(),
        "renaming a module is not supported: it means renaming its file"
    );
    assert_eq!(
        prepare_rename(&mut client, &main, 3, 0).unwrap_err(),
        "there is no name to rename here"
    );
    // The new name.
    for (name, message) in [
        ("def", "`def` is a reserved word"),
        ("None", "`None` is a reserved word"),
        (
            "1st",
            "`1st` is not a name: a name is ASCII letters, digits and `_`, and does not start with a digit",
        ),
        (
            "a-b",
            "`a-b` is not a name: a name is ASCII letters, digits and `_`, and does not start with a digit",
        ),
        ("area", "the name is already `area`"),
        (
            "other",
            "`other` already names something at src/main.plenty:1:33",
        ),
    ] {
        assert_eq!(rename_refused(&mut client, &main, 6, 8, name), message);
    }
    // A collision in the declaring module, and in a module importing the
    // declaration without an alias.
    assert_eq!(
        rename_refused(&mut client, &shapes, 13, 9, "Shape"),
        "`Shape` already names something at src/shapes.plenty:7:10"
    );
    assert_eq!(
        rename_refused(&mut client, &shapes, 13, 9, "area"),
        "`area` already names something at src/main.plenty:1:27"
    );
    // `Point` is captured by a local in `main` after the rename.
    let message = rename_refused(&mut client, &main, 6, 8, "p");
    assert!(
        message.starts_with("`p` already names something in scope, so after the rename the package would not check: src/main.plenty:7:9:"),
        "{message}"
    );
    // A package that does not check before the rename.
    write(
        &root.join("src/broken.plenty"),
        "from shapes import area\n\ndef f() -> i64:\n    area(\"x\")\n",
    );
    let message = rename_refused(&mut client, &main, 6, 8, "size");
    assert!(
        message.starts_with(
            "the package does not check, so the rename cannot be verified: src/broken.plenty:4:"
        ),
        "{message}"
    );
    std::fs::remove_file(root.join("src/broken.plenty")).unwrap();
    // An `if const` arm the analysis does not see.
    write(
        &root.join("src/arms.plenty"),
        "from shapes import other\n\nif const target.os == \"linux\":\n    def pick() -> i64:\n        other()\nelse:\n    def pick() -> i64:\n        other()\n",
    );
    assert_eq!(
        rename_refused(&mut client, &shapes, 13, 9, "another"),
        "the `if const` block at src/arms.plenty:3 mentions `other`; a rename cannot check the arms a build does not select"
    );
    std::fs::remove_file(root.join("src/arms.plenty")).unwrap();
    // A file no target or test reads.
    write(
        &root.join("examples/demo.plenty"),
        "def one() -> i64:\n    1\n\ndef main() -> ():\n    a = one()\n",
    );
    assert_eq!(
        rename_refused(&mut client, &root.join("examples/demo.plenty"), 0, 4, "two"),
        "no target or test of the package reads examples/demo.plenty, so a rename cannot find every use"
    );
    // Outside a package, only a local is renamed.
    let outside = tempfile::tempdir().unwrap();
    let single = outside.path().canonicalize().unwrap().join("script.plenty");
    write(
        &single,
        "def one() -> i64:\n    1\n\ndef main() -> ():\n    a = one()\n    b = a\n",
    );
    assert_eq!(
        prepare_rename(&mut client, &single, 0, 4).unwrap_err(),
        "renaming a module-level declaration needs a package: this file has no plenty.toml"
    );
    let (edit, _) = rename_reply(&mut client, &single, 5, 8, "c").unwrap();
    let outside_root = single.parent().unwrap();
    assert_eq!(
        renamed(outside_root, &edit, &[])["script.plenty"],
        "def one() -> i64:\n    1\n\ndef main() -> ():\n    c = one()\n    b = c\n"
    );
    client.exit();
}

#[test]
fn a_local_rename_stays_in_its_scope_and_reads_unsaved_text() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let main_text = "def helper(n: i64) -> i64:\n    n\n\ndef first() -> i64:\n    mut count = 1\n    count = helper(count)\n    count\n\ndef second() -> i64:\n    count = 2\n    count\n\ndef main() -> ():\n    a = first()\n    b = second()\n";
    app_package(&root, &[("src/main.plenty", main_text)]);
    let main = root.join("src/main.plenty");
    let mut client = Client::start_with(
        &dir.path().join("cache"),
        json!([]),
        json!({"documentChanges": true, "changeAnnotationSupport": {"groupsOnLabel": false}}),
    );
    client.open(&main);
    let (edit, _) = rename_reply(&mut client, &main, 5, 19, "total").unwrap();
    // A private declaration of a package without a library changes no
    // public API, so the edit carries no annotation; the open document's
    // version is named.
    assert!(edit.get("changeAnnotations").is_none(), "{edit}");
    assert_eq!(edit["documentChanges"][0]["textDocument"]["version"], 1);
    assert_eq!(
        renamed(&root, &edit, &[])["src/main.plenty"],
        main_text.replace(
            "mut count = 1\n    count = helper(count)\n    count\n",
            "mut total = 1\n    total = helper(total)\n    total\n"
        )
    );
    // The new name would capture `helper` in `first`.
    let message = rename_refused(&mut client, &main, 4, 8, "helper");
    assert!(
        message.starts_with("`helper` already names something in scope, so after the rename the file would not check: 6:14:"),
        "{message}"
    );
    // Unsaved text: a new use of `helper`.
    let unsaved = main_text.replace("    count = 2\n", "    count = helper(2)\n");
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri(&main), "version": 2},
            "contentChanges": [{"text": unsaved}]}),
    );
    let (edit, _) = rename_reply(&mut client, &main, 0, 4, "assist").unwrap();
    assert_eq!(edit["documentChanges"][0]["textDocument"]["version"], 2);
    assert_eq!(
        renamed(&root, &edit, &[(&main, &unsaved)])["src/main.plenty"],
        unsaved.replace("helper", "assist")
    );
    client.exit();
}

#[test]
fn a_public_library_rename_needs_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in REFERENCES_WORKSPACE {
        write(&dir.path().join(path), text);
    }
    let root = dir.path().canonicalize().unwrap();
    let shapes = root.join("tools/src/shapes.plenty");
    let mut client = Client::start_with(
        &dir.path().join("cache"),
        json!([]),
        json!({"documentChanges": true, "changeAnnotationSupport": {"groupsOnLabel": false}}),
    );
    let (edit, warnings) = rename_reply(&mut client, &shapes, 0, 8, "measure").unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        edit["changeAnnotations"],
        json!({"api": {
            "label": "Rename `area` to `measure`",
            "needsConfirmation": true,
            "description": "renames public `tools.shapes.area` to `measure`, so packages outside this workspace that import it must change",
        }})
    );
    for change in edit["documentChanges"].as_array().unwrap() {
        for e in change["edits"].as_array().unwrap() {
            assert_eq!(e["annotationId"], "api", "{e}");
        }
    }
    assert_eq!(renamed(&root, &edit, &[]).len(), 4);
    client.exit();
}

#[test]
fn a_module_alias_and_a_capturing_name_are_refused_with_their_reasons() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(
        &root,
        &[
            ("src/util.plenty", "pub def f() -> i64:\n    1\n"),
            (
                "src/main.plenty",
                "import util as u\n\ndef main() -> ():\n    a = u.f()\n    b = 2\n    c = (a, b)\n",
            ),
        ],
    );
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let refusal = "renaming a module alias (`import m as a`) is not supported";
    // The alias in the import, a use of it, and the module's name.
    for (line, character) in [(0, 15), (3, 8), (0, 8)] {
        let expected = if character == 8 && line == 0 {
            "renaming a module is not supported: it means renaming its file"
        } else {
            refusal
        };
        assert_eq!(
            prepare_rename(&mut client, &main, line, character).unwrap_err(),
            expected
        );
    }
    // `b` is an immutable local, so renaming `a` to it makes `b = 2` a
    // reassignment.
    let message = rename_refused(&mut client, &main, 3, 4, "b");
    assert!(
        message.starts_with(
            "`b` already names something in scope, so after the rename the file would not check: "
        ),
        "{message}"
    );
    client.exit();
}

#[test]
fn text_view_parameter_advice_is_published_at_the_parameter_type() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    write(
        &main,
        "def size(text: str) -> i64:\n    text.byte_len()\ndef main() -> ():\n    pass\n",
    );
    let mut client = Client::start(&dir.path().join("cache"));
    let diagnostics = client.open(&main);
    let warnings = diagnostics.as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0]["severity"], 2);
    assert_eq!(warnings[0]["code"], "text-view-parameter");
    assert_eq!(warnings[0]["range"]["start"]["line"], 0);
    assert_eq!(warnings[0]["range"]["start"]["character"], 15);
    assert!(
        warnings[0]["message"]
            .as_str()
            .unwrap()
            .contains("consider `&strview`")
    );
    client.exit();
}

const FORMAT_GENERIC: &str = "class Point:
    x: i64

    def norm(self, scale: i64) -> i64:
        unchecked:
            self.x * scale

def fmt_generic[T](t: T) -> Result[(), AllocError | IoError]:
    p = Point(x=1)
    print(f\"{p.x} {p.norm(2)}\"?)?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    a = Point(x=2)
    print(f\"{a.x}\"?)?
    Ok(())
";

/// Fields and methods in the f-string fields of a generic body that no call
/// instantiates are found and renamed (issue #876).
#[test]
fn members_in_f_strings_of_uninstantiated_generic_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(&root, &[("src/main.plenty", FORMAT_GENERIC)]);
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    for ((line, character), expected) in [
        (
            (1, 4),
            vec![(1, 4), (5, 17), (8, 14), (9, 15), (13, 14), (14, 15)],
        ),
        ((3, 8), vec![(3, 8), (9, 21)]),
    ] {
        let found: Vec<(u64, u64)> = positions(
            &root,
            &references(&mut client, &main, line, character, true),
        )
        .into_iter()
        .map(|(_, l, c)| (l, c))
        .collect();
        assert_eq!(found, expected, "from {line}:{character}");
    }
    let (edit, warnings) = rename_reply(&mut client, &main, 9, 15, "z").unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        renamed(&root, &edit, &[]),
        [(
            "src/main.plenty".to_owned(),
            FORMAT_GENERIC
                .replace("x: i64", "z: i64")
                .replace(".x", ".z")
                .replace("x=", "z=")
        )]
        .into()
    );
    let (edit, _) = rename_reply(&mut client, &main, 9, 21, "length").unwrap();
    assert_eq!(
        renamed(&root, &edit, &[]),
        [(
            "src/main.plenty".to_owned(),
            FORMAT_GENERIC.replace("norm", "length")
        )]
        .into()
    );
    client.exit();
}

/// A method or protocol requirement whose name is at least as long as its
/// owner's resolves to itself, not to the implicit `self` type anchored at
/// its `def` (issue #876).
#[test]
fn a_requirement_or_method_name_longer_than_its_owner_resolves_to_itself() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let source = "protocol Named:
    def label(self) -> i64:
        pass

class Point:
    x: i64

    def label(self) -> i64:
        self.x

    def describe(self) -> i64:
        self.x

def show[T: Named](t: &T) -> i64:
    t.label()

def main() -> ():
    p = Point(x=1)
    n = show(&p)
    d = p.describe()
";
    app_package(&root, &[("src/main.plenty", source)]);
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    for ((line, character), expected) in [
        ((1, 8), vec![(1, 8)]),
        ((7, 8), vec![(7, 8), (14, 6)]),
        ((10, 8), vec![(10, 8), (19, 10)]),
        ((0, 9), vec![(0, 9), (13, 12)]),
    ] {
        let found: Vec<(u64, u64)> = positions(
            &root,
            &references(&mut client, &main, line, character, true),
        )
        .into_iter()
        .map(|(_, l, c)| (l, c))
        .collect();
        assert_eq!(found, expected, "from {line}:{character}");
    }
    assert_eq!(
        rename_refused(&mut client, &main, 1, 8, "title"),
        "renaming a protocol requirement is not supported: a class or enum satisfies it by declaring a method of that name, which a rename does not edit"
    );
    assert_eq!(
        prepare_rename(&mut client, &main, 10, 8).unwrap()["placeholder"],
        "describe"
    );
    let (edit, _) = rename_reply(&mut client, &main, 10, 8, "summary").unwrap();
    assert_eq!(
        renamed(&root, &edit, &[]),
        [(
            "src/main.plenty".to_owned(),
            source.replace("describe", "summary")
        )]
        .into()
    );
    client.exit();
}

/// Renaming a field or variant of a `@serial` type warns that it changes the
/// encoded name; a method rename does not (issue #876).
#[test]
fn a_serial_field_or_variant_rename_warns_about_encoded_data() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let records = "@serial
pub class Record:
    pub count: i64

    pub def twice(self) -> i64:
        unchecked:
            self.count * 2

@serial
pub enum Kind:
    Small
    Large
";
    let main_source = "from records import Kind, Record

def main() -> ():
    r = Record(count=1)
    n = r.twice()
    k = Kind.Small
";
    app_package(
        &root,
        &[
            ("src/records.plenty", records),
            ("src/main.plenty", main_source),
        ],
    );
    let path = root.join("src/records.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    for (line, character, new_name, expected) in [
        (
            2,
            8,
            "total",
            vec![
                "renames a field of `@serial` `Record`, so data encoded before the rename no longer decodes",
            ],
        ),
        (
            10,
            4,
            "Tiny",
            vec![
                "renames a variant of `@serial` `Kind`, so data encoded before the rename no longer decodes",
            ],
        ),
        (4, 12, "double", vec![]),
    ] {
        let (_, warnings) = rename_reply(&mut client, &path, line, character, new_name).unwrap();
        assert_eq!(warnings, expected, "{new_name}");
    }
    client.exit();
}

const PARAMETERS_SHAPES: &str = "pub class Point:
    pub x: i64

    pub def scaled(self, factor: i64) -> i64:
        unchecked:
            self.x * factor

pub def area(width: i64, height: i64) -> i64:
    unchecked:
        width * height

def helper(count: i64) -> i64:
    count

def twice[T](value: T, times: i64) -> i64:
    helper(count=times)

pub def uses() -> i64:
    p = Point(x=1)
    unchecked:
        area(width=2, height=3) + p.scaled(factor=2) + helper(count=4)
";

const PARAMETERS_MAIN: &str = "from shapes import Point, area

def main() -> ():
    p = Point(x=1)
    a = area(width=2, height=p.x)
    s = p.scaled(factor=3)
";

/// A parameter rename edits its named arguments: across the package for a
/// callable other modules can call, and in its file otherwise (issue #876).
#[test]
fn a_parameter_rename_edits_its_named_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(
        &root,
        &[
            ("src/shapes.plenty", PARAMETERS_SHAPES),
            ("src/main.plenty", PARAMETERS_MAIN),
            // A library, so a public callable is part of its API.
            ("src/lib.plenty", "pub def one() -> i64:\n    1\n"),
        ],
    );
    let shapes = root.join("src/shapes.plenty");
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    let both = |old: &str, new: &str| -> std::collections::BTreeMap<String, String> {
        [
            (
                "src/main.plenty".to_owned(),
                PARAMETERS_MAIN.replace(old, new),
            ),
            (
                "src/shapes.plenty".to_owned(),
                PARAMETERS_SHAPES.replace(old, new),
            ),
        ]
        .into()
    };
    // From a named argument in another module, and from the declaration.
    for (path, line, character) in [(&main, 4, 13), (&shapes, 7, 13)] {
        assert_eq!(
            prepare_rename(&mut client, path, line, character).unwrap()["placeholder"],
            "width"
        );
        let (edit, warnings) = rename_reply(&mut client, path, line, character, "w").unwrap();
        assert_eq!(renamed(&root, &edit, &[]), both("width", "w"));
        assert_eq!(
            warnings,
            [
                "renames parameter `width` of public `app.shapes.area` to `w`, so packages outside this workspace that pass `width=` must change"
            ]
        );
    }
    let (edit, warnings) = rename_reply(&mut client, &shapes, 3, 25, "k").unwrap();
    assert_eq!(renamed(&root, &edit, &[]), both("factor", "k"));
    assert_eq!(
        warnings,
        [
            "renames parameter `factor` of public `app.shapes.Point.scaled` to `k`, so packages outside this workspace that pass `factor=` must change"
        ]
    );
    // Only its module can call `helper`, including from an uninstantiated
    // generic body.
    let (edit, warnings) = rename_reply(&mut client, &shapes, 15, 11, "n").unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        renamed(&root, &edit, &[]),
        [(
            "src/shapes.plenty".to_owned(),
            PARAMETERS_SHAPES.replace("count", "n")
        )]
        .into()
    );
    assert_eq!(
        rename_refused(&mut client, &main, 4, 13, "height"),
        "`area` already has a parameter named `height`"
    );
    assert_eq!(
        rename_refused(&mut client, &shapes, 3, 25, "self"),
        "`Point.scaled` already has a parameter named `self`"
    );
    client.exit();
}

/// A parameter rename is refused when a generic body that no call
/// instantiates passes the name to a callee the analysis cannot tell.
#[test]
fn a_parameter_rename_is_refused_when_a_generic_body_may_name_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    app_package(
        &root,
        &[(
            "src/main.plenty",
            "def helper(count: i64) -> i64:\n    count\n\nclass Holder[T]:\n    item: T\n\n    def go(self) -> i64:\n        self.item.run(count=1)\n\ndef main() -> ():\n    n = helper(count=1)\n",
        )],
    );
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    assert_eq!(
        rename_refused(&mut client, &main, 0, 11, "n"),
        "`count=` is written at main.plenty:8:23 in a generic body that no call instantiates, to a callee the editor analysis cannot tell, so a rename cannot tell whether it names the parameter of `helper`"
    );
    client.exit();
}

/// A parameter of a method, of `__init__` (passed as a constructor keyword),
/// of a generic function, and of a closure is renamed in its file.
#[test]
fn a_parameter_rename_covers_methods_initializers_and_closures() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let text =
        format!("{NAMED_ARGUMENTS}    f = def(value: i64) -> i64:\n        value\n    k = f(4)\n");
    app_package(&root, &[("src/main.plenty", &text)]);
    let main = root.join("src/main.plenty");
    let mut client = Client::start(&dir.path().join("cache"));
    for ((line, character), old, new) in [
        ((14, 16), "start", "initial"),
        ((6, 31), "by", "times"),
        ((16, 23), "first", "head"),
        ((17, 12), "value", "item"),
    ] {
        let (edit, warnings) = rename_reply(&mut client, &main, line, character, new)
            .unwrap_or_else(|e| panic!("from {line}:{character}: {e}"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            renamed(&root, &edit, &[]),
            [("src/main.plenty".to_owned(), text.replace(old, new))].into(),
            "from {line}:{character}"
        );
    }
    assert_eq!(
        rename_refused(&mut client, &main, 10, 12, "second"),
        "`pick` already has a parameter named `second`"
    );
    client.exit();
}
