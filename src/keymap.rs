//! Vim-style key mappings: key notation (`<CR>`, `<C-s>`, `<Leader>`) and the
//! mapping table the editor consults as keys are typed.

use crate::vim::Key;

/// Which modes a mapping applies in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapMode {
    Normal,
    Insert,
    Visual,
}

impl MapMode {
    pub fn letter(self) -> char {
        match self {
            MapMode::Normal => 'n',
            MapMode::Insert => 'i',
            MapMode::Visual => 'v',
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            MapMode::Normal => "normal",
            MapMode::Insert => "insert",
            MapMode::Visual => "visual",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "normal" | "n" => Some(MapMode::Normal),
            "insert" | "i" => Some(MapMode::Insert),
            "visual" | "v" | "x" => Some(MapMode::Visual),
            _ => None,
        }
    }
}

/// Where a mapping was defined; each source is reloaded independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Typed at the `:` prompt this session.
    Runtime,
    /// From `init.vex` (or a `:source`d file).
    Script,
    /// From the Keyboard Shortcuts panel.
    Panel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    pub mode: MapMode,
    pub lhs: Vec<Key>,
    pub rhs: Vec<Key>,
    /// `noremap`: the result is not mapped again.
    pub noremap: bool,
    pub origin: Origin,
}

pub enum Lookup {
    None,
    /// The keys so far start one or more mappings; wait for more.
    Prefix,
    /// A mapping matches exactly; `ambiguous` if a longer one could also match.
    Exact { index: usize, ambiguous: bool },
}

#[derive(Default)]
pub struct Keymaps {
    pub maps: Vec<Mapping>,
}

impl Keymaps {
    /// Add or replace the mapping for `lhs` in `mode`.
    pub fn insert(&mut self, m: Mapping) {
        self.maps.retain(|x| !(x.mode == m.mode && x.lhs == m.lhs));
        self.maps.push(m);
    }

    pub fn remove(&mut self, mode: MapMode, lhs: &[Key]) -> bool {
        let before = self.maps.len();
        self.maps.retain(|x| !(x.mode == mode && x.lhs == lhs));
        self.maps.len() != before
    }

    pub fn clear(&mut self, mode: MapMode) {
        self.maps.retain(|x| x.mode != mode);
    }

    pub fn clear_origin(&mut self, origin: Origin) {
        self.maps.retain(|x| x.origin != origin);
    }

    /// Match the start of `keys` (the typed-ahead keys) against the mappings:
    /// the longest mapping that `keys` begins with, and whether `keys` could
    /// still grow into a longer one.
    pub fn lookup(&self, mode: MapMode, keys: &[Key]) -> Lookup {
        let mut exact: Option<usize> = None;
        let mut longer = false;
        for (i, m) in self.maps.iter().enumerate().filter(|(_, m)| m.mode == mode && !m.lhs.is_empty()) {
            if m.lhs.len() > keys.len() {
                longer |= m.lhs.starts_with(keys);
            } else if keys.starts_with(&m.lhs) && exact.is_none_or(|e| self.maps[e].lhs.len() < m.lhs.len()) {
                exact = Some(i);
            }
        }
        match (exact, longer) {
            (Some(index), ambiguous) => Lookup::Exact { index, ambiguous },
            (None, true) => Lookup::Prefix,
            (None, false) => Lookup::None,
        }
    }
}

/// Parse vim key notation: plain characters plus `<CR>`, `<Esc>`, `<Tab>`,
/// `<BS>`, `<Del>`, `<Space>`, `<lt>`, `<Bar>`, `<Bslash>`, arrows,
/// `<Home>`/`<End>`/`<PageUp>`/`<PageDown>`, `<C-x>` and `<Leader>`.
/// Unknown `<...>` sequences are taken literally, as vim does.
pub fn parse_keys(s: &str, leader: &[Key]) -> Vec<Key> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            if let Some(end) = rest[1..].find('>').map(|e| e + 1) {
                let name = &rest[1..end];
                if let Some(keys) = named_key(name, leader) {
                    out.extend(keys);
                    rest = &rest[end + 1..];
                    continue;
                }
            }
        }
        out.push(Key::Char(c));
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn named_key(name: &str, leader: &[Key]) -> Option<Vec<Key>> {
    let lower = name.to_ascii_lowercase();
    let key = match lower.as_str() {
        "cr" | "enter" | "return" => Key::Enter,
        "esc" => Key::Esc,
        "tab" => Key::Tab,
        "bs" | "backspace" => Key::Backspace,
        "del" | "delete" => Key::Delete,
        "space" => Key::Char(' '),
        "lt" => Key::Char('<'),
        "gt" => Key::Char('>'),
        "bar" => Key::Char('|'),
        "bslash" => Key::Char('\\'),
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "leader" => return Some(leader.to_vec()),
        "c-[" => Key::Esc,
        _ => {
            let c = lower.strip_prefix("c-").filter(|r| r.chars().count() == 1)?.chars().next()?;
            if !c.is_ascii_alphabetic() {
                return None;
            }
            Key::Ctrl(c)
        }
    };
    Some(vec![key])
}

/// Format keys in the notation `parse_keys` reads.
pub fn format_keys(keys: &[Key]) -> String {
    keys.iter()
        .map(|k| match k {
            Key::Char(' ') => "<Space>".into(),
            Key::Char('<') => "<lt>".into(),
            Key::Char(c) => c.to_string(),
            Key::Ctrl(c) => format!("<C-{c}>"),
            Key::Esc => "<Esc>".into(),
            Key::Enter => "<CR>".into(),
            Key::Backspace => "<BS>".into(),
            Key::Delete => "<Del>".into(),
            Key::Tab => "<Tab>".into(),
            Key::Left => "<Left>".into(),
            Key::Right => "<Right>".into(),
            Key::Up => "<Up>".into(),
            Key::Down => "<Down>".into(),
            Key::Home => "<Home>".into(),
            Key::End => "<End>".into(),
            Key::PageUp => "<PageUp>".into(),
            Key::PageDown => "<PageDown>".into(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_notation() {
        let leader = [Key::Char(' ')];
        assert_eq!(
            parse_keys("<Leader>w:w<CR>", &leader),
            vec![Key::Char(' '), Key::Char('w'), Key::Char(':'), Key::Char('w'), Key::Enter]
        );
        assert_eq!(parse_keys("<C-S><esc>", &leader), vec![Key::Ctrl('s'), Key::Esc]);
        assert_eq!(parse_keys("a<b", &leader), vec![Key::Char('a'), Key::Char('<'), Key::Char('b')]);
        assert_eq!(parse_keys("<nope>", &leader).len(), 6, "unknown names are literal");
    }

    #[test]
    fn format_round_trips() {
        let keys = parse_keys("<Space>x<lt><C-w>l<CR>", &[]);
        assert_eq!(parse_keys(&format_keys(&keys), &[]), keys);
    }

    #[test]
    fn lookup_prefix_and_exact() {
        let mut km = Keymaps::default();
        let map = |lhs: &str| Mapping {
            mode: MapMode::Insert,
            lhs: parse_keys(lhs, &[]),
            rhs: vec![Key::Esc],
            noremap: true,
            origin: Origin::Runtime,
        };
        km.insert(map("jk"));
        km.insert(map("jkl"));
        let j = parse_keys("j", &[]);
        assert!(matches!(km.lookup(MapMode::Insert, &j), Lookup::Prefix));
        assert!(matches!(km.lookup(MapMode::Insert, &parse_keys("jk", &[])), Lookup::Exact { ambiguous: true, .. }));
        assert!(matches!(km.lookup(MapMode::Insert, &parse_keys("jkl", &[])), Lookup::Exact { ambiguous: false, .. }));
        assert!(matches!(km.lookup(MapMode::Normal, &j), Lookup::None));
        km.insert(map("jk")); // replaces, not duplicates
        assert_eq!(km.maps.len(), 2);
    }
}
