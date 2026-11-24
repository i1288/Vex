//! Text buffer: a list of lines plus cursor, file path and undo history.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static TAB_SIZE: AtomicUsize = AtomicUsize::new(4);
static EXPAND_TAB: AtomicBool = AtomicBool::new(true);

/// Display width of a tab, and the width of one indent level.
pub fn tab_size() -> usize {
    TAB_SIZE.load(Ordering::Relaxed)
}

/// Whether indenting inserts spaces rather than a tab character.
pub fn expand_tab() -> bool {
    EXPAND_TAB.load(Ordering::Relaxed)
}

pub fn set_indent(size: usize, expand: bool) {
    TAB_SIZE.store(size.clamp(1, 16), Ordering::Relaxed);
    EXPAND_TAB.store(expand, Ordering::Relaxed);
}

/// One level of indentation as text.
pub fn indent_unit() -> String {
    if expand_tab() { " ".repeat(tab_size()) } else { "\t".into() }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<Vec<char>>,
    cursor: Pos,
}

pub struct Buffer {
    pub lines: Vec<Vec<char>>,
    pub path: Option<PathBuf>,
    pub cursor: Pos,
    /// Column that vertical motions try to return to.
    pub want_col: usize,
    /// Scroll offsets in pixels, owned by the view.
    pub scroll_y: f32,
    pub scroll_x: f32,
    pub dirty: bool,
    /// The file used Windows (CRLF) line endings; saving keeps them.
    pub crlf: bool,
    /// Unique for the life of the app; buffers move around, this doesn't.
    pub id: u64,
    /// Bumped on every edit, so background work can tell if it is stale.
    pub version: u64,
    /// Problems to show: language-server results plus syntax errors on lines
    /// the server didn't flag. Rebuilt by `merge_diagnostics`.
    pub diagnostics: Vec<crate::diagnostics::Diagnostic>,
    /// From the tree-sitter syntax check, and the version it was run on.
    pub syntax_diagnostics: Vec<crate::diagnostics::Diagnostic>,
    pub checked_version: Option<u64>,
    /// From the language server (type checking).
    pub lsp_diagnostics: Vec<crate::diagnostics::Diagnostic>,
    /// Lines changed since the last commit, for the gutter, and the version
    /// they were worked out for.
    pub git_changes: Vec<crate::git::LineChange>,
    pub git_version: Option<u64>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::from_str("")
    }
}

impl Buffer {
    pub fn from_str(text: &str) -> Self {
        let mut lines: Vec<Vec<char>> = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).chars().collect())
            .collect();
        // A trailing newline terminates the last line rather than starting a new one.
        if lines.len() > 1 && text.ends_with('\n') {
            lines.pop();
        }
        Self {
            lines,
            path: None,
            cursor: Pos::default(),
            want_col: 0,
            scroll_y: 0.0,
            scroll_x: 0.0,
            dirty: false,
            crlf: text.contains("\r\n"),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            version: 0,
            diagnostics: Vec::new(),
            syntax_diagnostics: Vec::new(),
            checked_version: None,
            lsp_diagnostics: Vec::new(),
            git_changes: Vec::new(),
            git_version: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let text = match fs::read(path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let mut buf = Self::from_str(&text);
        buf.path = Some(path.to_path_buf());
        Ok(buf)
    }

    pub fn save(&mut self) -> io::Result<usize> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| io::Error::other("E32: No file name"))?;
        let mut text = self.text();
        text.push('\n');
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        fs::write(&path, &text)?;
        self.dirty = false;
        Ok(text.len())
    }

    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into())
    }

    pub fn extension(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.extension())
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.lines.get(line).map_or(0, |l| l.len())
    }

    pub fn last_line(&self) -> usize {
        self.lines.len() - 1
    }

    /// Character at `p`, or `'\n'` at (or past) the end of a line.
    pub fn char_at(&self, p: Pos) -> char {
        self.lines
            .get(p.line)
            .and_then(|l| l.get(p.col))
            .copied()
            .unwrap_or('\n')
    }

    /// Next position, treating each line end as a newline character.
    pub fn next_pos(&self, p: Pos) -> Option<Pos> {
        if p.col < self.line_len(p.line) {
            Some(Pos::new(p.line, p.col + 1))
        } else if p.line + 1 < self.lines.len() {
            Some(Pos::new(p.line + 1, 0))
        } else {
            None
        }
    }

    pub fn prev_pos(&self, p: Pos) -> Option<Pos> {
        if p.col > 0 {
            Some(Pos::new(p.line, p.col - 1))
        } else if p.line > 0 {
            Some(Pos::new(p.line - 1, self.line_len(p.line - 1)))
        } else {
            None
        }
    }

    pub fn first_non_blank(&self, line: usize) -> usize {
        self.lines[line]
            .iter()
            .position(|c| !c.is_whitespace())
            .unwrap_or(self.line_len(line))
    }

    pub fn indent_of(&self, line: usize) -> Vec<char> {
        self.lines[line]
            .iter()
            .take_while(|c| **c == ' ' || **c == '\t')
            .copied()
            .collect()
    }

    /// Screen column of a char column, expanding tabs.
    pub fn visual_col(&self, line: usize, col: usize) -> usize {
        let mut v = 0;
        for c in self.lines[line].iter().take(col) {
            v += if *c == '\t' { tab_size() - v % tab_size() } else { 1 };
        }
        v
    }

    /// Char column closest to a screen column.
    pub fn col_from_visual(&self, line: usize, target: usize) -> usize {
        let mut v = 0;
        for (i, c) in self.lines[line].iter().enumerate() {
            let w = if *c == '\t' { tab_size() - v % tab_size() } else { 1 };
            if v + w > target {
                return i;
            }
            v += w;
        }
        self.line_len(line)
    }

    /// Clamp the cursor into the buffer. `past_end` allows the column one past
    /// the last character (insert mode).
    pub fn clamp_cursor(&mut self, past_end: bool) {
        self.cursor.line = self.cursor.line.min(self.last_line());
        let len = self.line_len(self.cursor.line);
        let max = if past_end { len } else { len.saturating_sub(1) };
        self.cursor.col = self.cursor.col.min(max);
    }

    // ---- editing -------------------------------------------------------

    /// Combine syntax and language-server problems for display. A line the
    /// server reports an error on doesn't also get the syntax error.
    pub fn merge_diagnostics(&mut self) {
        use crate::diagnostics::Severity;
        let mut all = self.lsp_diagnostics.clone();
        let server_lines: std::collections::HashSet<usize> =
            self.lsp_diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.start.line).collect();
        all.extend(self.syntax_diagnostics.iter().filter(|d| !server_lines.contains(&d.start.line)).cloned());
        all.sort_by_key(|d| (d.start, d.severity));
        self.diagnostics = all;
    }

    /// Record that the text changed.
    pub fn touch(&mut self) {
        self.dirty = true;
        self.version += 1;
    }

    /// Insert `text` (may contain newlines) at `p`; returns the position after it.
    pub fn insert(&mut self, p: Pos, text: &str) -> Pos {
        self.touch();
        let mut parts = text.split('\n');
        let first: Vec<char> = parts.next().unwrap_or("").chars().collect();
        let col = p.col.min(self.line_len(p.line));
        let tail = self.lines[p.line].split_off(col);
        let mut end = Pos::new(p.line, col + first.len());
        self.lines[p.line].extend(first);
        for part in parts {
            end.line += 1;
            let chars: Vec<char> = part.chars().collect();
            end.col = chars.len();
            self.lines.insert(end.line, chars);
        }
        self.lines[end.line].extend(tail);
        end
    }

    /// Text between `a` and `b` (exclusive), newlines between lines.
    pub fn slice(&self, a: Pos, b: Pos) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let mut out = String::new();
        let mut p = a;
        while p < b {
            out.push(self.char_at(p));
            match self.next_pos(p) {
                Some(n) => p = n,
                None => break,
            }
        }
        out
    }

    /// Delete text between `a` and `b` (exclusive); returns the removed text.
    pub fn delete(&mut self, a: Pos, b: Pos) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let removed = self.slice(a, b);
        if removed.is_empty() {
            return removed;
        }
        self.touch();
        let b_col = b.col.min(self.line_len(b.line));
        if a.line == b.line {
            self.lines[a.line].drain(a.col..b_col);
        } else {
            let tail: Vec<char> = self.lines[b.line][b_col..].to_vec();
            self.lines[a.line].truncate(a.col);
            self.lines[a.line].extend(tail);
            self.lines.drain(a.line + 1..=b.line);
        }
        removed
    }

    /// Remove whole lines `a..=b`, keeping at least one line in the buffer.
    pub fn delete_lines(&mut self, a: usize, b: usize) -> String {
        self.touch();
        let removed: Vec<String> = self
            .lines
            .drain(a..=b)
            .map(|l| l.into_iter().collect())
            .collect();
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        removed.join("\n")
    }

    pub fn insert_lines(&mut self, at: usize, text: &str) {
        self.touch();
        for (i, l) in text.split('\n').enumerate() {
            self.lines.insert(at + i, l.chars().collect());
        }
    }

    pub fn lines_text(&self, a: usize, b: usize) -> String {
        self.lines[a..=b]
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Remove spaces and tabs at line ends (as one undoable change); returns
    /// whether anything changed.
    pub fn trim_trailing_whitespace(&mut self) -> bool {
        if !self.lines.iter().any(|l| l.last().is_some_and(|c| c.is_whitespace())) {
            return false;
        }
        self.checkpoint();
        for line in &mut self.lines {
            let keep = line.iter().rposition(|c| !c.is_whitespace()).map_or(0, |i| i + 1);
            line.truncate(keep);
        }
        self.touch();
        true
    }

    // ---- undo ----------------------------------------------------------

    /// Record the current state as an undo point; call before each change.
    pub fn checkpoint(&mut self) {
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        if self.undo.len() > 1000 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else { return false };
        self.redo.push(Snapshot {
            lines: std::mem::replace(&mut self.lines, s.lines),
            cursor: self.cursor,
        });
        self.cursor = s.cursor;
        self.touch();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        self.undo.push(Snapshot {
            lines: std::mem::replace(&mut self.lines, s.lines),
            cursor: self.cursor,
        });
        self.cursor = s.cursor;
        self.touch();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_delete_multiline() {
        let mut b = Buffer::from_str("hello world");
        let end = b.insert(Pos::new(0, 5), ",\nbig");
        assert_eq!(b.text(), "hello,\nbig world");
        assert_eq!(end, Pos::new(1, 3));
        let removed = b.delete(Pos::new(0, 5), Pos::new(1, 3));
        assert_eq!(removed, ",\nbig");
        assert_eq!(b.text(), "hello world");
    }

    #[test]
    fn trailing_newline_is_not_an_extra_line() {
        let b = Buffer::from_str("a\nb\n");
        assert_eq!(b.line_count(), 2);
    }

    #[test]
    fn crlf_files_keep_their_line_endings() {
        let path = std::env::temp_dir().join(format!("vex-crlf-{}.txt", std::process::id()));
        std::fs::write(&path, "a\r\nb\r\n").unwrap();
        let mut b = Buffer::open(&path).unwrap();
        assert!(b.crlf);
        assert_eq!(b.text(), "a\nb");
        b.insert(Pos::new(1, 1), "c");
        b.save().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\r\nbc\r\n");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn trims_trailing_whitespace() {
        let mut b = Buffer::from_str("a  \n\t\nb");
        assert!(b.trim_trailing_whitespace());
        assert_eq!(b.text(), "a\n\nb");
        assert!(!b.trim_trailing_whitespace());
    }

    #[test]
    fn undo_redo() {
        let mut b = Buffer::from_str("abc");
        b.checkpoint();
        b.delete(Pos::new(0, 0), Pos::new(0, 1));
        assert_eq!(b.text(), "bc");
        b.undo();
        assert_eq!(b.text(), "abc");
        b.redo();
        assert_eq!(b.text(), "bc");
    }
}
