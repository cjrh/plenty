//! `plenty lsp`: a language server over stdio (issue #155). It answers hover,
//! go to definition, completion, and inferred-type inlay hints from
//! [`crate::ide::analyze`], which checks a file with the compiler's own front
//! end, and publishes that check's errors. Its one refactoring, moving a
//! top-level declaration to another module, is planned by
//! [`crate::ide::relocate`] (issue #822).
//!
//! The protocol needs only JSON-RPC framing and a handful of messages, so they
//! are written here over `serde_json` values rather than through an LSP crate.
use crate::ide::{self, Analysis, File, Location, Reference, Symbol, SymbolKind};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Edits closer together than this are checked once.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Serve one client on stdin and stdout until it exits.
pub fn serve() -> Result<()> {
    let (sender, messages) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        loop {
            // An I/O error or a bad header leaves the stream unframed, so it
            // ends the session like end of input; a body that is not JSON was
            // read whole, so the server answers it and keeps reading.
            let message = read_message(&mut input).ok().flatten();
            let end = message.is_none();
            if sender.send(message).is_err() || end {
                break;
            }
        }
    });
    let mut server = Server::default();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    loop {
        let message = if server.dirty.is_empty() {
            messages.recv().ok()
        } else {
            match messages.recv_timeout(DEBOUNCE) {
                Ok(message) => Some(message),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    for uri in std::mem::take(&mut server.dirty) {
                        server.check(&uri, &mut output)?;
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => None,
            }
        };
        let Some(Some(message)) = message else {
            return Ok(());
        };
        let message = match message {
            Ok(message) => message,
            Err(error) => {
                write_message(
                    &mut output,
                    &json!({"jsonrpc": "2.0", "id": null, "error": {
                        "code": -32700, "message": format!("parse error: {error}")}}),
                )?;
                continue;
            }
        };
        if !server.handle(message, &mut output)? {
            return Ok(());
        }
    }
}

/// Read one framed message: `None` at end of input, and an inner error for a
/// body that is not JSON.
fn read_message(
    input: &mut impl BufRead,
) -> Result<Option<std::result::Result<Value, serde_json::Error>>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    let mut body = vec![0; length.ok_or("message without Content-Length")?];
    input.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)))
}

fn write_message(output: &mut impl Write, message: &Value) -> Result<()> {
    let body = message.to_string();
    write!(output, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    output.flush()?;
    Ok(())
}

/// How the client counts a position's character offset.
#[derive(Clone, Copy, Default, PartialEq)]
enum Encoding {
    Utf8,
    #[default]
    Utf16,
    Utf32,
}

struct Document {
    path: PathBuf,
    text: String,
    /// The client's version, for edits it must apply to this text.
    version: Option<i64>,
}

#[derive(Default)]
struct Server {
    encoding: Encoding,
    documents: HashMap<String, Document>,
    analyses: HashMap<String, Analysis>,
    /// Documents edited since their last check.
    dirty: HashSet<String>,
    shutdown: bool,
    /// Client support for `documentChanges` and change annotations in a
    /// workspace edit.
    document_changes: bool,
    change_annotations: bool,
}

impl Server {
    /// Handle one message; false once the client has said `exit`.
    fn handle(&mut self, message: Value, output: &mut impl Write) -> Result<bool> {
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let params = &message["params"];
        let id = message.get("id").cloned();
        // After `shutdown` the protocol allows only `exit`: other requests get
        // an InvalidRequest error and notifications are dropped.
        if self.shutdown && method != "exit" {
            if id.is_some() {
                write_message(
                    output,
                    &json!({"jsonrpc": "2.0", "id": id, "error": {
                        "code": -32600, "message": format!("{method} after shutdown")}}),
                )?;
            }
            return Ok(true);
        }
        let result = match method.as_str() {
            "initialize" => Some(self.initialize(params)),
            "shutdown" => {
                self.shutdown = true;
                Some(Value::Null)
            }
            "exit" => return Ok(false),
            "textDocument/didOpen" => {
                let document = &params["textDocument"];
                let uri = document["uri"].as_str().unwrap_or_default().to_owned();
                if let Some(path) = uri_path(&uri) {
                    let text = document["text"].as_str().unwrap_or_default().to_owned();
                    let version = document["version"].as_i64();
                    self.documents.insert(
                        uri.clone(),
                        Document {
                            path,
                            text,
                            version,
                        },
                    );
                    self.check(&uri, output)?;
                }
                None
            }
            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                // Full synchronization: the last change holds the whole text.
                let text = params["contentChanges"]
                    .as_array()
                    .and_then(|changes| changes.last())
                    .and_then(|change| change["text"].as_str());
                if let (Some(document), Some(text)) = (self.documents.get_mut(uri), text) {
                    document.text = text.to_owned();
                    document.version = params["textDocument"]["version"].as_i64();
                    self.dirty.insert(uri.to_owned());
                }
                None
            }
            "textDocument/didSave" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                if self.documents.contains_key(uri) {
                    self.dirty.remove(uri);
                    self.check(&uri.to_owned(), output)?;
                }
                None
            }
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                self.documents.remove(uri);
                self.analyses.remove(uri);
                self.dirty.remove(uri);
                None
            }
            "textDocument/inlayHint" => Some(self.inlay_hints(params, output)?),
            "textDocument/hover" => Some(self.query(params, output, Self::hover)?),
            "textDocument/definition" => Some(self.query(params, output, Self::definition)?),
            "textDocument/completion" => Some(self.query(params, output, Self::completion)?),
            "textDocument/codeAction" => Some(self.code_actions(params)),
            "codeAction/resolve" => match self.resolve_action(params) {
                Ok((action, api_changes)) => {
                    // Without change annotations the edit cannot carry the API
                    // changes, so the editor shows them as a message instead.
                    if !self.change_annotations && !api_changes.is_empty() {
                        write_message(
                            output,
                            &json!({"jsonrpc": "2.0", "method": "window/showMessage",
                                "params": {"type": 2, "message": api_changes.join(" ")}}),
                        )?;
                    }
                    Some(action)
                }
                Err(message) => {
                    // RequestFailed: the move was refused and nothing changes.
                    write_message(
                        output,
                        &json!({"jsonrpc": "2.0", "id": id, "error": {
                            "code": -32803, "message": message}}),
                    )?;
                    None
                }
            },
            _ if id.is_some() => {
                write_message(
                    output,
                    &json!({"jsonrpc": "2.0", "id": id, "error": {
                        "code": -32601, "message": format!("unsupported method {method}")}}),
                )?;
                None
            }
            _ => None,
        };
        if let (Some(id), Some(result)) = (id, result) {
            write_message(
                output,
                &json!({"jsonrpc": "2.0", "id": id, "result": result}),
            )?;
        }
        Ok(true)
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let offered = params["capabilities"]["general"]["positionEncodings"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let (encoding, name) = if offered.iter().any(|e| e == "utf-32") {
            (Encoding::Utf32, "utf-32")
        } else if offered.iter().any(|e| e == "utf-8") {
            (Encoding::Utf8, "utf-8")
        } else {
            (Encoding::Utf16, "utf-16")
        };
        self.encoding = encoding;
        let workspace_edit = &params["capabilities"]["workspace"]["workspaceEdit"];
        self.document_changes = workspace_edit["documentChanges"].as_bool() == Some(true);
        self.change_annotations =
            self.document_changes && workspace_edit["changeAnnotationSupport"].is_object();
        json!({
            "capabilities": {
                "positionEncoding": name,
                "textDocumentSync": {"openClose": true, "change": 1, "save": true},
                "hoverProvider": true,
                "inlayHintProvider": {"resolveProvider": false},
                "definitionProvider": true,
                "completionProvider": {"triggerCharacters": ["."]},
                "codeActionProvider": {
                    "codeActionKinds": ["refactor.move"],
                    "resolveProvider": true,
                },
            },
            "serverInfo": {"name": "plenty", "version": env!("CARGO_PKG_VERSION")},
        })
    }

    /// Check a document with every open document's unsaved text, and
    /// publish its errors.
    fn check(&mut self, uri: &String, output: &mut impl Write) -> Result<()> {
        let Some(document) = self.documents.get(uri) else {
            return Ok(());
        };
        let overlay = self
            .documents
            .values()
            .filter_map(|d| Some((d.path.canonicalize().ok()?, d.text.clone())))
            .collect();
        let path = document.path.clone();
        let is_script = crate::script::header(&document.text, &path.display().to_string())
            .is_ok_and(|header| header.manifest.is_some())
            || document
                .text
                .lines()
                .next()
                .is_some_and(|line| line.starts_with("#!") && line.contains("plenty script"));
        let graph = if is_script {
            crate::script::editor_graph(&path, &document.text)
        } else {
            source_graph(&path)
        };
        let mut analysis = ide::analyze(&path, graph.as_ref(), overlay);
        // A file that does not parse yields no names; keep the last ones so
        // queries away from the edit still work.
        if analysis.references.is_empty()
            && let Some(previous) = self.analyses.remove(uri)
        {
            analysis.symbols = previous.symbols;
            analysis.references = previous.references;
            for (file, text) in previous.sources {
                analysis.sources.entry(file).or_insert(text);
            }
        }
        let this = path.canonicalize().unwrap_or(path);
        // A warning elsewhere is published when its own file is checked.
        let warnings = analysis
            .warnings
            .iter()
            .filter(|(_, d)| matches!(&d.file, Some(File::Path(p)) if *p == this))
            .map(|(lint, d)| (Some(*lint), d));
        let diagnostics: Vec<Value> = analysis
            .diagnostics
            .iter()
            .map(|d| (None, d))
            .chain(warnings)
            .map(|(lint, d): (Option<crate::Lint>, _)| {
                let here = matches!(&d.file, Some(File::Path(p)) if *p == this);
                let (line, column, message) = if here {
                    (d.line, d.column, d.message.clone())
                } else {
                    let place = match &d.file {
                        Some(File::Path(p)) => format!("{}:{}:{}: ", p.display(), d.line, d.column),
                        Some(File::Standard(s)) => format!("{s}:{}:{}: ", d.line, d.column),
                        None => String::new(),
                    };
                    (1, 1, format!("{place}{}", d.message))
                };
                // Diagnostics carry a start position; underline the word there.
                let length = document
                    .text
                    .lines()
                    .nth(line as usize - 1)
                    .map(|text| {
                        text.chars()
                            .skip(column as usize - 1)
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .count()
                    })
                    .unwrap_or(0)
                    .max(1) as u32;
                let location = Location {
                    file: File::Path(this.clone()),
                    line,
                    column,
                    length,
                };
                let mut published = json!({
                    "range": self.range(&analysis, &location),
                    // LSP severities: 1 is an error, 2 a warning.
                    "severity": if lint.is_some() { 2 } else { 1 },
                    "source": "plenty",
                    "message": message,
                });
                if let Some(lint) = lint {
                    published["code"] = json!(lint.name());
                }
                published
            })
            .collect();
        write_message(
            output,
            &json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                "params": {"uri": uri, "diagnostics": diagnostics}}),
        )?;
        self.analyses.insert(uri.clone(), analysis);
        Ok(())
    }

    /// Reuse the last check, unless the document has changed since then.
    fn analysis_uri(&mut self, params: &Value, output: &mut impl Write) -> Result<Option<String>> {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if !self.documents.contains_key(&uri) {
            let Some(path) = uri_path(&uri) else {
                return Ok(None);
            };
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            self.documents.insert(
                uri.clone(),
                Document {
                    path,
                    text,
                    version: None,
                },
            );
            self.check(&uri, output)?;
        } else if self.dirty.remove(&uri) || !self.analyses.contains_key(&uri) {
            self.check(&uri, output)?;
        }
        Ok(Some(uri))
    }

    /// Answer a position query from the document's current analysis.
    fn query(
        &mut self,
        params: &Value,
        output: &mut impl Write,
        answer: fn(&Self, &Analysis, &File, u32, u32, &str) -> Result<Value>,
    ) -> Result<Value> {
        let Some(uri) = self.analysis_uri(params, output)? else {
            return Ok(Value::Null);
        };
        let document = &self.documents[&uri];
        let line = params["position"]["line"].as_u64().unwrap_or(0) as u32;
        let units = params["position"]["character"].as_u64().unwrap_or(0) as u32;
        let text = document.text.lines().nth(line as usize).unwrap_or_default();
        let column = self.column(text, units) + 1;
        let file = File::Path(
            document
                .path
                .canonicalize()
                .unwrap_or(document.path.clone()),
        );
        answer(self, &self.analyses[&uri], &file, line + 1, column, text)
    }

    fn inlay_hints(&mut self, params: &Value, output: &mut impl Write) -> Result<Value> {
        let position = |value: &Value| -> Option<(u32, u32)> {
            Some((
                value["line"].as_u64()?.try_into().ok()?,
                value["character"].as_u64()?.try_into().ok()?,
            ))
        };
        let (Some(start), Some(end)) = (
            position(&params["range"]["start"]),
            position(&params["range"]["end"]),
        ) else {
            return Ok(json!([]));
        };
        if start >= end {
            return Ok(json!([]));
        }
        let Some(uri) = self.analysis_uri(params, output)? else {
            return Ok(Value::Null);
        };
        let document = &self.documents[&uri];
        let file = File::Path(
            document
                .path
                .canonicalize()
                .unwrap_or(document.path.clone()),
        );
        let analysis = &self.analyses[&uri];
        let lines: Vec<&str> = document.text.lines().collect();
        let clamp = |(line, character): (u32, u32)| {
            let length = lines
                .get(line as usize)
                .map(|text| text.chars().map(|c| self.width(c)).sum())
                .unwrap_or(0);
            (line, character.min(length))
        };
        let (start, end) = (clamp(start), clamp(end));
        let mut hints = Vec::new();
        for binding in &analysis.inferred_bindings {
            if binding.location.file != file {
                continue;
            }
            let location = &binding.location;
            let line = location.line - 1;
            let Some(text) = lines.get(line as usize) else {
                continue;
            };
            let character: u32 = text
                .chars()
                .take((location.column - 1 + location.length) as usize)
                .map(|c| self.width(c))
                .sum();
            let point = (line, character);
            if start <= point && point < end {
                hints.push((
                    point,
                    json!({
                        "position": {"line": line, "character": character},
                        "label": format!(": {}", binding.ty),
                        "kind": 1,
                        "paddingLeft": false,
                        "paddingRight": false,
                    }),
                ));
            }
        }
        hints.sort_by_key(|(point, _)| *point);
        Ok(Value::Array(
            hints.into_iter().map(|(_, hint)| hint).collect(),
        ))
    }

    fn hover(
        &self,
        analysis: &Analysis,
        file: &File,
        line: u32,
        column: u32,
        _: &str,
    ) -> Result<Value> {
        let Some(reference) = analysis.reference_at(file, line, column) else {
            return Ok(Value::Null);
        };
        let symbol = &analysis.symbols[reference.symbol];
        Ok(json!({
            "contents": {"kind": "markdown", "value": hover_text(symbol, reference)},
            "range": self.range(analysis, &reference.location),
        }))
    }

    fn definition(
        &self,
        analysis: &Analysis,
        file: &File,
        line: u32,
        column: u32,
        _: &str,
    ) -> Result<Value> {
        let Some(reference) = analysis.reference_at(file, line, column) else {
            return Ok(Value::Null);
        };
        let Some(location) = &analysis.symbols[reference.symbol].location else {
            return Ok(Value::Null);
        };
        let path = match &location.file {
            File::Path(path) => path.clone(),
            File::Standard(relative) => {
                let Some(text) = analysis.sources.get(&location.file) else {
                    return Ok(Value::Null);
                };
                materialize(relative, text)?
            }
        };
        Ok(json!({"uri": path_uri(&path), "range": self.range(analysis, location)}))
    }

    fn completion(
        &self,
        analysis: &Analysis,
        file: &File,
        line: u32,
        column: u32,
        text: &str,
    ) -> Result<Value> {
        let before: String = text.chars().take(column as usize - 1).collect();
        let typed = before
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .count();
        let stem = &before[..before.len()
            - before
                .chars()
                .rev()
                .take(typed)
                .map(char::len_utf8)
                .sum::<usize>()];
        let items = match stem.strip_suffix('.') {
            Some(receiver) => members(analysis, file, line, receiver),
            None => names(analysis, file, line),
        };
        Ok(json!({"isIncomplete": false, "items": items}))
    }

    /// One unresolved `refactor.move` action per other module of the
    /// package, for the declaration at the range's start. Planning the move
    /// checks the whole package, so it waits for `codeAction/resolve`.
    fn code_actions(&self, params: &Value) -> Value {
        let wanted = params["context"]["only"].as_array().is_none_or(|kinds| {
            kinds
                .iter()
                .filter_map(Value::as_str)
                .any(|k| "refactor.move".starts_with(k) || k.starts_with("refactor.move"))
        });
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let Some(path) = uri_path(uri) else {
            return json!([]);
        };
        let document = self.documents.get(uri);
        let text = match document {
            Some(document) => document.text.clone(),
            None => std::fs::read_to_string(&path).unwrap_or_default(),
        };
        let version = document.and_then(|d| d.version);
        let line = params["range"]["start"]["line"].as_u64().unwrap_or(0) as u32 + 1;
        let candidate = wanted
            .then(|| ide::relocate::candidates(&path, &text, line))
            .flatten();
        let Some(candidate) = candidate else {
            return json!([]);
        };
        let actions: Vec<Value> = candidate
            .destinations
            .iter()
            .map(|(module, file)| {
                json!({
                    "title": format!("Move `{}` to module `{module}`", candidate.name),
                    "kind": "refactor.move",
                    // The name and version let resolve refuse an action
                    // offered for text that has changed since.
                    "data": {"uri": uri, "line": line, "destination": path_uri(file),
                        "name": candidate.name, "version": version},
                })
            })
            .collect();
        json!(actions)
    }

    /// The action's workspace edit, or why the move is refused.
    fn resolve_action(&self, action: &Value) -> std::result::Result<(Value, Vec<String>), String> {
        let data = &action["data"];
        let path = data["uri"].as_str().and_then(uri_path);
        let destination = data["destination"].as_str().and_then(uri_path);
        let line = data["line"].as_u64().and_then(|l| u32::try_from(l).ok());
        let (Some(path), Some(destination), Some(line)) = (path, destination, line) else {
            return Err("this code action has no move to resolve".into());
        };
        let document = data["uri"].as_str().and_then(|uri| self.documents.get(uri));
        let text = match document {
            Some(document) => document.text.clone(),
            None => std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
        };
        let current = ide::relocate::candidates(&path, &text, line).map(|c| c.name);
        if document.and_then(|d| d.version) != data["version"].as_i64()
            || current.as_deref() != data["name"].as_str()
        {
            return Err(
                "the file has changed since this move was offered; ask for code actions again"
                    .into(),
            );
        }
        let overlay: HashMap<PathBuf, String> = self
            .documents
            .values()
            .filter_map(|d| Some((d.path.canonicalize().ok()?, d.text.clone())))
            .collect();
        let plan = ide::relocate::plan(&path, line, &destination, &overlay)?;
        let annotation = "api";
        let mut changes = serde_json::Map::new();
        let mut document_changes = vec![];
        for file in &plan.files {
            // An open document's edits name the client's URI, which can
            // differ from the canonical path through a symbolic link.
            let open = self
                .documents
                .iter()
                .find(|(_, d)| d.path.canonicalize().ok().as_ref() == Some(&file.path));
            let document = open.map(|(_, d)| d);
            let text = match document {
                Some(document) => document.text.clone(),
                None => std::fs::read_to_string(&file.path).map_err(|e| e.to_string())?,
            };
            let lines: Vec<&str> = text.split('\n').collect();
            let units = |line: u32, column: u32| -> u32 {
                lines
                    .get(line as usize)
                    .map(|l| l.chars().take(column as usize).map(|c| self.width(c)).sum())
                    .unwrap_or(0)
            };
            let edits: Vec<Value> = file
                .edits
                .iter()
                .map(|e| {
                    let mut edit = json!({
                        "range": {
                            "start": {"line": e.line, "character": units(e.line, e.column)},
                            "end": {"line": e.end_line, "character": units(e.end_line, e.end_column)},
                        },
                        "newText": e.text,
                    });
                    if self.change_annotations && !plan.api_changes.is_empty() {
                        edit["annotationId"] = json!(annotation);
                    }
                    edit
                })
                .collect();
            let uri = open.map_or_else(|| path_uri(&file.path), |(uri, _)| uri.clone());
            if self.document_changes {
                document_changes.push(json!({
                    "textDocument": {"uri": uri, "version": document.and_then(|d| d.version)},
                    "edits": edits,
                }));
            } else {
                changes.insert(uri, json!(edits));
            }
        }
        let edit = if self.document_changes {
            let mut edit = json!({"documentChanges": document_changes});
            if self.change_annotations && !plan.api_changes.is_empty() {
                edit["changeAnnotations"] = json!({annotation: {
                    "label": format!("Move `{}` to `{}`", plan.name, plan.destination),
                    "needsConfirmation": true,
                    "description": plan.api_changes.join(" "),
                }});
            }
            edit
        } else {
            json!({"changes": changes})
        };
        let mut resolved = action.clone();
        resolved["edit"] = edit;
        Ok((resolved, plan.api_changes))
    }

    /// The character offset of `units` client units into `text`.
    fn column(&self, text: &str, units: u32) -> u32 {
        let mut counted = 0;
        for (i, c) in text.chars().enumerate() {
            if counted >= units {
                return i as u32;
            }
            counted += self.width(c);
        }
        text.chars().count() as u32
    }

    fn width(&self, c: char) -> u32 {
        match self.encoding {
            Encoding::Utf8 => c.len_utf8() as u32,
            Encoding::Utf16 => c.len_utf16() as u32,
            Encoding::Utf32 => 1,
        }
    }

    fn range(&self, analysis: &Analysis, location: &Location) -> Value {
        let text = match &location.file {
            File::Path(path) => self
                .documents
                .values()
                .find(|d| d.path.canonicalize().ok().as_ref() == Some(path))
                .map(|d| d.text.clone())
                .or_else(|| analysis.sources.get(&location.file).map(|t| t.to_string())),
            File::Standard(_) => analysis.sources.get(&location.file).map(|t| t.to_string()),
        }
        .unwrap_or_default();
        let line_text = text
            .lines()
            .nth(location.line as usize - 1)
            .unwrap_or_default();
        let units = |column: u32| -> u32 {
            line_text
                .chars()
                .take(column as usize - 1)
                .map(|c| self.width(c))
                .sum()
        };
        let line = location.line - 1;
        json!({
            "start": {"line": line, "character": units(location.column)},
            "end": {"line": line, "character": units(location.column + location.length)},
        })
    }
}

fn hover_text(symbol: &Symbol, reference: &Reference) -> String {
    let mut declaration = symbol.declaration.clone();
    if matches!(symbol.kind, SymbolKind::Local | SymbolKind::Parameter)
        && !declaration.contains(':')
        && let Some(ty) = &reference.ty
    {
        declaration = format!("{declaration}: {ty}");
    }
    let mut text = format!("```plenty\n{declaration}\n```");
    let qualified = !matches!(symbol.kind, SymbolKind::Local | SymbolKind::Parameter);
    if qualified && symbol.name.contains('.') {
        text.push_str(&format!("\n\n`{}`", symbol.name));
    }
    if !symbol.doc.is_empty() {
        text.push_str("\n\n---\n\n");
        text.push_str(&dedent(&symbol.doc));
    }
    text
}

/// A docstring's lines without the indentation its continuation lines share.
fn dedent(doc: &str) -> String {
    let mut lines = doc.lines();
    let first = lines.next().unwrap_or_default().trim();
    let rest: Vec<&str> = lines.collect();
    let indent = rest
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    std::iter::once(first.to_owned())
        .chain(
            rest.iter()
                .map(|l| l.get(indent..).unwrap_or("").trim_end().to_owned()),
        )
        .collect::<Vec<_>>()
        .join("\n")
}

fn completion_kind(kind: SymbolKind) -> u32 {
    match kind {
        SymbolKind::Function => 3,
        SymbolKind::Method => 2,
        SymbolKind::Field => 5,
        SymbolKind::Local | SymbolKind::Parameter => 6,
        SymbolKind::Class => 7,
        SymbolKind::Protocol => 8,
        SymbolKind::Module => 9,
        SymbolKind::Enum => 13,
        SymbolKind::Variant => 20,
        SymbolKind::Constant => 21,
        SymbolKind::TypeAlias => 25,
    }
}

fn item(label: &str, symbol: &Symbol) -> Value {
    let summary = symbol.doc.lines().next().unwrap_or_default().trim();
    json!({
        "label": label,
        "kind": completion_kind(symbol.kind),
        "detail": symbol.declaration,
        "documentation": summary,
    })
}

fn short(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// Fields, methods, variants, or module members after `receiver.`.
fn members(analysis: &Analysis, file: &File, line: u32, receiver: &str) -> Vec<Value> {
    let written: String = receiver
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let last = short(&written);
    let column = receiver.chars().count() as u32 - last.chars().count() as u32 + 1;
    // The receiver as the analysis resolved it, or a module path written out.
    let owner = match analysis.reference_at(file, line, column) {
        Some(reference) => {
            let symbol = &analysis.symbols[reference.symbol];
            match symbol.kind {
                SymbolKind::Local | SymbolKind::Parameter | SymbolKind::Field => {
                    let ty = reference.ty.clone().or_else(|| {
                        symbol
                            .declaration
                            .split_once(": ")
                            .map(|(_, ty)| ty.to_owned())
                    });
                    match ty {
                        Some(ty) => class_name(&ty),
                        None => return vec![],
                    }
                }
                _ => symbol.name.clone(),
            }
        }
        None => written.clone(),
    };
    // A module's members are the declarations in its file.
    let module = analysis
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Module && s.name == owner)
        .and_then(|s| s.location.as_ref());
    if let Some(module) = module {
        return analysis
            .symbols
            .iter()
            .filter(|s| {
                s.location.as_ref().is_some_and(|l| l.file == module.file)
                    && matches!(
                        s.kind,
                        SymbolKind::Function
                            | SymbolKind::Class
                            | SymbolKind::Enum
                            | SymbolKind::Protocol
                            | SymbolKind::Constant
                            | SymbolKind::TypeAlias
                    )
            })
            .map(|s| item(short(&s.name), s))
            .collect();
    }
    let prefix = format!("{owner}.");
    let mut seen = HashSet::new();
    analysis
        .symbols
        .iter()
        .filter(|s| {
            s.name
                .strip_prefix(&prefix)
                .is_some_and(|rest| !rest.contains('.') && !rest.starts_with("__"))
                && !matches!(s.kind, SymbolKind::Local | SymbolKind::Parameter)
        })
        .filter(|s| seen.insert(s.name.clone()))
        .map(|s| item(short(&s.name), s))
        .collect()
}

/// The declaration a type's name refers to: `&mut Box[i64]` is `Box`.
fn class_name(ty: &str) -> String {
    let ty = ty.trim_start_matches('&').trim_start_matches("mut ").trim();
    ty.split('[').next().unwrap_or(ty).to_owned()
}

/// Names that can be written at a position: this file's declarations and
/// locals bound above it, names it imports or uses without qualification,
/// and keywords.
fn names(analysis: &Analysis, file: &File, line: u32) -> Vec<Value> {
    let text = analysis.sources.get(file);
    let unqualified = |r: &Reference| -> bool {
        let Some(text) = text else { return false };
        let Some(line) = text.lines().nth(r.location.line as usize - 1) else {
            return false;
        };
        let before: String = line.chars().take(r.location.column as usize - 1).collect();
        !before.trim_end().ends_with('.')
    };
    // Locals are offered from the function the position is in: the last
    // one declared above it in this file.
    let function = analysis
        .symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .filter_map(|s| s.location.as_ref())
        .filter(|l| &l.file == file && l.line <= line)
        .map(|l| l.line)
        .max()
        .unwrap_or(0);
    let mut offered = HashSet::new();
    let mut items = vec![];
    for reference in analysis
        .references
        .iter()
        .filter(|r| &r.location.file == file)
    {
        let symbol = &analysis.symbols[reference.symbol];
        let label = short(&symbol.name);
        let visible = match symbol.kind {
            SymbolKind::Local | SymbolKind::Parameter => symbol
                .location
                .as_ref()
                .is_some_and(|l| l.line >= function && l.line <= line),
            SymbolKind::Method | SymbolKind::Field | SymbolKind::Variant | SymbolKind::Module => {
                false
            }
            _ => unqualified(reference),
        };
        if visible && offered.insert(label.to_owned()) {
            items.push(item(label, symbol));
        }
    }
    for symbol in &analysis.symbols {
        let here = symbol.location.as_ref().is_some_and(|l| &l.file == file);
        let top = !matches!(
            symbol.kind,
            SymbolKind::Local
                | SymbolKind::Parameter
                | SymbolKind::Method
                | SymbolKind::Field
                | SymbolKind::Variant
                | SymbolKind::Module
        );
        if here && top && offered.insert(short(&symbol.name).to_owned()) {
            items.push(item(short(&symbol.name), symbol));
        }
    }
    items.extend(
        crate::frontend::KEYWORDS
            .iter()
            .filter(|k| !crate::frontend::REJECTED_KEYWORDS.contains(k))
            .filter(|k| offered.insert(k.to_string()))
            .map(|k| json!({"label": k, "kind": 14})),
    );
    items
}

/// The package graph a file belongs to, read without fetching or locking;
/// None outside a package, where imports resolve from the file's directory.
fn source_graph(path: &Path) -> Option<crate::project::SourceGraph> {
    let path = path.canonicalize().ok()?;
    let directory = crate::project::discover(path.parent()?).ok()?;
    let options = crate::project::ResolveOptions {
        locked: true,
        offline: true,
        ..Default::default()
    };
    let project = crate::project::Project::load(&directory, &options).ok()?;
    let index = project.packages.iter().position(|p| {
        p.directory
            .join("src")
            .canonicalize()
            .is_ok_and(|src| path.starts_with(src))
    })?;
    Some(project.source_graph(index))
}

/// A read-only copy of an embedded `std` module, for an editor to open.
/// Copies live below `$PLENTY_CACHE_DIR`, `$XDG_CACHE_HOME/plenty`, or
/// `~/.cache/plenty`, in `std-sources/VERSION/`.
fn materialize(relative: &str, text: &str) -> Result<PathBuf> {
    let base = std::env::var_os("PLENTY_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .filter(|v| !v.is_empty())
                .map(|x| PathBuf::from(x).join("plenty"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".cache").join("plenty"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("plenty"));
    let path = base
        .join("std-sources")
        .join(env!("CARGO_PKG_VERSION"))
        .join(relative);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
        std::fs::create_dir_all(path.parent().ok_or("std module path")?)?;
        if let Ok(metadata) = std::fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            std::fs::set_permissions(&path, permissions)?;
        }
        std::fs::write(&path, text)?;
        let mut permissions = std::fs::metadata(&path)?.permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions)?;
    }
    Ok(path)
}

fn uri_path(uri: &str) -> Option<PathBuf> {
    let path = uri.strip_prefix("file://")?;
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(decoded).ok()?))
}

fn path_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ide::InferredBinding;

    #[test]
    fn inlay_hint_positions_and_ranges_use_client_code_units() {
        // The current language has ASCII binding names. Exercise the protocol
        // conversion separately with non-ASCII text before a synthetic binding.
        for (encoding, character) in [
            (Encoding::Utf8, 6),
            (Encoding::Utf16, 4),
            (Encoding::Utf32, 3),
        ] {
            let mut server = Server {
                encoding,
                ..Server::default()
            };
            let uri = "file:///inlay-encoding.plenty".to_owned();
            let path = uri_path(&uri).unwrap();
            let text = "𐐀 x".to_owned();
            server.documents.insert(
                uri.clone(),
                Document {
                    path: path.clone(),
                    text,
                    version: None,
                },
            );
            server.analyses.insert(
                uri.clone(),
                Analysis {
                    inferred_bindings: vec![InferredBinding {
                        location: Location {
                            file: File::Path(path),
                            line: 1,
                            column: 3,
                            length: 1,
                        },
                        ty: "i64".into(),
                    }],
                    ..Analysis::default()
                },
            );
            let mut output = Vec::new();
            let request = |start, end| {
                json!({"textDocument": {"uri": uri}, "range": {
                    "start": {"line": 0, "character": start}, "end": {"line": 1, "character": end}
                }})
            };
            let hints = server
                .inlay_hints(&request(character, 0), &mut output)
                .unwrap();
            assert_eq!(
                hints[0]["position"],
                json!({"line": 0, "character": character})
            );
            // Oversized character offsets clamp to end of line.
            assert_eq!(
                server.inlay_hints(&request(100, 0), &mut output).unwrap(),
                hints
            );
            assert!(output.is_empty(), "cached queries must not recheck");
        }
    }
}
