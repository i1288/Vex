//! Syntax checking with tree-sitter, on a background thread so typing never
//! waits for it. Finds syntax errors (ERROR nodes) and things the parser had
//! to assume (MISSING nodes, like a missing `;`); not type errors.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

use tree_sitter::{Language, Node, Parser, Point};

use crate::buffer::Pos;

/// Files larger than this are not checked (parsing them could take a while).
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_DIAGNOSTICS: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub start: Pos,
    /// Exclusive; equal to `start` for something missing at a point.
    pub end: Pos,
    pub message: String,
}

impl Diagnostic {
    /// Whether `p` is on this diagnostic (a point diagnostic covers one char).
    pub fn contains(&self, p: Pos) -> bool {
        let end = if self.end == self.start { Pos::new(self.start.line, self.start.col + 1) } else { self.end };
        p >= self.start && p < end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Grammar {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    C,
    Cpp,
    Go,
    Java,
    Json,
    Bash,
    Css,
    Html,
    Toml,
    Yaml,
}

impl Grammar {
    pub fn for_extension(ext: &str) -> Option<Self> {
        Some(match ext {
            "rs" => Self::Rust,
            "py" | "pyw" => Self::Python,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "c" | "h" => Self::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Self::Cpp,
            "go" => Self::Go,
            "java" => Self::Java,
            "json" => Self::Json,
            "sh" | "bash" => Self::Bash,
            "css" => Self::Css,
            "html" | "htm" => Self::Html,
            "toml" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            _ => return None,
        })
    }

    fn language(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::Json => tree_sitter_json::LANGUAGE.into(),
            Self::Bash => tree_sitter_bash::LANGUAGE.into(),
            Self::Css => tree_sitter_css::LANGUAGE.into(),
            Self::Html => tree_sitter_html::LANGUAGE.into(),
            Self::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            Self::Yaml => tree_sitter_yaml::LANGUAGE.into(),
        }
    }

    pub fn parser(self) -> Parser {
        let mut p = Parser::new();
        p.set_language(&self.language()).expect("grammar is compatible with this tree-sitter");
        p
    }
}

/// Parse `text` and list its syntax errors, in document order.
pub fn check(parser: &mut Parser, text: &str) -> Vec<Diagnostic> {
    if text.len() > MAX_BYTES {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else { return Vec::new() };
    let root = tree.root_node();
    if !root.has_error() {
        return Vec::new();
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let pos = |p: Point| {
        let line = lines.get(p.row).copied().unwrap_or("");
        let col = line.get(..p.column.min(line.len())).map_or(0, |s| s.chars().count());
        Pos::new(p.row, col)
    };

    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if out.len() >= MAX_DIAGNOSTICS {
            break;
        }
        if node.is_missing() {
            let at = pos(node.start_position());
            out.push(Diagnostic { start: at, end: at, message: missing_message(node.kind()) });
        } else if node.is_error() {
            let start = pos(node.start_position());
            let mut end = pos(node.end_position());
            // Underline at most the rest of the first line of a long error.
            if end.line > start.line || end <= start {
                let len = lines.get(start.line).map_or(0, |l| l.chars().count());
                end = Pos::new(start.line, len.max(start.col + 1));
            }
            out.push(Diagnostic { start, end, message: error_message(node, text) });
        } else if node.has_error() {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            stack.extend(children.into_iter().rev());
        }
    }
    out.sort_by_key(|d| d.start);
    out
}

fn missing_message(kind: &str) -> String {
    if kind.chars().any(|c| c.is_alphanumeric()) {
        format!("Missing {}", kind.replace('_', " "))
    } else {
        format!("Missing `{kind}`")
    }
}

fn error_message(node: Node, text: &str) -> String {
    // Unbalanced brackets are the most common cause; name them when the error
    // region shows one.
    let end = node.end_byte().min(node.start_byte() + 4096);
    let region = text.get(node.start_byte()..end).unwrap_or("");
    for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
        let opens = region.matches(open).count();
        let closes = region.matches(close).count();
        if opens > closes {
            return format!("Unclosed `{open}`");
        }
        if closes > opens {
            return format!("Unexpected `{close}`");
        }
    }
    let mut leaf = node;
    while let Some(child) = leaf.child(0) {
        leaf = child;
    }
    let token = text.get(leaf.start_byte()..leaf.end_byte()).unwrap_or("").trim();
    if token.is_empty() || token.chars().count() > 24 || token.contains('\n') {
        "Syntax error".into()
    } else {
        format!("Syntax error: unexpected `{token}`")
    }
}

struct Job {
    id: u64,
    version: u64,
    grammar: Grammar,
    text: String,
}

pub struct Done {
    pub id: u64,
    pub version: u64,
    pub diagnostics: Vec<Diagnostic>,
}

/// Runs checks on a worker thread. Submit text whenever a file changes; only
/// the newest submission per file is checked.
pub struct Checker {
    jobs: Sender<Job>,
    results: Receiver<Done>,
}

impl Checker {
    /// `notify` runs on the worker after results are ready (e.g. to repaint).
    pub fn new(notify: impl Fn() + Send + 'static) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("vex-syntax-check".into())
            .spawn(move || {
                let mut parsers: HashMap<Grammar, Parser> = HashMap::new();
                while let Ok(first) = job_rx.recv() {
                    // Skip versions that newer submissions already replaced.
                    let mut latest = HashMap::new();
                    latest.insert(first.id, first);
                    while let Ok(job) = job_rx.try_recv() {
                        latest.insert(job.id, job);
                    }
                    for job in latest.into_values() {
                        let parser = parsers.entry(job.grammar).or_insert_with(|| job.grammar.parser());
                        let diagnostics = check(parser, &job.text);
                        if done_tx.send(Done { id: job.id, version: job.version, diagnostics }).is_err() {
                            return;
                        }
                    }
                    notify();
                }
            })
            .expect("can start the syntax-check thread");
        Self { jobs, results }
    }

    pub fn submit(&self, id: u64, version: u64, grammar: Grammar, text: String) {
        let _ = self.jobs.send(Job { id, version, grammar, text });
    }

    pub fn results(&self) -> impl Iterator<Item = Done> + '_ {
        self.results.try_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errors(g: Grammar, text: &str) -> Vec<Diagnostic> {
        check(&mut g.parser(), text)
    }

    #[test]
    fn valid_code_has_no_errors_in_every_language() {
        let samples = [
            (Grammar::Rust, "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n"),
            (Grammar::Python, "def f(x):\n    return x * 2\n\nprint(f(3))\n"),
            (Grammar::JavaScript, "const f = (x) => x * 2;\nconsole.log(f(3));\n"),
            (Grammar::TypeScript, "function f(x: number): number { return x * 2; }\n"),
            (Grammar::Tsx, "const A = () => <div className=\"a\">hi</div>;\n"),
            (Grammar::C, "#include <stdio.h>\nint main(void) { printf(\"hi\\n\"); return 0; }\n"),
            (Grammar::Cpp, "#include <vector>\nint main() { std::vector<int> v{1, 2}; return v[0]; }\n"),
            (Grammar::Go, "package main\n\nfunc main() {\n\tx := 1\n\t_ = x\n}\n"),
            (Grammar::Java, "class A { int f(int x) { return x * 2; } }\n"),
            (Grammar::Json, "{\"a\": [1, 2, {\"b\": null}]}\n"),
            (Grammar::Bash, "for f in *.txt; do\n  echo \"$f\"\ndone\n"),
            (Grammar::Css, "a { color: red; margin: 0 auto; }\n"),
            (Grammar::Html, "<!DOCTYPE html>\n<html><body><p>hi</p></body></html>\n"),
            (Grammar::Toml, "[package]\nname = \"vex\"\nversion = \"0.1.0\"\n"),
            (Grammar::Yaml, "a: 1\nb:\n  - x\n  - y\n"),
        ];
        for (g, text) in samples {
            assert_eq!(errors(g, text), vec![], "{g:?} should parse cleanly");
        }
    }

    #[test]
    fn finds_errors_with_positions() {
        let e = errors(Grammar::Rust, "fn main() {\n    let x = 1\n    let y = 2;\n}\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].message, "Missing `;`");
        assert_eq!(e[0].start, Pos::new(1, 13));

        let e = errors(Grammar::Json, "{\"a\": 1,}\n");
        assert!(!e.is_empty());
        assert_eq!(e[0].start.line, 0);

        let e = errors(Grammar::Python, "def f(:\n    pass\n");
        assert!(!e.is_empty());

        let e = errors(Grammar::Python, "def g(name):\n    print(name\n\ng(1)\n");
        assert_eq!(e.first().map(|d| d.message.as_str()), Some("Unclosed `(`"), "{e:?}");

        let e = errors(Grammar::Rust, "fn main() {\n    let x = f(1));\n}\n");
        assert_eq!(e.first().map(|d| d.message.as_str()), Some("Unexpected `)`"), "{e:?}");

        let e = errors(Grammar::JavaScript, "let s = 'é' + ;\n");
        assert!(!e.is_empty());
        assert_eq!(e[0].start.line, 0);
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let e = errors(Grammar::Rust, "fn é() { let ü = 1 }\n");
        assert!(!e.is_empty(), "missing semicolon expected");
        // "fn é() { let ü = 1" is 18 chars but more bytes.
        assert!(e.iter().all(|d| d.start.col <= 19), "{e:?}");
    }

    /// `cargo test --release -- --ignored --nocapture check_speed`
    #[test]
    #[ignore]
    fn check_speed() {
        let text = include_str!("vim.rs").repeat(4);
        let mut parser = Grammar::Rust.parser();
        let start = std::time::Instant::now();
        let runs = 20;
        for _ in 0..runs {
            check(&mut parser, &text);
        }
        let per = start.elapsed() / runs;
        println!("{} lines: {per:?} per full check", text.lines().count());
        let broken = text.replacen("fn ", "fn (", 50);
        let start = std::time::Instant::now();
        let n = check(&mut parser, &broken).len();
        println!("with errors: {:?} ({n} diagnostics)", start.elapsed());
    }

    #[test]
    fn worker_returns_only_the_latest_version() {
        let checker = Checker::new(|| {});
        checker.submit(7, 1, Grammar::Rust, "fn main( {".into());
        checker.submit(7, 2, Grammar::Rust, "fn main() {}".into());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut last = None;
        while std::time::Instant::now() < deadline {
            for done in checker.results() {
                last = Some((done.version, done.diagnostics.len()));
            }
            if last.is_some_and(|(v, _)| v == 2) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(last, Some((2, 0)));
    }
}
