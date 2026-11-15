//! Small keyword-based syntax highlighter. Line oriented, with block-comment
//! state carried between lines.

use crate::theme;
use eframe::egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Control,
    Type,
    Function,
    Str,
    Number,
    Comment,
    Constant,
}

impl Tok {
    pub fn color(self) -> Color32 {
        match self {
            Tok::Plain => theme::p().text,
            Tok::Keyword => theme::p().keyword,
            Tok::Control => theme::p().control,
            Tok::Type => theme::p().type_,
            Tok::Function => theme::p().function,
            Tok::Str => theme::p().string,
            Tok::Number => theme::p().number,
            Tok::Comment => theme::p().comment,
            Tok::Constant => theme::p().constant,
        }
    }
}

pub struct Lang {
    pub name: &'static str,
    keywords: &'static [&'static str],
    control: &'static [&'static str],
    types: &'static [&'static str],
    line_comment: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
}

const RUST: Lang = Lang {
    name: "Rust",
    keywords: &[
        "as", "async", "await", "const", "crate", "dyn", "enum", "extern", "false", "fn", "impl", "in", "let", "mod",
        "move", "mut", "pub", "ref", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
        "use", "where",
    ],
    control: &["break", "continue", "else", "for", "if", "loop", "match", "return", "while", "yield"],
    types: &[
        "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "str", "u8", "u16", "u32", "u64",
        "u128", "usize",
    ],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &['"'],
};

const C: Lang = Lang {
    name: "C/C++",
    keywords: &[
        "auto", "class", "const", "constexpr", "delete", "enum", "explicit", "extern", "false", "friend", "inline",
        "namespace", "new", "noexcept", "nullptr", "operator", "override", "private", "protected", "public",
        "register", "sizeof", "static", "struct", "template", "this", "true", "typedef", "typename", "union",
        "using", "virtual", "volatile", "NULL", "#include", "#define", "#ifdef", "#ifndef", "#endif", "#if",
        "#else", "#pragma",
    ],
    control: &["break", "case", "catch", "continue", "default", "do", "else", "for", "goto", "if", "return", "switch", "throw", "try", "while"],
    types: &[
        "bool", "char", "double", "float", "int", "long", "short", "signed", "unsigned", "void", "size_t",
        "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "std", "string",
    ],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &['"', '\''],
};

const PYTHON: Lang = Lang {
    name: "Python",
    keywords: &[
        "and", "as", "async", "await", "class", "def", "del", "False", "global", "in", "is", "lambda", "None",
        "nonlocal", "not", "or", "self", "True", "import", "from",
    ],
    control: &["assert", "break", "continue", "elif", "else", "except", "finally", "for", "if", "pass", "raise", "return", "try", "while", "with", "yield"],
    types: &["int", "float", "str", "bool", "list", "dict", "set", "tuple", "bytes", "object"],
    line_comment: &["#"],
    block_comment: None,
    quotes: &['"', '\''],
};

const JS: Lang = Lang {
    name: "JavaScript",
    keywords: &[
        "async", "await", "class", "const", "constructor", "delete", "enum", "export", "extends", "false", "function",
        "get", "implements", "import", "in", "instanceof", "interface", "let", "new", "null", "of", "private",
        "protected", "public", "readonly", "set", "static", "super", "this", "true", "type", "typeof", "undefined",
        "var", "void", "from", "as",
    ],
    control: &["break", "case", "catch", "continue", "default", "do", "else", "finally", "for", "if", "return", "switch", "throw", "try", "while", "yield"],
    types: &["any", "boolean", "number", "string", "unknown", "never", "object", "symbol", "bigint"],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &['"', '\'', '`'],
};

const GO: Lang = Lang {
    name: "Go",
    keywords: &[
        "chan", "const", "defer", "false", "func", "go", "import", "interface", "iota", "map", "nil", "package",
        "range", "struct", "true", "type", "var",
    ],
    control: &["break", "case", "continue", "default", "else", "fallthrough", "for", "goto", "if", "return", "select", "switch"],
    types: &[
        "bool", "byte", "error", "float32", "float64", "int", "int8", "int16", "int32", "int64", "rune", "string",
        "uint", "uint8", "uint16", "uint32", "uint64", "uintptr", "any",
    ],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &['"', '\'', '`'],
};

const JAVA: Lang = Lang {
    name: "Java",
    keywords: &[
        "abstract", "class", "extends", "final", "implements", "import", "interface", "new", "null", "package",
        "private", "protected", "public", "static", "super", "this", "true", "false", "void", "enum", "var",
    ],
    control: &["break", "case", "catch", "continue", "default", "do", "else", "finally", "for", "if", "return", "switch", "throw", "throws", "try", "while"],
    types: &["boolean", "byte", "char", "double", "float", "int", "long", "short", "String"],
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: &['"', '\''],
};

const SHELL: Lang = Lang {
    name: "Shell",
    keywords: &["export", "local", "function", "echo", "set", "unset", "source", "alias", "readonly"],
    control: &["if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "in", "return", "exit"],
    types: &[],
    line_comment: &["#"],
    block_comment: None,
    quotes: &['"', '\''],
};

const CONFIG: Lang = Lang {
    name: "Config",
    keywords: &["true", "false", "null"],
    control: &[],
    types: &[],
    line_comment: &["#", ";"],
    block_comment: None,
    quotes: &['"', '\''],
};

const JSON: Lang = Lang {
    name: "JSON",
    keywords: &["true", "false", "null"],
    control: &[],
    types: &[],
    line_comment: &[],
    block_comment: None,
    quotes: &['"'],
};

pub fn lang_for(ext: &str) -> Option<&'static Lang> {
    Some(match ext {
        "rs" => &RUST,
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" => &C,
        "py" | "pyw" => &PYTHON,
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => &JS,
        "go" => &GO,
        "java" | "kt" | "cs" => &JAVA,
        "sh" | "bash" | "zsh" => &SHELL,
        "toml" | "ini" | "cfg" | "conf" | "yaml" | "yml" => &CONFIG,
        "json" => &JSON,
        _ => return None,
    })
}

pub fn language_name(ext: &str) -> &'static str {
    match ext {
        "md" | "markdown" => "Markdown",
        "txt" | "" => "Plain Text",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "ts" | "tsx" => "TypeScript",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        _ => lang_for(ext).map_or("Plain Text", |l| l.name),
    }
}

fn starts_with(line: &[char], at: usize, pat: &str) -> bool {
    let mut i = at;
    for c in pat.chars() {
        if line.get(i) != Some(&c) {
            return false;
        }
        i += 1;
    }
    true
}

/// Split `line` into colored spans `(start, end, tok)` in char indices.
/// `in_block` carries "inside a block comment" across lines.
pub fn highlight(lang: &Lang, line: &[char], in_block: &mut bool) -> Vec<(usize, usize, Tok)> {
    let mut spans = Vec::new();
    let mut push = |s: usize, e: usize, t: Tok| {
        if e > s {
            spans.push((s, e, t));
        }
    };
    let n = line.len();
    let mut i = 0;
    let mut plain_start = 0;

    if *in_block {
        let (_, end) = lang.block_comment.expect("block state without block comments");
        let close = (0..n).find(|&j| starts_with(line, j, end));
        match close {
            Some(j) => {
                *in_block = false;
                i = j + end.chars().count();
                push(0, i, Tok::Comment);
                plain_start = i;
            }
            None => {
                push(0, n, Tok::Comment);
                return spans;
            }
        }
    }

    while i < n {
        let c = line[i];
        let tok_start = i;
        let tok = if lang.line_comment.iter().any(|lc| starts_with(line, i, lc)) {
            i = n;
            Some(Tok::Comment)
        } else if let Some((open, close)) = lang.block_comment.filter(|(o, _)| starts_with(line, i, o)) {
            let body = i + open.chars().count();
            match (body..n).find(|&j| starts_with(line, j, close)) {
                Some(j) => i = j + close.chars().count(),
                None => {
                    i = n;
                    *in_block = true;
                }
            }
            Some(Tok::Comment)
        } else if lang.quotes.contains(&c) {
            i += 1;
            while i < n && line[i] != c {
                if line[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            Some(Tok::Str)
        } else if c.is_ascii_digit() && (i == 0 || !is_ident(line[i - 1])) {
            while i < n && (line[i].is_ascii_alphanumeric() || line[i] == '.' || line[i] == '_') {
                i += 1;
            }
            Some(Tok::Number)
        } else if is_ident(c) || c == '#' {
            i += 1;
            while i < n && is_ident(line[i]) {
                i += 1;
            }
            let word: String = line[tok_start..i].iter().collect();
            let next = line[i..].iter().find(|c| !c.is_whitespace()).copied();
            Some(if lang.control.contains(&word.as_str()) {
                Tok::Control
            } else if lang.keywords.contains(&word.as_str()) {
                Tok::Keyword
            } else if lang.types.contains(&word.as_str()) {
                Tok::Type
            } else if next == Some('(') || next == Some('!') {
                Tok::Function
            } else if word.chars().all(|c| c.is_uppercase() || c == '_' || c.is_ascii_digit()) && word.len() > 1 {
                Tok::Constant
            } else if word.chars().next().is_some_and(|c| c.is_uppercase()) {
                Tok::Type
            } else {
                Tok::Plain
            })
        } else {
            i += 1;
            None
        };
        if let Some(t) = tok {
            push(plain_start, tok_start, Tok::Plain);
            push(tok_start, i, t);
            plain_start = i;
        }
    }
    push(plain_start, n, Tok::Plain);
    spans
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<(String, Tok)> {
        let line: Vec<char> = src.chars().collect();
        let mut block = false;
        highlight(&RUST, &line, &mut block)
            .into_iter()
            .filter(|s| s.2 != Tok::Plain)
            .map(|(s, e, t)| (line[s..e].iter().collect(), t))
            .collect()
    }

    #[test]
    fn rust_line() {
        let t = toks("let x = foo(\"hi\"); // done");
        assert_eq!(
            t,
            vec![
                ("let".into(), Tok::Keyword),
                ("foo".into(), Tok::Function),
                ("\"hi\"".into(), Tok::Str),
                ("// done".into(), Tok::Comment),
            ]
        );
    }

    #[test]
    fn block_comment_spans_lines() {
        let mut block = false;
        let a: Vec<char> = "x /* start".chars().collect();
        highlight(&RUST, &a, &mut block);
        assert!(block);
        let b: Vec<char> = "end */ y".chars().collect();
        let spans = highlight(&RUST, &b, &mut block);
        assert!(!block);
        assert_eq!(spans[0], (0, 6, Tok::Comment));
    }
}
