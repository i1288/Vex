//! A small Language Server Protocol client for type checking: Vex starts a
//! language server per language (rust-analyzer, pyright, clangd, gopls, …),
//! keeps it told about open files, and collects the diagnostics it publishes.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};

use serde_json::{json, Value};

use crate::buffer::Pos;
use crate::diagnostics::{Diagnostic, Severity};

/// A language a server can handle: its LSP language id, the server it
/// belongs to (one server per key), and commands to try, in order.
pub struct Language {
    pub id: &'static str,
    pub server: &'static str,
    pub candidates: &'static [&'static str],
}

/// Server key, display name and how to install it (for the settings page).
pub const SERVERS: &[(&str, &str, &str)] = &[
    ("rust", "Rust", "rustup component add rust-analyzer"),
    ("python", "Python", "pip install basedpyright (or npm i -g pyright)"),
    ("typescript", "JavaScript / TypeScript", "npm i -g typescript typescript-language-server"),
    ("c", "C / C++", "install clangd from your package manager"),
    ("go", "Go", "go install golang.org/x/tools/gopls@latest"),
    ("bash", "Bash", "npm i -g bash-language-server"),
    ("java", "Java", "install jdtls (Eclipse JDT Language Server)"),
];

pub fn language_for(ext: &str) -> Option<Language> {
    let (id, server, candidates): (&str, &str, &[&str]) = match ext {
        "rs" => ("rust", "rust", &["rust-analyzer"]),
        "py" | "pyw" => ("python", "python", &["basedpyright-langserver --stdio", "pyright-langserver --stdio", "pylsp"]),
        "js" | "mjs" | "cjs" => ("javascript", "typescript", &["typescript-language-server --stdio"]),
        "jsx" => ("javascriptreact", "typescript", &["typescript-language-server --stdio"]),
        "ts" | "mts" | "cts" => ("typescript", "typescript", &["typescript-language-server --stdio"]),
        "tsx" => ("typescriptreact", "typescript", &["typescript-language-server --stdio"]),
        "c" | "h" => ("c", "c", &["clangd"]),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => ("cpp", "c", &["clangd"]),
        "go" => ("go", "go", &["gopls"]),
        "sh" | "bash" => ("shellscript", "bash", &["bash-language-server start"]),
        "java" => ("java", "java", &["jdtls"]),
        _ => return None,
    };
    Some(Language { id, server, candidates })
}

/// Folders searched besides PATH: apps started from a desktop menu often
/// don't have these on their PATH.
fn extra_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"].iter().map(PathBuf::from).collect();
    if let Some(h) = home {
        for d in [".cargo/bin", ".local/bin", "go/bin", ".npm-global/bin", ".bun/bin", "bin"] {
            dirs.push(h.join(d));
        }
    }
    dirs
}

fn search_path() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for d in extra_dirs() {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}

/// Full path of `program`, searching PATH and the usual install folders.
pub fn find_program(program: &str) -> Option<PathBuf> {
    if program.contains('/') || program.contains('\\') {
        return Path::new(program).is_file().then(|| PathBuf::from(program));
    }
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat", ""] } else { &[""] };
    search_path().into_iter().find_map(|dir| {
        exts.iter().map(|e| dir.join(format!("{program}{e}"))).find(|p| p.is_file())
    })
}

/// The command line to run for `server`: the user's override if set (empty
/// means "don't run one"), else the first installed candidate.
pub fn resolve_command(lang: &Language, overrides: &BTreeMap<String, String>) -> Option<Vec<String>> {
    if let Some(custom) = overrides.get(lang.server) {
        let parts: Vec<String> = custom.split_whitespace().map(String::from).collect();
        return (!parts.is_empty()).then_some(parts);
    }
    lang.candidates.iter().find_map(|c| {
        let parts: Vec<String> = c.split_whitespace().map(String::from).collect();
        find_program(&parts[0]).map(|_| parts)
    })
}

pub fn path_to_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from(if s.starts_with('/') { "file://" } else { "file:///" });
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => out.push(b as char),
            b':' if out.len() <= 10 => out.push(':'), // Windows drive letter
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut bytes = Vec::new();
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&raw[i + 1..i + 3]).ok()?, 16) {
                bytes.push(b);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    let s = String::from_utf8(bytes).ok()?;
    // file:///C:/x on Windows
    let s = if s.len() > 2 && s.as_bytes()[0] == b'/' && s.as_bytes()[2] == b':' { s[1..].to_string() } else { s };
    Some(PathBuf::from(s))
}

/// An LSP position (UTF-16 code units) as a char column on `line`.
pub fn utf16_to_col(line: &[char], units: usize) -> usize {
    let mut seen = 0;
    for (i, c) in line.iter().enumerate() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16();
    }
    line.len()
}

/// What the app hears from servers.
pub enum Event {
    Diagnostics { path: PathBuf, diagnostics: Vec<RawDiagnostic> },
    /// A server's progress text ("Indexing 40%"), or None when it's done.
    Progress { server: String, text: Option<String> },
    Exited { server: String },
}

/// A diagnostic with LSP (UTF-16) positions, converted once the buffer is known.
pub struct RawDiagnostic {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub severity: Severity,
    pub message: String,
}

impl RawDiagnostic {
    pub fn to_diagnostic(&self, lines: &[Vec<char>]) -> Diagnostic {
        let pos = |(l, c): (usize, usize)| {
            let line = lines.get(l).map_or(&[][..], |v| v.as_slice());
            Pos::new(l, utf16_to_col(line, c))
        };
        Diagnostic { start: pos(self.start), end: pos(self.end), message: self.message.clone(), severity: self.severity }
    }
}

struct Server {
    child: Child,
    outgoing: Sender<Value>,
    ready: bool,
    /// Messages to send once the server has answered `initialize`.
    queued: Vec<Value>,
    /// Open documents: buffer id → (uri, version sent).
    docs: HashMap<u64, (String, u64)>,
}

impl Server {
    fn send(&mut self, msg: Value) {
        if self.ready || msg.get("method").and_then(Value::as_str) == Some("initialize") {
            let _ = self.outgoing.send(msg);
        } else {
            self.queued.push(msg);
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// All running language servers.
pub struct Manager {
    servers: HashMap<String, Server>,
    /// Servers that could not be started; not retried until settings change.
    failed: HashSet<String>,
    events_tx: Sender<(String, Value)>,
    events: Receiver<(String, Value)>,
    notify: std::sync::Arc<dyn Fn() + Send + Sync>,
}

/// A buffer as the manager needs to see it.
pub struct Doc<'a> {
    pub id: u64,
    pub path: &'a Path,
    pub version: u64,
    pub text: &'a dyn Fn() -> String,
    /// The text matches the file on disk.
    pub saved: bool,
}

impl Manager {
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> Self {
        let (events_tx, events) = mpsc::channel();
        Self { servers: HashMap::new(), failed: HashSet::new(), events_tx, events, notify: std::sync::Arc::new(notify) }
    }

    pub fn is_open(&self, id: u64) -> bool {
        self.servers.values().any(|s| s.docs.contains_key(&id))
    }

    /// Whether the server has an older version of this document than `version`.
    pub fn is_stale(&self, id: u64, version: u64) -> bool {
        self.servers.values().any(|s| s.docs.get(&id).is_some_and(|(_, v)| *v != version))
    }

    pub fn running(&self) -> Vec<String> {
        self.servers.keys().cloned().collect()
    }

    /// Stop every server (type checking turned off, or settings changed).
    pub fn shutdown_all(&mut self) {
        self.servers.clear();
        self.failed.clear();
    }

    fn start(&mut self, key: &str, command: &[String], root: &Path) -> bool {
        let Some(program) = find_program(&command[0]) else { return false };
        let mut path_var: Vec<PathBuf> = search_path();
        path_var.dedup();
        let mut cmd = Command::new(program);
        cmd.args(&command[1..])
            .current_dir(root)
            .env("PATH", std::env::join_paths(path_var).unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let Ok(mut child) = cmd.spawn() else { return false };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return false };

        // Writer thread: the UI never waits on a server's input pipe.
        let (outgoing, out_rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            let mut stdin = stdin;
            for msg in out_rx {
                let body = msg.to_string();
                if write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).and_then(|_| stdin.flush()).is_err() {
                    break;
                }
            }
        });
        // Reader thread.
        let tx = self.events_tx.clone();
        let notify = self.notify.clone();
        let name = key.to_string();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut length = None;
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => {
                            let _ = tx.send((name.clone(), json!({"vex/exited": true})));
                            notify();
                            return;
                        }
                        Ok(_) => {}
                    }
                    let l = line.trim();
                    if l.is_empty() {
                        break;
                    }
                    if let Some(v) = l.strip_prefix("Content-Length:") {
                        length = v.trim().parse::<usize>().ok();
                    }
                }
                let Some(len) = length else { continue };
                let mut body = vec![0; len];
                if reader.read_exact(&mut body).is_err() {
                    continue;
                }
                if let Ok(msg) = serde_json::from_slice::<Value>(&body) {
                    let _ = tx.send((name.clone(), msg));
                    notify();
                }
            }
        });

        let root_uri = path_to_uri(root);
        let folder_name = root.file_name().map_or("workspace".into(), |n| n.to_string_lossy().into_owned());
        let mut server = Server { child, outgoing, ready: false, queued: Vec::new(), docs: HashMap::new() };
        server.send(json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "clientInfo": {"name": "Vex", "version": env!("CARGO_PKG_VERSION")},
                "rootUri": root_uri,
                "workspaceFolders": [{"uri": root_uri, "name": folder_name}],
                "capabilities": {
                    "general": {"positionEncodings": ["utf-16"]},
                    "window": {"workDoneProgress": true},
                    "workspace": {"configuration": true, "workspaceFolders": true},
                    "textDocument": {
                        "synchronization": {"didSave": true, "dynamicRegistration": false},
                        "publishDiagnostics": {"relatedInformation": false, "versionSupport": true}
                    }
                }
            }
        }));
        self.servers.insert(key.to_string(), server);
        true
    }

    /// Make sure `doc` is open in its server with its latest text.
    pub fn sync(&mut self, doc: &Doc, lang: &Language, root: &Path, overrides: &BTreeMap<String, String>) {
        if self.failed.contains(lang.server) {
            return;
        }
        if !self.servers.contains_key(lang.server) {
            let started = resolve_command(lang, overrides).is_some_and(|cmd| self.start(lang.server, &cmd, root));
            if !started {
                self.failed.insert(lang.server.to_string());
                return;
            }
        }
        let server = self.servers.get_mut(lang.server).expect("just started");
        match server.docs.get(&doc.id).cloned() {
            Some((_, v)) if v == doc.version => {}
            Some((uri, _)) => {
                server.docs.insert(doc.id, (uri.clone(), doc.version));
                server.notify(
                    "textDocument/didChange",
                    json!({"textDocument": {"uri": uri, "version": doc.version}, "contentChanges": [{"text": (doc.text)()}]}),
                );
            }
            None => {
                let uri = path_to_uri(doc.path);
                server.docs.insert(doc.id, (uri.clone(), doc.version));
                server.notify(
                    "textDocument/didOpen",
                    json!({"textDocument": {"uri": uri, "languageId": lang.id, "version": doc.version, "text": (doc.text)()}}),
                );
                // Some servers (rust-analyzer) run their full check only on save;
                // an unmodified file counts as just saved.
                if doc.saved {
                    server.notify("textDocument/didSave", json!({"textDocument": {"uri": uri}}));
                }
            }
        }
    }

    pub fn did_save(&mut self, id: u64) {
        for server in self.servers.values_mut() {
            if let Some((uri, _)) = server.docs.get(&id).cloned() {
                server.notify("textDocument/didSave", json!({"textDocument": {"uri": uri}}));
            }
        }
    }

    /// Tell servers about buffers that were closed.
    pub fn close_missing(&mut self, open_ids: &HashSet<u64>) {
        for server in self.servers.values_mut() {
            let closed: Vec<(u64, String)> =
                server.docs.iter().filter(|(id, _)| !open_ids.contains(id)).map(|(id, (uri, _))| (*id, uri.clone())).collect();
            for (id, uri) in closed {
                server.docs.remove(&id);
                server.notify("textDocument/didClose", json!({"textDocument": {"uri": uri}}));
            }
        }
    }

    /// Handle everything servers sent since the last call.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        while let Ok((key, msg)) = self.events.try_recv() {
            if msg.get("vex/exited").is_some() {
                self.servers.remove(&key);
                self.failed.insert(key.clone());
                events.push(Event::Exited { server: key });
                continue;
            }
            let Some(server) = self.servers.get_mut(&key) else { continue };
            let method = msg.get("method").and_then(Value::as_str);
            let id = msg.get("id").cloned();
            match (method, id) {
                // The answer to `initialize`: the server is ready.
                (None, Some(id)) if id == json!(0) => {
                    server.ready = true;
                    let _ = server.outgoing.send(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
                    for m in std::mem::take(&mut server.queued) {
                        let _ = server.outgoing.send(m);
                    }
                }
                (None, Some(_)) => {} // answers to requests we don't make
                // Requests from the server need an answer.
                (Some(method), Some(id)) => {
                    let result = match method {
                        "workspace/configuration" => {
                            let n = msg["params"]["items"].as_array().map_or(0, Vec::len);
                            json!(vec![Value::Null; n])
                        }
                        "workspace/workspaceFolders" | "client/registerCapability" | "client/unregisterCapability"
                        | "window/workDoneProgress/create" | "window/showMessageRequest" => Value::Null,
                        _ => {
                            let _ = server.outgoing.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "not supported"}}));
                            continue;
                        }
                    };
                    let _ = server.outgoing.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
                }
                (Some("textDocument/publishDiagnostics"), None) => {
                    let params = &msg["params"];
                    let Some(path) = params["uri"].as_str().and_then(uri_to_path) else { continue };
                    let list = params["diagnostics"].as_array().cloned().unwrap_or_default();
                    events.push(Event::Diagnostics { path, diagnostics: list.iter().filter_map(raw_diagnostic).collect() });
                }
                (Some("$/progress"), None) => {
                    let value = &msg["params"]["value"];
                    let text = match value["kind"].as_str() {
                        Some("end") => None,
                        _ => {
                            let title = value["title"].as_str().unwrap_or("");
                            let message = value["message"].as_str().unwrap_or("");
                            let pct = value["percentage"].as_u64().map(|p| format!(" {p}%")).unwrap_or_default();
                            Some(format!("{title} {message}{pct}").trim().to_string())
                        }
                    };
                    events.push(Event::Progress { server: key.clone(), text });
                }
                _ => {} // logMessage, showMessage, telemetry…
            }
        }
        events
    }
}

fn raw_diagnostic(d: &Value) -> Option<RawDiagnostic> {
    let at = |p: &Value| Some((p["line"].as_u64()? as usize, p["character"].as_u64()? as usize));
    let severity = match d["severity"].as_u64().unwrap_or(1) {
        1 => Severity::Error,
        2 => Severity::Warning,
        3 => Severity::Info,
        _ => return None, // hints (unused code etc.) are left out
    };
    let mut message = d["message"].as_str()?.to_string();
    let code = d["code"].as_str().map(String::from).or_else(|| d["code"].as_i64().map(|c| c.to_string()));
    if let Some(source) = d["source"].as_str() {
        message = match code {
            Some(c) => format!("{message}  ({source} {c})"),
            None => format!("{message}  ({source})"),
        };
    }
    Some(RawDiagnostic { start: at(&d["range"]["start"])?, end: at(&d["range"]["end"])?, severity, message })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_round_trip() {
        let p = Path::new("/home/me/My Project/src/main é.rs");
        let uri = path_to_uri(p);
        assert_eq!(uri, "file:///home/me/My%20Project/src/main%20%C3%A9.rs");
        assert_eq!(uri_to_path(&uri).unwrap(), p);
        assert_eq!(uri_to_path("file:///C:/x/y.rs").unwrap(), PathBuf::from("C:/x/y.rs"));
    }

    #[test]
    fn utf16_columns() {
        let line: Vec<char> = "a😀b".chars().collect();
        assert_eq!(utf16_to_col(&line, 0), 0);
        assert_eq!(utf16_to_col(&line, 1), 1);
        assert_eq!(utf16_to_col(&line, 3), 2, "the emoji is two UTF-16 units");
        assert_eq!(utf16_to_col(&line, 99), 3);
    }

    #[test]
    fn parses_diagnostics() {
        let d = json!({
            "range": {"start": {"line": 3, "character": 4}, "end": {"line": 3, "character": 9}},
            "severity": 1, "source": "rustc", "code": "E0308", "message": "mismatched types"
        });
        let r = raw_diagnostic(&d).unwrap();
        assert_eq!((r.start, r.end, r.severity), ((3, 4), (3, 9), Severity::Error));
        assert_eq!(r.message, "mismatched types  (rustc E0308)");
        assert!(raw_diagnostic(&json!({"range": d["range"], "severity": 4, "message": "unused"})).is_none());
    }

    #[test]
    fn overrides_pick_or_disable_servers() {
        let lang = language_for("rs").unwrap();
        let mut o = BTreeMap::new();
        o.insert("rust".to_string(), "".to_string());
        assert_eq!(resolve_command(&lang, &o), None, "empty disables");
        o.insert("rust".to_string(), "my-ra --flag".to_string());
        assert_eq!(resolve_command(&lang, &o), Some(vec!["my-ra".to_string(), "--flag".to_string()]));
    }
}
