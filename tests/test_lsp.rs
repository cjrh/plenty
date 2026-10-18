//! `plenty lsp` driven over stdio with scripted messages (issue #155).
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

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
        let result = client.request("initialize", json!({"capabilities": {}}));
        assert_eq!(result["capabilities"]["hoverProvider"], true);
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, message: Value) {
        let body = message.to_string();
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

fn uri(path: &Path) -> String {
    format!("file://{}", path.canonicalize().unwrap().display())
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
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
        file.ends_with("/std-sources/0.1.0/std/math.plenty"),
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
