//! Modal (vim-style) editing engine. Independent of the GUI: the app feeds it
//! `Key`s and acts on the `Action`s it returns.

use regex::{Regex, RegexBuilder};

use crate::buffer::{expand_tab, indent_unit, tab_size, Buffer, Pos};
use crate::keymap::{self, Keymaps, Lookup, MapMode, Mapping, Origin};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    Command,
    Search,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::Command => "COMMAND",
            Mode::Search => "SEARCH",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Esc,
    Enter,
    Backspace,
    Delete,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Requests the editor engine makes of the surrounding app.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Write(Option<String>),
    WriteAll,
    WriteQuit,
    WriteQuitAll,
    Quit { force: bool },
    QuitAll { force: bool },
    Edit(String),
    NextTab,
    PrevTab,
    /// `:set` arguments, applied by the app.
    Set(String),
    OpenSettings,
    /// Split the focused pane, optionally opening a file in the new one.
    Split(Option<String>),
    /// A `Ctrl-w` window command: h l w W v s q c o.
    Window(char),
    /// Run a script file (`:source`).
    Source(String),
    /// An app command by name: terminal, files, sidebar, keybindings.
    App(&'static str),
}

#[derive(Clone, Copy, Debug)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    NextLine,
    WordFwd(bool),
    WordBack(bool),
    WordEnd(bool),
    LineStart,
    FirstNonBlank,
    LineEnd,
    FileTop,
    FileBottom,
    ParaFwd,
    ParaBack,
    Find(char, char),
    RepeatFind(bool),
    MatchPair,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MKind {
    Exclusive,
    Inclusive,
    Linewise,
}

#[derive(Clone, Copy, Debug)]
enum Target {
    Line,
    Motion(Motion),
    Object { inner: bool, kind: char },
}

#[derive(Debug)]
enum Cmd {
    Move(Motion, Option<usize>),
    Op(char, Option<usize>, Target),
    Simple(&'static str, Option<usize>, Option<char>),
}

enum Parse<T> {
    Incomplete,
    Invalid,
    Done(T),
}

enum Range {
    Lines(usize, usize),
    Chars(Pos, Pos),
}

pub struct Vim {
    pub mode: Mode,
    pub cmdline: String,
    pub message: Option<String>,
    pub message_is_error: bool,
    pub search: Option<String>,
    search_re: Option<(String, Regex)>,
    pub highlight_search: bool,
    pub search_backward: bool,
    pub visual_anchor: Pos,
    /// Number of lines visible in the view; set by the app for paging.
    pub page_lines: usize,
    /// Yanked text the app should copy to the system clipboard.
    pub clipboard_out: Option<String>,
    pending: Vec<Key>,
    register: String,
    register_linewise: bool,
    dot: Vec<Key>,
    recording: Option<Vec<Key>>,
    replaying: bool,
    last_find: Option<(char, char)>,
    history: Vec<String>,
    history_idx: usize,
    pub keymaps: Keymaps,
    /// What `<Leader>` expands to (`let mapleader = ...`).
    pub leader: Vec<Key>,
    /// Origin recorded for mappings defined by `:map` commands.
    pub map_origin: Origin,
    /// Typed keys held back because they may start a mapping.
    typeahead: Vec<Key>,
    typeahead_since: f64,
}

/// How long typed keys wait for the rest of a mapping (vim's 'timeoutlen').
pub const MAP_TIMEOUT: f64 = 1.0;
const DEFAULT_LEADER: char = '\\';

impl Default for Vim {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            cmdline: String::new(),
            message: None,
            message_is_error: false,
            search: None,
            search_re: None,
            highlight_search: false,
            search_backward: false,
            visual_anchor: Pos::default(),
            page_lines: 30,
            clipboard_out: None,
            pending: Vec::new(),
            register: String::new(),
            register_linewise: false,
            dot: Vec::new(),
            recording: None,
            replaying: false,
            last_find: None,
            history: Vec::new(),
            history_idx: 0,
            keymaps: Keymaps::default(),
            leader: vec![Key::Char(DEFAULT_LEADER)],
            map_origin: Origin::Runtime,
            typeahead: Vec::new(),
            typeahead_since: 0.0,
        }
    }
}

// ---- parsing -------------------------------------------------------------

fn parse_count(keys: &[Key], i: &mut usize) -> Option<usize> {
    let mut n: Option<usize> = None;
    while let Some(Key::Char(c)) = keys.get(*i) {
        let Some(d) = c.to_digit(10) else { break };
        if d == 0 && n.is_none() {
            break; // a leading 0 is the line-start motion
        }
        n = Some(n.unwrap_or(0).saturating_mul(10).saturating_add(d as usize));
        *i += 1;
    }
    n
}

fn mul(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (None, None) => None,
        _ => Some(a.unwrap_or(1) * b.unwrap_or(1)),
    }
}

fn parse_motion(keys: &[Key], i: usize) -> Parse<Motion> {
    use Motion::*;
    let m = match keys.get(i) {
        None => return Parse::Incomplete,
        Some(Key::Char(c)) => match c {
            'h' => Left,
            'l' | ' ' => Right,
            'j' => Down,
            'k' => Up,
            'w' => WordFwd(false),
            'W' => WordFwd(true),
            'b' => WordBack(false),
            'B' => WordBack(true),
            'e' => WordEnd(false),
            'E' => WordEnd(true),
            '0' => LineStart,
            '^' => FirstNonBlank,
            '$' => LineEnd,
            'G' => FileBottom,
            '}' => ParaFwd,
            '{' => ParaBack,
            '%' => MatchPair,
            ';' => RepeatFind(false),
            ',' => RepeatFind(true),
            'f' | 't' | 'F' | 'T' => match keys.get(i + 1) {
                None => return Parse::Incomplete,
                Some(Key::Char(ch)) => Find(*c, *ch),
                _ => return Parse::Invalid,
            },
            'g' => match keys.get(i + 1) {
                None => return Parse::Incomplete,
                Some(Key::Char('g')) => FileTop,
                _ => return Parse::Invalid,
            },
            _ => return Parse::Invalid,
        },
        Some(Key::Left) | Some(Key::Backspace) => Left,
        Some(Key::Right) => Right,
        Some(Key::Up) => Up,
        Some(Key::Down) => Down,
        Some(Key::Home) => LineStart,
        Some(Key::End) => LineEnd,
        Some(Key::Enter) => NextLine,
        _ => return Parse::Invalid,
    };
    Parse::Done(m)
}

fn parse_simple(keys: &[Key], i: usize, count: Option<usize>) -> Parse<Cmd> {
    let with_arg = |name, keys: &[Key]| match keys.get(i + 1) {
        None => Parse::Incomplete,
        Some(Key::Char(c)) => Parse::Done(Cmd::Simple(name, count, Some(*c))),
        _ => Parse::Invalid,
    };
    let name = match &keys[i] {
        Key::Char(c) => match c {
            'x' => "x",
            'X' => "X",
            'p' => "p",
            'P' => "P",
            'u' => "u",
            'i' => "i",
            'a' => "a",
            'I' => "I",
            'A' => "A",
            'o' => "o",
            'O' => "O",
            'v' => "v",
            'V' => "V",
            'J' => "J",
            'D' => "D",
            'C' => "C",
            'Y' => "Y",
            's' => "s",
            'S' => "S",
            '~' => "~",
            'n' => "n",
            'N' => "N",
            '*' => "*",
            '#' => "#",
            ':' => ":",
            '/' => "/",
            '?' => "?",
            '.' => ".",
            'r' => return with_arg("r", keys),
            'g' => match keys.get(i + 1) {
                None => return Parse::Incomplete,
                Some(Key::Char('t')) => "gt",
                Some(Key::Char('T')) => "gT",
                _ => return Parse::Invalid,
            },
            'Z' => match keys.get(i + 1) {
                None => return Parse::Incomplete,
                Some(Key::Char('Z')) => "ZZ",
                Some(Key::Char('Q')) => "ZQ",
                _ => return Parse::Invalid,
            },
            _ => return Parse::Invalid,
        },
        Key::Ctrl('r') => "C-r",
        Key::Ctrl('w') => {
            return match keys.get(i + 1) {
                None => Parse::Incomplete,
                Some(Key::Char(c) | Key::Ctrl(c)) => Parse::Done(Cmd::Simple("C-w", count, Some(*c))),
                _ => Parse::Invalid,
            };
        }
        Key::Ctrl('d') => "C-d",
        Key::Ctrl('u') => "C-u",
        Key::Ctrl('f') | Key::PageDown => "C-f",
        Key::Ctrl('b') | Key::PageUp => "C-b",
        Key::Delete => "x",
        _ => return Parse::Invalid,
    };
    Parse::Done(Cmd::Simple(name, count, None))
}

fn parse(keys: &[Key]) -> Parse<Cmd> {
    let mut i = 0;
    let count = parse_count(keys, &mut i);
    let Some(k) = keys.get(i) else { return Parse::Incomplete };
    if let Key::Char(op @ ('d' | 'c' | 'y' | '>' | '<')) = k {
        let op = *op;
        i += 1;
        let count = mul(count, parse_count(keys, &mut i));
        let Some(k2) = keys.get(i) else { return Parse::Incomplete };
        if *k2 == Key::Char(op) {
            return Parse::Done(Cmd::Op(op, count, Target::Line));
        }
        if let Key::Char(m @ ('i' | 'a')) = k2 {
            return match keys.get(i + 1) {
                None => Parse::Incomplete,
                Some(Key::Char(kind)) => Parse::Done(Cmd::Op(
                    op,
                    count,
                    Target::Object { inner: *m == 'i', kind: *kind },
                )),
                _ => Parse::Invalid,
            };
        }
        return match parse_motion(keys, i) {
            Parse::Done(m) => Parse::Done(Cmd::Op(op, count, Target::Motion(m))),
            Parse::Incomplete => Parse::Incomplete,
            Parse::Invalid => Parse::Invalid,
        };
    }
    match parse_motion(keys, i) {
        Parse::Done(m) => Parse::Done(Cmd::Move(m, count)),
        Parse::Incomplete => Parse::Incomplete,
        Parse::Invalid => parse_simple(keys, i, count),
    }
}

// ---- text navigation helpers --------------------------------------------

fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

fn word_fwd(buf: &Buffer, mut p: Pos, big: bool) -> Pos {
    let start_line = p.line;
    let c0 = class(buf.char_at(p), big);
    if c0 != 0 {
        loop {
            match buf.next_pos(p) {
                Some(n) => {
                    p = n;
                    if class(buf.char_at(p), big) != c0 {
                        break;
                    }
                }
                None => return p,
            }
        }
    }
    loop {
        if class(buf.char_at(p), big) != 0 {
            return p;
        }
        if p.line != start_line && buf.line_len(p.line) == 0 {
            return p;
        }
        match buf.next_pos(p) {
            Some(n) => p = n,
            None => return p,
        }
    }
}

fn word_end(buf: &Buffer, p: Pos, big: bool) -> Pos {
    let Some(mut p) = buf.next_pos(p) else { return p };
    while class(buf.char_at(p), big) == 0 {
        match buf.next_pos(p) {
            Some(n) => p = n,
            None => return p,
        }
    }
    let c = class(buf.char_at(p), big);
    while let Some(n) = buf.next_pos(p) {
        if class(buf.char_at(n), big) != c {
            break;
        }
        p = n;
    }
    p
}

fn word_back(buf: &Buffer, p: Pos, big: bool) -> Pos {
    let Some(mut p) = buf.prev_pos(p) else { return p };
    while class(buf.char_at(p), big) == 0 {
        if p.col == 0 && buf.line_len(p.line) == 0 {
            return p;
        }
        match buf.prev_pos(p) {
            Some(n) => p = n,
            None => return p,
        }
    }
    let c = class(buf.char_at(p), big);
    while let Some(n) = buf.prev_pos(p) {
        if class(buf.char_at(n), big) != c {
            break;
        }
        p = n;
    }
    p
}

fn is_blank(buf: &Buffer, line: usize) -> bool {
    buf.lines[line].iter().all(|c| c.is_whitespace())
}

fn para_fwd(buf: &Buffer, p: Pos) -> Pos {
    let last = buf.last_line();
    let mut l = p.line;
    while l < last && is_blank(buf, l) {
        l += 1;
    }
    while l < last && !is_blank(buf, l) {
        l += 1;
    }
    if l == last && !is_blank(buf, l) {
        Pos::new(l, buf.line_len(l))
    } else {
        Pos::new(l, 0)
    }
}

fn para_back(buf: &Buffer, p: Pos) -> Pos {
    let mut l = p.line;
    while l > 0 && is_blank(buf, l) {
        l -= 1;
    }
    while l > 0 && !is_blank(buf, l) {
        l -= 1;
    }
    Pos::new(l, 0)
}

fn find_char(buf: &Buffer, from: Pos, kind: char, ch: char, count: usize, repeat: bool) -> Option<Pos> {
    let line = &buf.lines[from.line];
    let mut found = 0;
    if kind == 'f' || kind == 't' {
        let skip = usize::from(repeat && kind == 't');
        for c in from.col + 1 + skip..line.len() {
            if line[c] == ch {
                found += 1;
                if found == count {
                    return Some(Pos::new(from.line, if kind == 'f' { c } else { c - 1 }));
                }
            }
        }
    } else {
        let skip = usize::from(repeat && kind == 'T');
        for c in (0..from.col.saturating_sub(skip)).rev() {
            if line[c] == ch {
                found += 1;
                if found == count {
                    return Some(Pos::new(from.line, if kind == 'F' { c } else { c + 1 }));
                }
            }
        }
    }
    None
}

const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

/// Find the unmatched `open` before `from` and its matching `close`.
fn enclosing_pair(buf: &Buffer, from: Pos, open: char, close: char) -> Option<(Pos, Pos)> {
    let mut p = from;
    let mut depth = 0;
    let start = loop {
        let c = buf.char_at(p);
        if c == open {
            if depth == 0 {
                break p;
            }
            depth -= 1;
        } else if c == close && p != from {
            depth += 1;
        }
        p = buf.prev_pos(p)?;
    };
    let end = matching_close(buf, start, open, close)?;
    Some((start, end))
}

fn matching_close(buf: &Buffer, start: Pos, open: char, close: char) -> Option<Pos> {
    let mut p = start;
    let mut depth = 0;
    loop {
        p = buf.next_pos(p)?;
        let c = buf.char_at(p);
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                return Some(p);
            }
            depth -= 1;
        }
    }
}

fn match_pair(buf: &Buffer, from: Pos) -> Option<Pos> {
    let line = &buf.lines[from.line];
    let (col, c) = line
        .iter()
        .enumerate()
        .skip(from.col)
        .find(|(_, c)| PAIRS[..3].iter().any(|(o, cl)| *c == o || *c == cl))?;
    let p = Pos::new(from.line, col);
    for (o, cl) in PAIRS {
        if *c == o {
            return matching_close(buf, p, o, cl);
        }
        if *c == cl {
            return enclosing_pair(buf, p, o, cl).map(|(s, _)| s);
        }
    }
    None
}

/// Compile a search pattern. Smartcase: case-insensitive unless the pattern has
/// uppercase. A pattern that is not valid regex is matched literally.
pub fn build_regex(pat: &str) -> Regex {
    let fold = !pat.chars().any(|c| c.is_uppercase());
    RegexBuilder::new(pat)
        .case_insensitive(fold)
        .build()
        .or_else(|_| RegexBuilder::new(&regex::escape(pat)).case_insensitive(fold).build())
        .expect("escaped pattern is valid")
}

/// Char ranges `(start, end)` of every match of `re` in `line`.
pub fn search_matches(line: &[char], re: &Regex) -> Vec<(usize, usize)> {
    let text: String = line.iter().collect();
    // Byte offset -> char index.
    let mut idx = vec![0; text.len() + 1];
    for (ci, (bi, _)) in text.char_indices().enumerate() {
        idx[bi] = ci;
    }
    idx[text.len()] = line.len();
    re.find_iter(&text).map(|m| (idx[m.start()], idx[m.end()])).collect()
}

/// Split `s` on unescaped `delim`, unescaping `\delim`, into at most `n` parts.
fn split_unescaped(s: &str, delim: char, n: usize) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&delim) {
            parts.last_mut().unwrap().push(chars.next().unwrap());
        } else if c == delim && parts.len() < n {
            parts.push(String::new());
        } else {
            parts.last_mut().unwrap().push(c);
        }
    }
    parts
}

/// Vim-style `\1` backreferences become regex-crate `${1}`.
fn convert_replacement(rep: &str) -> String {
    let mut out = String::new();
    let mut chars = rep.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(d)) if d.is_ascii_digit() => {
                out.push_str(&format!("${{{d}}}"));
                chars.next();
            }
            ('\\', Some('n')) => {
                out.push('\n');
                chars.next();
            }
            ('\\', Some('t')) => {
                out.push('\t');
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

// ---- engine --------------------------------------------------------------

impl Vim {
    /// Type one key: mappings apply, then the editor handles the result.
    /// `now` is a clock in seconds, for the mapping timeout.
    pub fn feed(&mut self, buf: &mut Buffer, key: Key, now: f64) -> Vec<Action> {
        self.typeahead.push(key);
        self.typeahead_since = now;
        self.process_typeahead(buf, false)
    }

    /// When held-back keys stop waiting for a mapping and run as typed.
    pub fn typeahead_deadline(&self) -> Option<f64> {
        (!self.typeahead.is_empty()).then_some(self.typeahead_since + MAP_TIMEOUT)
    }

    /// The mapping timeout passed: run held-back keys.
    pub fn flush_typeahead(&mut self, buf: &mut Buffer) -> Vec<Action> {
        self.process_typeahead(buf, true)
    }

    /// The mapping table for the current state; none mid-command or on the command line.
    fn map_mode(&self) -> Option<MapMode> {
        match self.mode {
            Mode::Normal if self.pending.is_empty() => Some(MapMode::Normal),
            Mode::Visual | Mode::VisualLine if self.pending.is_empty() => Some(MapMode::Visual),
            Mode::Insert => Some(MapMode::Insert),
            _ => None,
        }
    }

    fn process_typeahead(&mut self, buf: &mut Buffer, timed_out: bool) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut expansions = 0;
        while !self.typeahead.is_empty() {
            let found = match self.map_mode() {
                Some(m) => self.keymaps.lookup(m, &self.typeahead),
                None => Lookup::None,
            };
            match found {
                Lookup::Exact { index, ambiguous } if !ambiguous || timed_out => {
                    let m = self.keymaps.maps[index].clone();
                    self.typeahead.drain(..m.lhs.len());
                    expansions += 1;
                    if expansions > 100 {
                        self.typeahead.clear();
                        self.error("E223: recursive mapping");
                        break;
                    }
                    if m.noremap {
                        for k in m.rhs {
                            actions.extend(self.handle_key(buf, k));
                        }
                    } else {
                        self.typeahead.splice(0..0, m.rhs);
                    }
                }
                Lookup::Prefix | Lookup::Exact { .. } if !timed_out => break,
                _ => {
                    let k = self.typeahead.remove(0);
                    actions.extend(self.handle_key(buf, k));
                }
            }
        }
        actions
    }

    /// Run one ex command line (from a script); errors come back as `Err`.
    pub fn execute(&mut self, buf: &mut Buffer, line: &str) -> Result<Option<Action>, String> {
        self.message = None;
        let action = self.ex_command(buf, line);
        match self.message.take() {
            Some(msg) if self.message_is_error => Err(msg),
            msg => {
                self.message = msg;
                Ok(action)
            }
        }
    }

    pub fn handle_key(&mut self, buf: &mut Buffer, key: Key) -> Option<Action> {
        // Ctrl-C works like Esc, as in vim.
        let key = if key == Key::Ctrl('c') { Key::Esc } else { key };
        if !matches!(self.mode, Mode::Command | Mode::Search) {
            self.message = None;
        }
        match self.mode {
            Mode::Normal => self.normal_key(buf, key),
            Mode::Insert => {
                if let Some(rec) = &mut self.recording {
                    rec.push(key.clone());
                }
                self.insert_key(buf, key);
                None
            }
            Mode::Visual | Mode::VisualLine => self.visual_key(buf, key),
            Mode::Command | Mode::Search => self.cmdline_key(buf, key),
        }
    }

    pub fn pending_keys(&self) -> String {
        self.pending
            .iter()
            .chain(&self.typeahead)
            .map(|k| match k {
                Key::Char(c) => c.to_string(),
                Key::Ctrl(c) => format!("^{}", c.to_ascii_uppercase()),
                _ => String::new(),
            })
            .collect()
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
        self.message_is_error = true;
    }

    fn info(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
        self.message_is_error = false;
    }

    fn set_register(&mut self, text: String, linewise: bool) {
        self.register = text;
        self.register_linewise = linewise;
    }

    /// Selection of the visual modes as an exclusive char range or a line range.
    pub fn selection(&self, buf: &Buffer) -> Option<(Pos, Pos)> {
        let (a, b) = if self.visual_anchor <= buf.cursor {
            (self.visual_anchor, buf.cursor)
        } else {
            (buf.cursor, self.visual_anchor)
        };
        match self.mode {
            Mode::Visual => Some((a, b)),
            Mode::VisualLine => Some((Pos::new(a.line, 0), Pos::new(b.line, buf.line_len(b.line)))),
            _ => None,
        }
    }

    fn enter_normal(&mut self, buf: &mut Buffer) {
        self.mode = Mode::Normal;
        buf.clamp_cursor(false);
    }

    fn set_cursor(&mut self, buf: &mut Buffer, p: Pos) {
        buf.cursor = p;
        buf.clamp_cursor(self.mode == Mode::Insert);
        buf.want_col = buf.visual_col(buf.cursor.line, buf.cursor.col);
    }

    // ---- normal mode ----

    fn normal_key(&mut self, buf: &mut Buffer, key: Key) -> Option<Action> {
        if key == Key::Esc {
            self.pending.clear();
            return None;
        }
        self.pending.push(key);
        match parse(&self.pending) {
            Parse::Incomplete => None,
            Parse::Invalid => {
                self.pending.clear();
                None
            }
            Parse::Done(cmd) => {
                let keys = std::mem::take(&mut self.pending);
                let change = match &cmd {
                    Cmd::Op(op, ..) => *op != 'y',
                    Cmd::Simple(name, ..) => matches!(
                        *name,
                        "x" | "X" | "p" | "P" | "i" | "a" | "I" | "A" | "o" | "O" | "J" | "D" | "C"
                            | "s" | "S" | "~" | "r"
                    ),
                    Cmd::Move(..) => false,
                };
                if change {
                    buf.checkpoint();
                }
                let action = self.exec(buf, cmd);
                if change && !self.replaying {
                    if self.mode == Mode::Insert {
                        self.recording = Some(keys);
                    } else {
                        self.dot = keys;
                    }
                }
                action
            }
        }
    }

    fn exec(&mut self, buf: &mut Buffer, cmd: Cmd) -> Option<Action> {
        match cmd {
            Cmd::Move(m, count) => {
                if let Some((p, _)) = self.apply_motion(buf, m, count, buf.cursor, false) {
                    buf.cursor = p;
                    buf.clamp_cursor(false);
                    match m {
                        Motion::Up | Motion::Down => {}
                        Motion::LineEnd => buf.want_col = usize::MAX,
                        _ => buf.want_col = buf.visual_col(buf.cursor.line, buf.cursor.col),
                    }
                }
                None
            }
            Cmd::Op(op, count, target) => {
                self.operate(buf, op, count, target);
                None
            }
            Cmd::Simple(name, count, arg) => self.simple(buf, name, count, arg),
        }
    }

    fn simple(&mut self, buf: &mut Buffer, name: &str, count: Option<usize>, arg: Option<char>) -> Option<Action> {
        let n = count.unwrap_or(1);
        let cur = buf.cursor;
        match name {
            "x" => self.operate(buf, 'd', count, Target::Motion(Motion::Right)),
            "X" => self.operate(buf, 'd', count, Target::Motion(Motion::Left)),
            "D" => self.operate(buf, 'd', count, Target::Motion(Motion::LineEnd)),
            "C" => self.operate(buf, 'c', count, Target::Motion(Motion::LineEnd)),
            "s" => self.operate(buf, 'c', count, Target::Motion(Motion::Right)),
            "S" => self.operate(buf, 'c', count, Target::Line),
            "Y" => self.operate(buf, 'y', count, Target::Line),
            "p" | "P" => {
                let text = self.register.repeat(n);
                if self.register_linewise {
                    let text = vec![self.register.as_str(); n].join("\n");
                    let at = if name == "p" { cur.line + 1 } else { cur.line };
                    buf.insert_lines(at, &text);
                    self.set_cursor(buf, Pos::new(at, buf.first_non_blank(at)));
                } else if !text.is_empty() {
                    let col = if name == "p" && buf.line_len(cur.line) > 0 { cur.col + 1 } else { cur.col };
                    let end = buf.insert(Pos::new(cur.line, col), &text);
                    let end = buf.prev_pos(end).unwrap_or(end);
                    self.set_cursor(buf, end);
                }
            }
            "u" | "C-r" => {
                let mut done = 0;
                for _ in 0..n {
                    let ok = if name == "u" { buf.undo() } else { buf.redo() };
                    if !ok {
                        break;
                    }
                    done += 1;
                }
                if done == 0 {
                    self.info(if name == "u" { "Already at oldest change" } else { "Already at newest change" });
                }
                buf.clamp_cursor(false);
            }
            "i" => self.mode = Mode::Insert,
            "a" => {
                self.mode = Mode::Insert;
                if buf.line_len(cur.line) > 0 {
                    buf.cursor.col += 1;
                }
            }
            "I" => {
                self.mode = Mode::Insert;
                buf.cursor.col = buf.first_non_blank(cur.line);
            }
            "A" => {
                self.mode = Mode::Insert;
                buf.cursor.col = buf.line_len(cur.line);
            }
            "o" | "O" => {
                let indent: String = buf.indent_of(cur.line).into_iter().collect();
                let at = if name == "o" { cur.line + 1 } else { cur.line };
                buf.insert_lines(at, &indent);
                self.mode = Mode::Insert;
                buf.cursor = Pos::new(at, indent.chars().count());
            }
            "v" | "V" => {
                self.mode = if name == "v" { Mode::Visual } else { Mode::VisualLine };
                self.visual_anchor = cur;
            }
            "J" => {
                for _ in 0..n.max(2) - 1 {
                    self.join_line(buf, buf.cursor.line);
                }
            }
            "~" => {
                let len = buf.line_len(cur.line);
                let end = (cur.col + n).min(len);
                for c in cur.col..end {
                    let ch = buf.lines[cur.line][c];
                    buf.lines[cur.line][c] = toggle_case(ch);
                }
                if end > cur.col {
                    buf.dirty = true;
                }
                buf.cursor.col = end;
                buf.clamp_cursor(false);
            }
            "r" => {
                let ch = arg?;
                let len = buf.line_len(cur.line);
                if cur.col + n > len {
                    return None;
                }
                for c in cur.col..cur.col + n {
                    buf.lines[cur.line][c] = ch;
                }
                buf.dirty = true;
                buf.cursor.col = cur.col + n - 1;
            }
            "n" | "N" => {
                let backward = self.search_backward != (name == "N");
                for _ in 0..n {
                    if !self.search_next(buf, !backward) {
                        break;
                    }
                }
            }
            "*" | "#" => {
                let line = &buf.lines[cur.line];
                let is_word = |c: &char| c.is_alphanumeric() || *c == '_';
                if !line.get(cur.col).is_some_and(is_word) {
                    return None;
                }
                let s = (0..=cur.col).rev().take_while(|&i| is_word(&line[i])).last()?;
                let e = (cur.col..line.len()).take_while(|&i| is_word(&line[i])).last()?;
                self.search = Some(line[s..=e].iter().collect());
                self.search_backward = name == "#";
                self.highlight_search = true;
                buf.cursor.col = s;
                self.search_next(buf, name == "*");
            }
            ":" => {
                self.mode = Mode::Command;
                self.cmdline.clear();
                self.history_idx = self.history.len();
            }
            "/" | "?" => {
                self.mode = Mode::Search;
                self.search_backward = name == "?";
                self.cmdline.clear();
            }
            "." => {
                let keys = self.dot.clone();
                self.replaying = true;
                for k in keys {
                    self.handle_key(buf, k);
                }
                self.replaying = false;
            }
            "C-w" => return arg.map(Action::Window),
            "gt" => return Some(Action::NextTab),
            "gT" => return Some(Action::PrevTab),
            "ZZ" => return Some(Action::WriteQuit),
            "ZQ" => return Some(Action::Quit { force: true }),
            "C-d" | "C-u" | "C-f" | "C-b" => {
                let page = self.page_lines.max(2);
                let amount = if name == "C-d" || name == "C-u" { page / 2 } else { page - 2 };
                let line = if name == "C-d" || name == "C-f" {
                    (cur.line + amount * n).min(buf.last_line())
                } else {
                    cur.line.saturating_sub(amount * n)
                };
                buf.cursor = Pos::new(line, buf.first_non_blank(line));
                buf.clamp_cursor(false);
            }
            _ => {}
        }
        None
    }

    pub fn paste_text(&mut self, buf: &mut Buffer, text: &str) {
        match self.mode {
            Mode::Insert => {
                buf.cursor = buf.insert(buf.cursor, text);
            }
            Mode::Command | Mode::Search => self.cmdline.push_str(text.lines().next().unwrap_or("")),
            _ => {
                buf.checkpoint();
                let (reg, lw) = (std::mem::take(&mut self.register), self.register_linewise);
                self.register = text.to_string();
                self.register_linewise = text.ends_with('\n');
                if self.register_linewise {
                    self.register.pop();
                }
                self.mode = Mode::Normal;
                self.simple(buf, "p", None, None);
                self.register = reg;
                self.register_linewise = lw;
            }
        }
    }

    fn join_line(&mut self, buf: &mut Buffer, line: usize) {
        if line >= buf.last_line() {
            return;
        }
        let len = buf.line_len(line);
        let next_indent = buf.indent_of(line + 1).len();
        buf.delete(Pos::new(line, len), Pos::new(line + 1, next_indent));
        let needs_space = len > 0
            && buf.line_len(line) > len
            && !buf.lines[line][len - 1].is_whitespace()
            && buf.lines[line][len] != ')';
        if needs_space {
            buf.insert(Pos::new(line, len), " ");
        }
        buf.cursor = Pos::new(line, len);
    }

    /// Resolve a motion from `from`. Returns the target and the motion kind.
    fn apply_motion(
        &mut self,
        buf: &Buffer,
        m: Motion,
        count: Option<usize>,
        from: Pos,
        for_op: bool,
    ) -> Option<(Pos, MKind)> {
        use MKind::*;
        let n = count.unwrap_or(1);
        let last = buf.last_line();
        let r = match m {
            Motion::Left => (Pos::new(from.line, from.col.saturating_sub(n)), Exclusive),
            Motion::Right => {
                let len = buf.line_len(from.line);
                let max = if for_op { len } else { len.saturating_sub(1) };
                (Pos::new(from.line, (from.col + n).min(max)), Exclusive)
            }
            Motion::Up | Motion::Down => {
                let line = if matches!(m, Motion::Up) {
                    if for_op && from.line == 0 {
                        return None;
                    }
                    from.line.saturating_sub(n)
                } else {
                    if for_op && from.line == last {
                        return None;
                    }
                    (from.line + n).min(last)
                };
                (Pos::new(line, buf.col_from_visual(line, buf.want_col)), Linewise)
            }
            Motion::NextLine => {
                let line = (from.line + n).min(last);
                (Pos::new(line, buf.first_non_blank(line)), Linewise)
            }
            Motion::WordFwd(big) => {
                let mut p = from;
                for _ in 0..n {
                    p = word_fwd(buf, p, big);
                }
                (p, Exclusive)
            }
            Motion::WordBack(big) => {
                let mut p = from;
                for _ in 0..n {
                    p = word_back(buf, p, big);
                }
                (p, Exclusive)
            }
            Motion::WordEnd(big) => {
                let mut p = from;
                for _ in 0..n {
                    p = word_end(buf, p, big);
                }
                (p, Inclusive)
            }
            Motion::LineStart => (Pos::new(from.line, 0), Exclusive),
            Motion::FirstNonBlank => (Pos::new(from.line, buf.first_non_blank(from.line)), Exclusive),
            Motion::LineEnd => {
                let line = (from.line + n - 1).min(last);
                (Pos::new(line, buf.line_len(line).saturating_sub(1)), Inclusive)
            }
            Motion::FileTop | Motion::FileBottom => {
                let line = match count {
                    Some(c) => c.saturating_sub(1).min(last),
                    None if matches!(m, Motion::FileTop) => 0,
                    None => last,
                };
                (Pos::new(line, buf.first_non_blank(line)), Linewise)
            }
            Motion::ParaFwd => {
                let mut p = from;
                for _ in 0..n {
                    p = para_fwd(buf, p);
                }
                (p, Exclusive)
            }
            Motion::ParaBack => {
                let mut p = from;
                for _ in 0..n {
                    p = para_back(buf, p);
                }
                (p, Exclusive)
            }
            Motion::Find(kind, ch) => {
                self.last_find = Some((kind, ch));
                let p = find_char(buf, from, kind, ch, n, false)?;
                (p, if kind == 'f' || kind == 't' { Inclusive } else { Exclusive })
            }
            Motion::RepeatFind(reverse) => {
                let (mut kind, ch) = self.last_find?;
                if reverse {
                    kind = match kind {
                        'f' => 'F',
                        'F' => 'f',
                        't' => 'T',
                        _ => 't',
                    };
                }
                let p = find_char(buf, from, kind, ch, n, true)?;
                (p, if kind == 'f' || kind == 't' { Inclusive } else { Exclusive })
            }
            Motion::MatchPair => (match_pair(buf, from)?, Inclusive),
        };
        Some(r)
    }

    fn text_object(&self, buf: &Buffer, inner: bool, kind: char) -> Option<(Pos, Pos)> {
        let cur = buf.cursor;
        match kind {
            'w' | 'W' => {
                let line = &buf.lines[cur.line];
                if line.is_empty() {
                    return Some((cur, cur)); // so `ciw` still enters insert mode
                }
                let big = kind == 'W';
                let col = cur.col.min(line.len() - 1);
                let c = class(line[col], big);
                let mut s = col;
                while s > 0 && class(line[s - 1], big) == c {
                    s -= 1;
                }
                let mut e = col + 1;
                while e < line.len() && class(line[e], big) == c {
                    e += 1;
                }
                if !inner {
                    if e < line.len() && line[e].is_whitespace() {
                        while e < line.len() && line[e].is_whitespace() {
                            e += 1;
                        }
                    } else {
                        while s > 0 && line[s - 1].is_whitespace() {
                            s -= 1;
                        }
                    }
                }
                Some((Pos::new(cur.line, s), Pos::new(cur.line, e)))
            }
            '"' | '\'' | '`' => {
                let line = &buf.lines[cur.line];
                let quotes: Vec<usize> = (0..line.len())
                    .filter(|&i| line[i] == kind && (i == 0 || line[i - 1] != '\\'))
                    .collect();
                let (a, b) = quotes
                    .chunks_exact(2)
                    .map(|p| (p[0], p[1]))
                    .find(|&(a, b)| cur.col <= b && (a <= cur.col || a > cur.col))?;
                if inner {
                    Some((Pos::new(cur.line, a + 1), Pos::new(cur.line, b)))
                } else {
                    Some((Pos::new(cur.line, a), Pos::new(cur.line, b + 1)))
                }
            }
            _ => {
                let (open, close) = match kind {
                    '(' | ')' | 'b' => ('(', ')'),
                    '[' | ']' => ('[', ']'),
                    '{' | '}' | 'B' => ('{', '}'),
                    '<' | '>' => ('<', '>'),
                    _ => return None,
                };
                let (s, e) = enclosing_pair(buf, cur, open, close)?;
                if !inner {
                    return Some((s, Pos::new(e.line, e.col + 1)));
                }
                // A block whose braces sit on their own lines: take the lines between them.
                if e.line > s.line + 1
                    && s.col + 1 == buf.line_len(s.line)
                    && e.col == buf.first_non_blank(e.line)
                {
                    return Some((Pos::new(s.line + 1, 0), Pos::new(e.line, 0)));
                }
                Some((Pos::new(s.line, s.col + 1), e))
            }
        }
    }

    fn target_range(&mut self, buf: &Buffer, op: char, count: Option<usize>, target: Target) -> Option<Range> {
        let cur = buf.cursor;
        let n = count.unwrap_or(1);
        match target {
            Target::Line => Some(Range::Lines(cur.line, (cur.line + n - 1).min(buf.last_line()))),
            Target::Object { inner, kind } => {
                let (a, b) = self.text_object(buf, inner, kind)?;
                Some(Range::Chars(a, b))
            }
            Target::Motion(m) => {
                // `cw` on a word changes to the end of the word, like `ce`.
                if let (Motion::WordFwd(big), 'c') = (m, op) {
                    if class(buf.char_at(cur), big) != 0 {
                        let mut p = cur;
                        while let Some(nx) = buf.next_pos(p) {
                            if nx.line != p.line || class(buf.char_at(nx), big) != class(buf.char_at(cur), big) {
                                break;
                            }
                            p = nx;
                        }
                        for _ in 1..n {
                            p = word_end(buf, p, big);
                        }
                        return Some(Range::Chars(cur, Pos::new(p.line, p.col + 1)));
                    }
                }
                let (end, kind) = self.apply_motion(buf, m, count, cur, true)?;
                let (a, b) = if cur <= end { (cur, end) } else { (end, cur) };
                Some(match kind {
                    MKind::Linewise => Range::Lines(a.line, b.line),
                    MKind::Inclusive => Range::Chars(a, Pos::new(b.line, (b.col + 1).min(buf.line_len(b.line)))),
                    MKind::Exclusive => {
                        // An exclusive motion ending at column 0 of a later line stops at the line end.
                        if b.line > a.line && b.col == 0 && !matches!(m, Motion::ParaFwd | Motion::ParaBack) {
                            let l = b.line - 1;
                            Range::Chars(a, Pos::new(l, buf.line_len(l)))
                        } else {
                            Range::Chars(a, b)
                        }
                    }
                })
            }
        }
    }

    fn operate(&mut self, buf: &mut Buffer, op: char, count: Option<usize>, target: Target) {
        let Some(range) = self.target_range(buf, op, count, target) else { return };
        self.apply_op(buf, op, range);
    }

    fn apply_op(&mut self, buf: &mut Buffer, op: char, range: Range) {
        match range {
            Range::Lines(a, b) => match op {
                'd' => {
                    let text = buf.delete_lines(a, b);
                    self.set_register(text, true);
                    let line = a.min(buf.last_line());
                    self.set_cursor(buf, Pos::new(line, buf.first_non_blank(line)));
                }
                'y' => {
                    let text = buf.lines_text(a, b);
                    self.clipboard_out = Some(format!("{text}\n"));
                    self.set_register(text, true);
                    if b - a >= 2 {
                        self.info(format!("{} lines yanked", b - a + 1));
                    }
                    let col = buf.cursor.col;
                    self.set_cursor(buf, Pos::new(a, col));
                }
                'c' => {
                    let text = buf.lines_text(a, b);
                    self.set_register(text, true);
                    if b > a {
                        buf.delete_lines(a + 1, b);
                    }
                    let indent = buf.indent_of(a);
                    let len = indent.len();
                    buf.lines[a] = indent;
                    buf.dirty = true;
                    self.mode = Mode::Insert;
                    buf.cursor = Pos::new(a, len);
                }
                '>' | '<' => {
                    for l in a..=b {
                        shift_line(buf, l, op == '>');
                    }
                    self.set_cursor(buf, Pos::new(a, buf.first_non_blank(a)));
                }
                _ => {}
            },
            Range::Chars(a, b) => match op {
                'd' => {
                    let text = buf.delete(a, b);
                    self.set_register(text, false);
                    self.set_cursor(buf, a);
                }
                'y' => {
                    let text = buf.slice(a, b);
                    self.clipboard_out = Some(text.clone());
                    self.set_register(text, false);
                    self.set_cursor(buf, a);
                }
                'c' => {
                    let text = buf.delete(a, b);
                    self.set_register(text, false);
                    self.mode = Mode::Insert;
                    buf.cursor = a;
                }
                '>' | '<' => {
                    for l in a.line..=b.line {
                        shift_line(buf, l, op == '>');
                    }
                    self.set_cursor(buf, Pos::new(a.line, buf.first_non_blank(a.line)));
                }
                'u' | 'U' | '~' => {
                    let mut p = a;
                    while p < b && p.line < buf.line_count() {
                        if p.col < buf.line_len(p.line) {
                            let c = &mut buf.lines[p.line][p.col];
                            *c = match op {
                                'u' => c.to_lowercase().next().unwrap_or(*c),
                                'U' => c.to_uppercase().next().unwrap_or(*c),
                                _ => toggle_case(*c),
                            };
                        }
                        match buf.next_pos(p) {
                            Some(n) => p = n,
                            None => break,
                        }
                    }
                    buf.dirty = true;
                    self.set_cursor(buf, a);
                }
                _ => {}
            },
        }
    }

    // ---- visual mode ----

    fn visual_key(&mut self, buf: &mut Buffer, key: Key) -> Option<Action> {
        if key == Key::Esc {
            self.pending.clear();
            self.enter_normal(buf);
            return None;
        }
        self.pending.push(key);
        let keys = self.pending.clone();
        let mut i = 0;
        let count = parse_count(&keys, &mut i);
        let Some(k) = keys.get(i) else { return None };

        if let Key::Char(m @ ('i' | 'a')) = k {
            let Some(Key::Char(kind)) = keys.get(i + 1) else {
                if keys.len() == i + 1 {
                    return None;
                }
                self.pending.clear();
                return None;
            };
            self.pending.clear();
            if let Some((a, b)) = self.text_object(buf, *m == 'i', *kind) {
                self.visual_anchor = a;
                buf.cursor = buf.prev_pos(b).unwrap_or(b);
            }
            return None;
        }

        match parse_motion(&keys, i) {
            Parse::Incomplete => return None,
            Parse::Done(m) => {
                self.pending.clear();
                if let Some((p, _)) = self.apply_motion(buf, m, count, buf.cursor, false) {
                    buf.cursor = p;
                    buf.clamp_cursor(false);
                    if !matches!(m, Motion::Up | Motion::Down) {
                        buf.want_col = buf.visual_col(p.line, p.col);
                    }
                }
                return None;
            }
            Parse::Invalid => {}
        }
        self.pending.clear();

        let (a, b) = self.selection(buf)?;
        let linewise = self.mode == Mode::VisualLine;
        let range = if linewise {
            Range::Lines(a.line, b.line)
        } else {
            let end = if b.col >= buf.line_len(b.line) && b.line < buf.last_line() {
                Pos::new(b.line + 1, 0)
            } else {
                Pos::new(b.line, (b.col + 1).min(buf.line_len(b.line)))
            };
            Range::Chars(a, end)
        };
        let op = match k {
            Key::Char('d' | 'x') | Key::Delete => 'd',
            Key::Char('y') => 'y',
            Key::Char('c' | 's') => 'c',
            Key::Char('>') => '>',
            Key::Char('<') => '<',
            Key::Char('~') => '~',
            Key::Char('u') => 'u',
            Key::Char('U') => 'U',
            Key::Char('J') => {
                buf.checkpoint();
                for _ in 0..(b.line - a.line).max(1) {
                    self.join_line(buf, a.line);
                }
                self.enter_normal(buf);
                return None;
            }
            Key::Char('o') => {
                std::mem::swap(&mut self.visual_anchor, &mut buf.cursor);
                return None;
            }
            Key::Char('v') | Key::Char('V') => {
                let target = if *k == Key::Char('v') { Mode::Visual } else { Mode::VisualLine };
                if self.mode == target {
                    self.enter_normal(buf);
                } else {
                    self.mode = target;
                }
                return None;
            }
            Key::Char(':') => {
                self.mode = Mode::Command;
                self.cmdline.clear();
                return None;
            }
            _ => return None,
        };
        if op != 'y' {
            buf.checkpoint();
        }
        // Case ops on whole lines work charwise over the full lines.
        let range = match (range, op) {
            (Range::Lines(a, b), 'u' | 'U' | '~') => {
                Range::Chars(Pos::new(a, 0), Pos::new(b, buf.line_len(b)))
            }
            (r, _) => r,
        };
        self.mode = Mode::Normal;
        self.apply_op(buf, op, range);
        if self.mode == Mode::Normal {
            buf.clamp_cursor(false);
        }
        None
    }

    // ---- insert mode ----

    fn insert_key(&mut self, buf: &mut Buffer, key: Key) {
        let cur = buf.cursor;
        match key {
            Key::Esc => {
                if let Some(rec) = self.recording.take() {
                    self.dot = rec;
                }
                self.mode = Mode::Normal;
                buf.cursor.col = cur.col.saturating_sub(1);
                buf.clamp_cursor(false);
                buf.want_col = buf.visual_col(buf.cursor.line, buf.cursor.col);
                return;
            }
            Key::Char(c) => buf.cursor = buf.insert(cur, &c.to_string()),
            Key::Tab => {
                let v = buf.visual_col(cur.line, cur.col);
                let text = if expand_tab() { " ".repeat(tab_size() - v % tab_size()) } else { "\t".into() };
                buf.cursor = buf.insert(cur, &text);
            }
            Key::Enter => {
                let mut indent: String = buf.indent_of(cur.line).into_iter().take(cur.col).collect();
                let before = buf.lines[cur.line][..cur.col].iter().rev().find(|c| !c.is_whitespace());
                let after = buf.lines[cur.line].get(cur.col).copied();
                let opens = matches!(before, Some('{' | '(' | '[' | ':'));
                if opens {
                    let closes = matches!((before, after), (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']')));
                    let inner = format!("{indent}{}", indent_unit());
                    let end = buf.insert(cur, &format!("\n{inner}"));
                    if closes {
                        buf.insert(end, &format!("\n{indent}"));
                    }
                    buf.cursor = end;
                } else {
                    // Drop trailing whitespace left on the line being split.
                    let line = &buf.lines[cur.line];
                    let mut s = cur.col;
                    while s > 0 && line[s - 1].is_whitespace() {
                        s -= 1;
                    }
                    if s == 0 {
                        indent.clear();
                        indent.extend(&buf.lines[cur.line][..cur.col]);
                        s = cur.col;
                    }
                    buf.delete(Pos::new(cur.line, s), cur);
                    buf.cursor = buf.insert(Pos::new(cur.line, s), &format!("\n{indent}"));
                }
            }
            Key::Backspace => {
                if cur.col > 0 {
                    let line = &buf.lines[cur.line];
                    let all_spaces = line[..cur.col].iter().all(|c| *c == ' ');
                    let n = if all_spaces { (cur.col - 1) % tab_size() + 1 } else { 1 };
                    let a = Pos::new(cur.line, cur.col - n);
                    buf.delete(a, cur);
                    buf.cursor = a;
                } else if cur.line > 0 {
                    let a = Pos::new(cur.line - 1, buf.line_len(cur.line - 1));
                    buf.delete(a, cur);
                    buf.cursor = a;
                }
            }
            Key::Delete => {
                if let Some(n) = buf.next_pos(cur) {
                    buf.delete(cur, n);
                }
            }
            Key::Ctrl('w') => {
                let a = word_back(buf, cur, false);
                let a = if a.line < cur.line { Pos::new(cur.line, 0) } else { a };
                if a == cur && cur.col == 0 {
                    self.insert_key(buf, Key::Backspace);
                } else {
                    buf.delete(a, cur);
                    buf.cursor = a;
                }
            }
            Key::Ctrl('u') => {
                let a = Pos::new(cur.line, buf.first_non_blank(cur.line).min(cur.col));
                buf.delete(a, cur);
                buf.cursor = a;
            }
            Key::Left => buf.cursor.col = cur.col.saturating_sub(1),
            Key::Right => buf.cursor.col = (cur.col + 1).min(buf.line_len(cur.line)),
            Key::Up | Key::Down => {
                let line = if key == Key::Up { cur.line.saturating_sub(1) } else { (cur.line + 1).min(buf.last_line()) };
                buf.cursor = Pos::new(line, buf.col_from_visual(line, buf.want_col));
                return;
            }
            Key::Home => buf.cursor.col = buf.first_non_blank(cur.line),
            Key::End => buf.cursor.col = buf.line_len(cur.line),
            _ => return,
        }
        buf.want_col = buf.visual_col(buf.cursor.line, buf.cursor.col);
    }

    // ---- command line ----

    fn cmdline_key(&mut self, buf: &mut Buffer, key: Key) -> Option<Action> {
        match key {
            Key::Esc => self.mode = Mode::Normal,
            Key::Char(c) => self.cmdline.push(c),
            Key::Backspace => {
                if self.cmdline.pop().is_none() {
                    self.mode = Mode::Normal;
                }
            }
            Key::Ctrl('u') => self.cmdline.clear(),
            Key::Up | Key::Down if self.mode == Mode::Command => {
                if key == Key::Up {
                    self.history_idx = self.history_idx.saturating_sub(1);
                } else {
                    self.history_idx = (self.history_idx + 1).min(self.history.len());
                }
                self.cmdline = self.history.get(self.history_idx).cloned().unwrap_or_default();
            }
            Key::Enter => {
                let line = std::mem::take(&mut self.cmdline);
                if self.mode == Mode::Search {
                    self.mode = Mode::Normal;
                    if !line.is_empty() {
                        self.search = Some(line);
                    }
                    self.highlight_search = true;
                    let forward = !self.search_backward;
                    self.search_next(buf, forward);
                } else {
                    self.mode = Mode::Normal;
                    if !line.trim().is_empty() {
                        self.history.retain(|h| *h != line);
                        self.history.push(line.clone());
                    }
                    let action = self.ex_command(buf, &line);
                    buf.clamp_cursor(false);
                    return action;
                }
            }
            _ => {}
        }
        None
    }

    fn ex_command(&mut self, buf: &mut Buffer, line: &str) -> Option<Action> {
        let cmd = line.trim();
        if cmd.is_empty() || cmd.starts_with('"') {
            return None;
        }
        if let Ok(n) = cmd.parse::<usize>() {
            let l = n.saturating_sub(1).min(buf.last_line());
            buf.cursor = Pos::new(l, buf.first_non_blank(l));
            return None;
        }
        if let Some(rest) = cmd.strip_prefix("%s").or_else(|| cmd.strip_prefix('s')) {
            if rest.chars().next().is_some_and(|c| !c.is_alphanumeric() && c != ' ') {
                self.substitute(buf, rest, cmd.starts_with('%'));
                return None;
            }
        }
        let (word, arg) = match cmd.split_once(char::is_whitespace) {
            Some((w, a)) => (w, Some(a.trim().to_string()).filter(|a| !a.is_empty())),
            None => (cmd, None),
        };
        if self.map_command(word, arg.as_deref()) {
            return None;
        }
        match word {
            "let" => self.let_command(arg.as_deref().unwrap_or("")),
            "source" | "so" => match arg {
                Some(path) => return Some(Action::Source(path)),
                None => self.error("E471: Argument required"),
            },
            "terminal" | "term" => return Some(Action::App("terminal")),
            "files" | "find" => return Some(Action::App("files")),
            "sidebar" | "explorer" | "Explore" | "Lex" => return Some(Action::App("sidebar")),
            "keybindings" | "keys" | "shortcuts" => return Some(Action::App("keybindings")),
            "settingsfile" => return Some(Action::App("settingsfile")),
            "w" | "w!" | "write" => return Some(Action::Write(arg)),
            "wa" | "wall" => return Some(Action::WriteAll),
            "q" | "quit" | "bd" | "close" => return Some(Action::Quit { force: false }),
            "q!" | "quit!" | "bd!" => return Some(Action::Quit { force: true }),
            "qa" | "qall" => return Some(Action::QuitAll { force: false }),
            "qa!" | "qall!" => return Some(Action::QuitAll { force: true }),
            "wq" | "x" | "wq!" | "x!" => return Some(Action::WriteQuit),
            "wqa" | "xa" | "wqall" | "xall" => return Some(Action::WriteQuitAll),
            "e" | "edit" | "o" | "tabe" | "tabnew" => match arg {
                Some(path) => return Some(Action::Edit(path)),
                None => self.error("E32: No file name"),
            },
            "vsp" | "vsplit" | "vs" | "sp" | "split" | "vnew" | "new" => return Some(Action::Split(arg)),
            "only" | "on" => return Some(Action::Window('o')),
            "bn" | "bnext" | "tabn" | "tabnext" => return Some(Action::NextTab),
            "bp" | "bprev" | "bprevious" | "tabp" | "tabprev" => return Some(Action::PrevTab),
            "set" | "se" => return Some(Action::Set(arg.unwrap_or_default())),
            "colorscheme" | "colo" => {
                return Some(Action::Set(arg.map_or("theme?".into(), |a| format!("theme={a}"))));
            }
            "settings" | "config" | "preferences" => return Some(Action::OpenSettings),
            "noh" | "nohlsearch" => self.highlight_search = false,
            "u" | "undo" => {
                buf.undo();
            }
            "red" | "redo" => {
                buf.redo();
            }
            _ => self.error(format!("E492: Not an editor command: {cmd}")),
        }
        None
    }

    /// `:map` family. Returns false if `word` is not a mapping command.
    fn map_command(&mut self, word: &str, arg: Option<&str>) -> bool {
        use MapMode::*;
        enum Kind {
            Map(bool),
            Unmap,
            Clear,
        }
        let (modes, kind): (&[MapMode], Kind) = match word {
            "map" => (&[Normal, Visual], Kind::Map(false)),
            "noremap" | "no" => (&[Normal, Visual], Kind::Map(true)),
            "nmap" | "nm" => (&[Normal], Kind::Map(false)),
            "nnoremap" | "nno" => (&[Normal], Kind::Map(true)),
            "imap" | "im" => (&[Insert], Kind::Map(false)),
            "inoremap" | "ino" => (&[Insert], Kind::Map(true)),
            "vmap" | "vm" | "xmap" | "xm" => (&[Visual], Kind::Map(false)),
            "vnoremap" | "vn" | "xnoremap" | "xn" => (&[Visual], Kind::Map(true)),
            "unmap" | "unm" => (&[Normal, Visual], Kind::Unmap),
            "nunmap" | "nun" => (&[Normal], Kind::Unmap),
            "iunmap" | "iu" => (&[Insert], Kind::Unmap),
            "vunmap" | "vu" | "xunmap" | "xu" => (&[Visual], Kind::Unmap),
            "mapclear" | "mapc" => (&[Normal, Visual], Kind::Clear),
            "nmapclear" | "nmapc" => (&[Normal], Kind::Clear),
            "imapclear" | "imapc" => (&[Insert], Kind::Clear),
            "vmapclear" | "vmapc" | "xmapclear" | "xmapc" => (&[Visual], Kind::Clear),
            _ => return false,
        };
        // Modifiers vim scripts commonly carry; they don't change anything here.
        let mut rest = arg.unwrap_or("").trim_start();
        while let Some(r) = ["<silent>", "<buffer>", "<nowait>", "<unique>"].iter().find_map(|m| rest.strip_prefix(m)) {
            rest = r.trim_start();
        }
        let (lhs, rhs) = match rest.split_once(char::is_whitespace) {
            Some((l, r)) => (l, r.trim_start()),
            None => (rest, ""),
        };
        let lhs_keys = keymap::parse_keys(lhs, &self.leader);
        match kind {
            Kind::Clear => modes.iter().for_each(|&m| self.keymaps.clear(m)),
            Kind::Unmap if lhs.is_empty() => self.error("E474: Invalid argument"),
            Kind::Unmap => {
                let removed = modes.iter().fold(false, |any, &m| self.keymaps.remove(m, &lhs_keys) || any);
                if !removed {
                    self.error(format!("E31: No such mapping: {lhs}"));
                }
            }
            Kind::Map(_) if rhs.is_empty() => self.list_mappings(modes, (!lhs.is_empty()).then_some(&lhs_keys)),
            Kind::Map(noremap) => {
                let rhs = keymap::parse_keys(rhs, &self.leader);
                for &mode in modes {
                    self.keymaps.insert(Mapping { mode, lhs: lhs_keys.clone(), rhs: rhs.clone(), noremap, origin: self.map_origin });
                }
            }
        }
        true
    }

    fn list_mappings(&mut self, modes: &[MapMode], lhs: Option<&Vec<Key>>) {
        let found: Vec<String> = self
            .keymaps
            .maps
            .iter()
            .filter(|m| modes.contains(&m.mode) && lhs.is_none_or(|l| m.lhs.starts_with(l)))
            .map(|m| format!("{} {} {}", m.mode.letter(), keymap::format_keys(&m.lhs), keymap::format_keys(&m.rhs)))
            .collect();
        if found.is_empty() {
            self.info("No mapping found");
        } else {
            self.info(found.join("   |   "));
        }
    }

    /// `:let mapleader = ","` (other variables are accepted and ignored).
    fn let_command(&mut self, arg: &str) {
        let Some((name, value)) = arg.split_once('=') else {
            self.error(format!("E15: Invalid expression: {arg}"));
            return;
        };
        let value = value.trim();
        let unquoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        if name.trim() == "mapleader" {
            // "\<Space>" in a double-quoted vim string is the key itself.
            let keys = keymap::parse_keys(&unquoted.replace("\\<", "<"), &[]);
            if keys.is_empty() {
                self.error("E15: mapleader cannot be empty");
            } else {
                self.leader = keys;
            }
        }
    }

    /// Reset what scripts set up, before reloading them.
    pub fn reset_script_state(&mut self) {
        self.keymaps.clear_origin(Origin::Script);
        self.leader = vec![Key::Char(DEFAULT_LEADER)];
    }

    /// `:s/pat/rep/[gi]` with regex patterns; `all` applies it to every line.
    /// The replacement may use `$1`/`\1` groups, `\n` and `\t`.
    fn substitute(&mut self, buf: &mut Buffer, spec: &str, all: bool) {
        let mut chars = spec.chars();
        let Some(delim) = chars.next() else { return };
        let parts = split_unescaped(chars.as_str(), delim, 3);
        let flags = parts.get(2).map(String::as_str).unwrap_or("");
        let global = flags.contains('g');
        let pat = match parts[0].as_str() {
            "" => match &self.search {
                Some(p) => p.clone(),
                None => {
                    self.error("E35: No previous regular expression");
                    return;
                }
            },
            p => p.to_string(),
        };
        let re = if flags.contains('i') { build_regex(&format!("(?i){pat}")) } else { build_regex(&pat) };
        let rep = convert_replacement(parts.get(1).map(String::as_str).unwrap_or(""));
        let lines = if all { 0..buf.line_count() } else { buf.cursor.line..buf.cursor.line + 1 };

        // Rebuild the affected range as text so replacements may add or join lines.
        let (first, last) = (lines.start, lines.end - 1);
        let mut out = Vec::new();
        let (mut subs, mut changed_lines, mut last_changed) = (0, 0, None);
        for l in first..=last {
            let text: String = buf.lines[l].iter().collect();
            let n = re.find_iter(&text).take(if global { usize::MAX } else { 1 }).count();
            if n == 0 {
                out.push(text);
                continue;
            }
            subs += n;
            changed_lines += 1;
            last_changed = Some(l);
            out.push(re.replacen(&text, if global { 0 } else { 1 }, rep.as_str()).into_owned());
        }
        let Some(l) = last_changed else {
            self.error(format!("E486: Pattern not found: {pat}"));
            return;
        };
        buf.checkpoint();
        let added = out.iter().map(|s| s.matches('\n').count()).sum::<usize>();
        let whole = first == 0 && last == buf.last_line();
        buf.delete_lines(first, last);
        if whole {
            buf.lines.clear(); // delete_lines leaves one empty line behind
        }
        buf.insert_lines(first, &out.join("\n"));
        let l = (l + added).min(buf.last_line());
        buf.cursor = Pos::new(l, buf.first_non_blank(l));
        if changed_lines > 1 {
            self.info(format!("{subs} substitutions on {changed_lines} lines"));
        }
    }

    /// The current search as a compiled regex, if highlighting is on.
    pub fn highlight_regex(&mut self) -> Option<Regex> {
        if !self.highlight_search {
            return None;
        }
        self.search_regex()
    }

    fn search_regex(&mut self) -> Option<Regex> {
        let pat = self.search.as_ref().filter(|p| !p.is_empty())?;
        match &self.search_re {
            Some((p, re)) if p == pat => Some(re.clone()),
            _ => {
                let re = build_regex(pat);
                self.search_re = Some((pat.clone(), re.clone()));
                Some(re)
            }
        }
    }

    fn search_next(&mut self, buf: &mut Buffer, forward: bool) -> bool {
        let Some(re) = self.search_regex() else {
            self.error("E35: No previous regular expression");
            return false;
        };
        let cur = buf.cursor;
        let total = buf.line_count();
        for step in 0..=total {
            let l = if forward { (cur.line + step) % total } else { (cur.line + total * 2 - step) % total };
            // The cursor line is visited twice: first after the cursor, finally before it.
            let ok = |c: usize| match (step == 0, step == total, forward) {
                (true, _, true) => c > cur.col,
                (true, _, false) => c < cur.col,
                (_, true, true) => c <= cur.col,
                (_, true, false) => c >= cur.col,
                _ => true,
            };
            let ms = search_matches(&buf.lines[l], &re).into_iter().map(|m| m.0);
            let hit = if forward {
                ms.into_iter().find(|&c| ok(c))
            } else {
                ms.into_iter().rev().find(|&c| ok(c))
            };
            if let Some(c) = hit {
                let wrapped = if forward { cur.line + step >= total } else { step > cur.line };
                if wrapped {
                    self.info(if forward { "search hit BOTTOM, continuing at TOP" } else { "search hit TOP, continuing at BOTTOM" });
                }
                buf.cursor = Pos::new(l, c);
                buf.want_col = buf.visual_col(l, c);
                return true;
            }
        }
        self.error(format!("E486: Pattern not found: {}", self.search.clone().unwrap_or_default()));
        false
    }
}

fn toggle_case(c: char) -> char {
    if c.is_uppercase() {
        c.to_lowercase().next().unwrap_or(c)
    } else {
        c.to_uppercase().next().unwrap_or(c)
    }
}

fn shift_line(buf: &mut Buffer, line: usize, right: bool) {
    if right {
        if !buf.lines[line].is_empty() {
            buf.insert(Pos::new(line, 0), &indent_unit());
        }
    } else {
        let n = match buf.lines[line].first() {
            Some('\t') => 1,
            _ => buf.lines[line].iter().take(tab_size()).take_while(|c| **c == ' ').count(),
        };
        if n > 0 {
            buf.delete(Pos::new(line, 0), Pos::new(line, n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, keys: &str) -> (Buffer, Vim) {
        let mut buf = Buffer::from_str(text);
        let mut vim = Vim::default();
        feed(&mut buf, &mut vim, keys);
        (buf, vim)
    }

    /// Feed keys; `<esc>`, `<cr>`, `<bs>` and `<c-x>` are special.
    fn feed(buf: &mut Buffer, vim: &mut Vim, keys: &str) {
        let mut rest = keys;
        while let Some(c) = rest.chars().next() {
            if c == '<' {
                if let Some(end) = rest.find('>') {
                    let name = &rest[1..end];
                    let key = match name {
                        "esc" => Some(Key::Esc),
                        "cr" => Some(Key::Enter),
                        "bs" => Some(Key::Backspace),
                        n if n.starts_with("c-") => n.chars().nth(2).map(Key::Ctrl),
                        _ => None,
                    };
                    if let Some(k) = key {
                        vim.handle_key(buf, k);
                        rest = &rest[end + 1..];
                        continue;
                    }
                }
            }
            vim.handle_key(buf, Key::Char(c));
            rest = &rest[c.len_utf8()..];
        }
    }

    #[test]
    fn delete_word_and_undo() {
        let (mut b, mut v) = run("foo bar baz", "dw");
        assert_eq!(b.text(), "bar baz");
        feed(&mut b, &mut v, "u");
        assert_eq!(b.text(), "foo bar baz");
    }

    #[test]
    fn count_and_dot_repeat() {
        let (b, _) = run("a b c d e f", "2dw.");
        assert_eq!(b.text(), "e f");
    }

    #[test]
    fn dw_on_last_word_keeps_newline() {
        let (b, _) = run("one two\nthree", "wdw");
        assert_eq!(b.text(), "one \nthree");
    }

    #[test]
    fn change_word_and_repeat() {
        let (b, _) = run("foo foo foo", "cwbar<esc>w.");
        assert_eq!(b.text(), "bar bar foo");
    }

    #[test]
    fn dd_and_paste() {
        let (b, _) = run("1\n2\n3", "ddp");
        assert_eq!(b.text(), "2\n1\n3");
    }

    #[test]
    fn yank_line_paste_above() {
        let (b, _) = run("a\nb", "jyykP");
        assert_eq!(b.text(), "b\na\nb");
    }

    #[test]
    fn change_inner_quotes_and_parens() {
        let (b, _) = run("say(\"hello\", x)", "fhci\"bye<esc>");
        assert_eq!(b.text(), "say(\"bye\", x)");
        let (b, _) = run("f(a, b)", "fadi(");
        assert_eq!(b.text(), "f()");
    }

    #[test]
    fn change_inner_word_on_empty_line_inserts() {
        let (b, _) = run("a\n\nb", "jciwx<esc>");
        assert_eq!(b.text(), "a\nx\nb");
    }

    #[test]
    fn delete_inner_block_on_own_lines() {
        let (b, _) = run("fn x() {\n    a;\n    b;\n}", "jdi{");
        assert_eq!(b.text(), "fn x() {\n}");
    }

    #[test]
    fn insert_open_and_auto_indent() {
        let (b, _) = run("    foo", "oba<cr>r<esc>");
        assert_eq!(b.text(), "    foo\n    ba\n    r");
        let (b, _) = run("fn a() {}", "f}i<cr>x<esc>");
        assert_eq!(b.text(), "fn a() {\n    x\n}");
    }

    #[test]
    fn visual_delete_and_line_yank() {
        let (b, _) = run("hello world", "vlld");
        assert_eq!(b.text(), "lo world");
        let (b, _) = run("a\nb\nc", "Vjyjjp");
        assert_eq!(b.text(), "a\nb\nc\na\nb");
    }

    #[test]
    fn find_and_till() {
        let (b, _) = run("a,b,c,d", "dt,");
        assert_eq!(b.text(), ",b,c,d");
        let (b, _) = run("a,b,c,d", "f,;D");
        assert_eq!(b.text(), "a,b");
    }

    #[test]
    fn search_and_next() {
        let (b, v) = run("x foo\nfoo y\nz", "/foo<cr>n");
        assert_eq!(b.cursor, Pos::new(1, 0));
        assert!(v.highlight_search);
    }

    #[test]
    fn substitute_global() {
        let (b, _) = run("a a\na", ":%s/a/b/g<cr>");
        assert_eq!(b.text(), "b b\nb");
    }

    #[test]
    fn regex_substitute_with_groups() {
        let (b, _) = run("foo(1, 2)\nfoo(3, 4)", r":%s/foo\((\d), (\d)\)/bar(\2, $1)/<cr>");
        assert_eq!(b.text(), "bar(2, 1)\nbar(4, 3)");
    }

    #[test]
    fn regex_substitute_can_split_lines_and_undo() {
        let (mut b, mut v) = run("a,b,c", r":s/,/\n/g<cr>");
        assert_eq!(b.text(), "a\nb\nc");
        feed(&mut b, &mut v, "u");
        assert_eq!(b.text(), "a,b,c");
    }

    #[test]
    fn substitute_escaped_delimiter_and_flags() {
        let (b, _) = run("a/b A/B", r":s/a\/b/x/gi<cr>");
        assert_eq!(b.text(), "x x");
    }

    #[test]
    fn regex_search_and_smartcase() {
        let (b, _) = run("x = 10\nfoo Bar 42", r"/\d{2}<cr>n");
        assert_eq!(b.cursor, Pos::new(1, 8));
        let (b, _) = run("bar\nBar", "/Bar<cr>");
        assert_eq!(b.cursor, Pos::new(1, 0));
        let (b, _) = run("xx\nBAR", "/bar<cr>");
        assert_eq!(b.cursor, Pos::new(1, 0));
    }

    #[test]
    fn invalid_regex_matches_literally() {
        let (b, _) = run("call f(x)", "/f(<cr>");
        assert_eq!(b.cursor, Pos::new(0, 5));
    }

    #[test]
    fn search_matches_reports_char_ranges() {
        let line: Vec<char> = "héllo wörld".chars().collect();
        let re = build_regex("w.r");
        assert_eq!(search_matches(&line, &re), vec![(6, 9)]);
    }

    /// Type keys through the mapping layer at time `t`.
    fn type_at(buf: &mut Buffer, vim: &mut Vim, keys: &str, t: f64) -> Vec<Action> {
        keymap::parse_keys(keys, &[]).into_iter().flat_map(|k| vim.feed(buf, k, t)).collect()
    }

    fn ex(buf: &mut Buffer, vim: &mut Vim, line: &str) {
        vim.execute(buf, line).unwrap();
    }

    #[test]
    fn insert_mapping_jk_escapes() {
        let mut b = Buffer::from_str("");
        let mut v = Vim::default();
        ex(&mut b, &mut v, "inoremap jk <Esc>");
        type_at(&mut b, &mut v, "ihijk", 0.0);
        assert_eq!(b.text(), "hi");
        assert_eq!(v.mode, Mode::Normal);
    }

    #[test]
    fn held_key_is_typed_after_timeout_or_other_key() {
        let mut b = Buffer::from_str("");
        let mut v = Vim::default();
        ex(&mut b, &mut v, "inoremap jk <Esc>");
        type_at(&mut b, &mut v, "ij", 0.0);
        assert_eq!(b.text(), "", "j waits for a possible k");
        assert_eq!(v.typeahead_deadline(), Some(MAP_TIMEOUT));
        v.flush_typeahead(&mut b);
        assert_eq!(b.text(), "j");
        type_at(&mut b, &mut v, "jx", 2.0);
        assert_eq!(b.text(), "jjx");
    }

    #[test]
    fn leader_mapping_to_ex_command() {
        let mut b = Buffer::from_str("a");
        let mut v = Vim::default();
        ex(&mut b, &mut v, r#"let mapleader = "<Space>""#);
        ex(&mut b, &mut v, "nnoremap <leader>w :w<CR>");
        let actions = type_at(&mut b, &mut v, "<Space>w", 0.0);
        assert_eq!(actions, vec![Action::Write(None)]);
    }

    #[test]
    fn recursive_and_non_recursive_maps() {
        let mut b = Buffer::from_str("abcdef");
        let mut v = Vim::default();
        ex(&mut b, &mut v, "nnoremap x l");
        ex(&mut b, &mut v, "nmap Q xx"); // recursive: each x becomes l
        type_at(&mut b, &mut v, "Q", 0.0);
        assert_eq!(b.cursor.col, 2);
        assert_eq!(b.text(), "abcdef", "x was remapped, nothing deleted");
        ex(&mut b, &mut v, "nnoremap W xx"); // not remapped: real deletes
        type_at(&mut b, &mut v, "W", 0.0);
        assert_eq!(b.text(), "abef");
        ex(&mut b, &mut v, "nmap R R");
        type_at(&mut b, &mut v, "R", 0.0);
        assert_eq!(v.message.as_deref(), Some("E223: recursive mapping"));
    }

    #[test]
    fn mappings_wait_for_pending_operator() {
        let mut b = Buffer::from_str("one two");
        let mut v = Vim::default();
        ex(&mut b, &mut v, "nnoremap w b");
        type_at(&mut b, &mut v, "dw", 0.0); // `w` after `d` is a motion, not the mapping
        assert_eq!(b.text(), "two");
    }

    #[test]
    fn unmap_and_list() {
        let mut b = Buffer::from_str("");
        let mut v = Vim::default();
        ex(&mut b, &mut v, "nnoremap <silent> <C-s> :w<CR>");
        ex(&mut b, &mut v, "nmap");
        assert_eq!(v.message.as_deref(), Some("n <C-s> :w<CR>"));
        ex(&mut b, &mut v, "nunmap <C-s>");
        assert!(v.keymaps.maps.is_empty());
        assert!(v.execute(&mut b, "nunmap <C-s>").is_err());
        assert!(v.execute(&mut b, "\" a comment").unwrap().is_none());
    }

    #[test]
    fn window_commands() {
        let mut b = Buffer::from_str("");
        let mut v = Vim::default();
        v.handle_key(&mut b, Key::Ctrl('w'));
        assert_eq!(v.handle_key(&mut b, Key::Char('l')), Some(Action::Window('l')));
        v.handle_key(&mut b, Key::Ctrl('w'));
        assert_eq!(v.handle_key(&mut b, Key::Ctrl('w')), Some(Action::Window('w')));
        feed(&mut b, &mut v, ":vsp src/a.rs");
        assert_eq!(v.handle_key(&mut b, Key::Enter), Some(Action::Split(Some("src/a.rs".into()))));
    }

    #[test]
    fn ex_commands_return_actions() {
        let mut b = Buffer::from_str("");
        let mut v = Vim::default();
        feed(&mut b, &mut v, ":w");
        assert_eq!(v.handle_key(&mut b, Key::Enter), Some(Action::Write(None)));
        feed(&mut b, &mut v, ":q!");
        assert_eq!(v.handle_key(&mut b, Key::Enter), Some(Action::Quit { force: true }));
    }

    #[test]
    fn join_and_indent() {
        let (b, _) = run("a\n    b", "J");
        assert_eq!(b.text(), "a b");
        let (b, _) = run("a\nb", ">j");
        assert_eq!(b.text(), "    a\n    b");
    }

    #[test]
    fn goto_line_and_end() {
        let (b, _) = run("a\nb\nc", "G");
        assert_eq!(b.cursor.line, 2);
        let (b, _) = run("a\nb\nc", "2gg");
        assert_eq!(b.cursor.line, 1);
    }
}
