//! App commands and their keyboard shortcuts, user-configurable through
//! `keybindings.toml` (edited by the Keyboard Shortcuts panel).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::PathBuf;

use eframe::egui::{Key, Modifiers};
use serde::{Deserialize, Serialize};

/// Whether shortcuts follow macOS conventions (Cmd as the main modifier).
fn is_mac() -> bool {
    #[cfg(test)]
    if let Some(mac) = tests::MAC_OVERRIDE.with(|m| m.get()) {
        return mac;
    }
    cfg!(target_os = "macos")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    QuickOpen,
    CommandLine,
    Save,
    ToggleSidebar,
    ToggleTerminal,
    ToggleWrap,
    ToggleMinimap,
    SplitEditor,
    NextTab,
    PrevTab,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    OpenSettings,
    Keybindings,
    FocusPane(u8),
}

pub struct CommandInfo {
    pub command: Command,
    /// Name in keybindings.toml.
    pub id: &'static str,
    pub title: &'static str,
    pub defaults: &'static [&'static str],
    /// Still works while the terminal has focus.
    pub in_terminal: bool,
}

const fn info(
    command: Command,
    id: &'static str,
    title: &'static str,
    defaults: &'static [&'static str],
    in_terminal: bool,
) -> CommandInfo {
    CommandInfo { command, id, title, defaults, in_terminal }
}

pub const COMMANDS: &[CommandInfo] = &[
    info(Command::QuickOpen, "quick_open", "Go to File", &["Cmd+P"], true),
    info(Command::CommandLine, "command_line", "Command Line", &["Cmd+Shift+P"], true),
    info(Command::Save, "save", "Save File", &["Cmd+S"], false),
    info(Command::ToggleSidebar, "toggle_sidebar", "Toggle Sidebar", &["Cmd+B"], true),
    info(Command::ToggleTerminal, "toggle_terminal", "Toggle Terminal", &["Ctrl+`"], true),
    info(Command::ToggleWrap, "toggle_wrap", "Toggle Word Wrap", &["Alt+Z"], false),
    info(Command::ToggleMinimap, "toggle_minimap", "Toggle Minimap", &[], false),
    info(Command::SplitEditor, "split_editor", "Split Editor", &["Cmd+\\"], false),
    info(Command::NextTab, "next_tab", "Next Tab", &["Ctrl+Tab", "Cmd+PageDown"], false),
    info(Command::PrevTab, "prev_tab", "Previous Tab", &["Ctrl+Shift+Tab", "Cmd+PageUp"], false),
    info(Command::ZoomIn, "zoom_in", "Zoom In", &["Cmd+=", "Cmd++"], true),
    info(Command::ZoomOut, "zoom_out", "Zoom Out", &["Cmd+-"], true),
    info(Command::ZoomReset, "zoom_reset", "Reset Zoom", &["Cmd+0"], true),
    info(Command::OpenSettings, "open_settings", "Open Settings File", &["Cmd+,"], true),
    info(Command::Keybindings, "keyboard_shortcuts", "Keyboard Shortcuts", &["Cmd+K"], true),
    info(Command::FocusPane(1), "focus_pane_1", "Focus Pane 1", &["Cmd+1"], true),
    info(Command::FocusPane(2), "focus_pane_2", "Focus Pane 2", &["Cmd+2"], true),
    info(Command::FocusPane(3), "focus_pane_3", "Focus Pane 3", &["Cmd+3"], true),
    info(Command::FocusPane(4), "focus_pane_4", "Focus Pane 4", &["Cmd+4"], true),
    info(Command::FocusPane(5), "focus_pane_5", "Focus Pane 5", &["Cmd+5"], true),
];

pub fn info_of(c: Command) -> &'static CommandInfo {
    COMMANDS.iter().find(|i| i.command == c).expect("every command is listed")
}

/// A key plus modifiers. `cmd` is the platform's main modifier (Ctrl, or Cmd
/// on macOS); `ctrl` is only used on macOS, where Ctrl is a separate key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

impl Chord {
    /// Parse `Cmd+Shift+P`, `Alt+Z`, `Ctrl+\``, `Cmd++`, `F5`. `Cmd` is the
    /// platform's main modifier (Ctrl except on macOS); `Ctrl` is the Control
    /// key, which on other platforms is the same thing.
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let (mods, key) = match s.strip_suffix("++") {
            Some(m) => (m, "+"),
            None => s.rsplit_once('+').filter(|(_, k)| !k.is_empty()).unwrap_or(("", s)),
        };
        let key = Key::from_name(key)
            .or_else(|| Key::from_name(&key.to_uppercase()))
            .ok_or_else(|| format!("unknown key \"{key}\" in \"{s}\""))?;
        let mut c = Chord { cmd: false, ctrl: false, alt: false, shift: false, key };
        for m in mods.split('+').filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" if is_mac() => c.ctrl = true,
                "ctrl" | "control" | "cmd" | "command" | "super" | "meta" => c.cmd = true,
                "alt" | "option" => c.alt = true,
                "shift" => c.shift = true,
                other => return Err(format!("unknown modifier \"{other}\" in \"{s}\"")),
            }
        }
        Ok(c)
    }

    pub fn from_event(key: Key, m: Modifiers) -> Self {
        Chord { cmd: m.command, ctrl: is_mac() && m.ctrl, alt: m.alt, shift: m.shift, key }
    }

    pub fn matches(&self, key: Key, m: Modifiers) -> bool {
        let ev = Self::from_event(key, m);
        // `+` usually needs Shift to type; don't require the user to say so.
        let shift_ok = ev.shift == self.shift || key == Key::Plus;
        ev.key == self.key && ev.cmd == self.cmd && ev.ctrl == self.ctrl && ev.alt == self.alt && shift_ok
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.cmd {
            parts.push(if is_mac() { "Cmd" } else { "Ctrl" });
        }
        if self.alt {
            parts.push(if is_mac() { "Option" } else { "Alt" });
        }
        if self.shift {
            parts.push("Shift");
        }
        let key = self.key.symbol_or_name();
        parts.push(key);
        parts.join("+")
    }
}

/// A Vim mapping made in the panel, saved in keybindings.toml.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct MapEntry {
    pub mode: String,
    pub keys: String,
    pub action: String,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    shortcuts: BTreeMap<String, OneOrMany>,
    #[serde(default, rename = "map", skip_serializing_if = "Vec::is_empty")]
    maps: Vec<MapEntry>,
}

pub struct Keybindings {
    bindings: HashMap<Command, Vec<Chord>>,
    pub maps: Vec<MapEntry>,
}

pub fn path() -> Option<PathBuf> {
    crate::settings::config_dir().map(|d| d.join("keybindings.toml"))
}

fn defaults(c: Command) -> Vec<Chord> {
    info_of(c).defaults.iter().map(|s| Chord::parse(s).expect("default shortcuts parse")).collect()
}

impl Default for Keybindings {
    fn default() -> Self {
        Self { bindings: COMMANDS.iter().map(|i| (i.command, defaults(i.command))).collect(), maps: Vec::new() }
    }
}

impl Keybindings {
    /// Parse keybindings.toml text; unknown entries are reported, valid ones kept.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut kb = Self::default();
        let mut errors = Vec::new();
        let file: File = match toml::from_str(text) {
            Ok(f) => f,
            Err(e) => return (kb, vec![format!("keybindings.toml: {}", e.message())]),
        };
        for (id, value) in file.shortcuts {
            let Some(info) = COMMANDS.iter().find(|i| i.id == id) else {
                errors.push(format!("keybindings.toml: unknown command \"{id}\""));
                continue;
            };
            let strings = match value {
                OneOrMany::One(s) if s.is_empty() => Vec::new(),
                OneOrMany::One(s) => vec![s],
                OneOrMany::Many(v) => v,
            };
            match strings.iter().map(|s| Chord::parse(s)).collect::<Result<Vec<_>, _>>() {
                Ok(chords) => {
                    kb.bindings.insert(info.command, chords);
                }
                Err(e) => errors.push(format!("keybindings.toml: {e}")),
            }
        }
        kb.maps = file.maps;
        (kb, errors)
    }

    pub fn load() -> (Self, Vec<String>) {
        match path().map(fs::read_to_string) {
            Some(Ok(text)) => Self::parse(&text),
            Some(Err(e)) if e.kind() != io::ErrorKind::NotFound => (Self::default(), vec![format!("keybindings.toml: {e}")]),
            _ => (Self::default(), Vec::new()),
        }
    }

    /// The file contents: only shortcuts that differ from the defaults.
    pub fn to_toml(&self) -> String {
        let mut file = File { maps: self.maps.clone(), ..Default::default() };
        for i in COMMANDS {
            let chords = &self.bindings[&i.command];
            if *chords != defaults(i.command) {
                let labels: Vec<String> = chords.iter().map(Chord::label).collect();
                let value = match labels.len() {
                    1 => OneOrMany::One(labels[0].clone()),
                    0 => OneOrMany::One(String::new()),
                    _ => OneOrMany::Many(labels),
                };
                file.shortcuts.insert(i.id.to_string(), value);
            }
        }
        let body = toml::to_string_pretty(&file).unwrap_or_default();
        format!("# Vex keyboard shortcuts and Vim mappings (edited by the Keyboard Shortcuts panel).\n\n{body}")
    }

    pub fn save(&self) -> io::Result<()> {
        let path = path().ok_or_else(|| io::Error::other("no config folder"))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.to_toml())
    }

    pub fn chords(&self, c: Command) -> &[Chord] {
        &self.bindings[&c]
    }

    pub fn is_default(&self, c: Command) -> bool {
        self.bindings[&c] == defaults(c)
    }

    /// The command bound to this key press, if any.
    pub fn lookup(&self, key: Key, m: Modifiers) -> Option<Command> {
        COMMANDS
            .iter()
            .map(|i| i.command)
            .find(|c| self.bindings[c].iter().any(|ch| ch.matches(key, m)))
    }

    /// Bind `c` to exactly `chord`, taking it away from any other command.
    /// Returns the titles of commands that lost it.
    pub fn set(&mut self, c: Command, chord: Chord) -> Vec<&'static str> {
        let mut taken = Vec::new();
        for (other, chords) in self.bindings.iter_mut() {
            if *other != c && chords.contains(&chord) {
                chords.retain(|x| *x != chord);
                taken.push(info_of(*other).title);
            }
        }
        self.bindings.insert(c, vec![chord]);
        taken
    }

    pub fn unbind(&mut self, c: Command) {
        self.bindings.insert(c, Vec::new());
    }

    pub fn reset(&mut self, c: Command) -> Vec<&'static str> {
        let mut taken = Vec::new();
        for chord in defaults(c) {
            for (other, chords) in self.bindings.iter_mut() {
                if *other != c && chords.contains(&chord) {
                    chords.retain(|x| *x != chord);
                    taken.push(info_of(*other).title);
                }
            }
        }
        self.bindings.insert(c, defaults(c));
        taken
    }

    /// The shortcut's label for menus and the welcome screen.
    pub fn label(&self, c: Command) -> String {
        self.bindings[&c].first().map_or_else(|| "—".into(), Chord::label)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        /// Lets a test act as macOS or not; each test runs on its own thread.
        pub(crate) static MAC_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
    }

    fn on_platform(mac: bool) {
        MAC_OVERRIDE.with(|m| m.set(Some(mac)));
    }

    /// Modifiers as the OS reports them for the main modifier key.
    fn main_key(mac: bool) -> Modifiers {
        if mac {
            Modifiers { mac_cmd: true, command: true, ..Default::default() }
        } else {
            Modifiers { ctrl: true, command: true, ..Default::default() }
        }
    }

    /// Modifiers for the physical Control key.
    fn control_key(mac: bool) -> Modifiers {
        if mac { Modifiers::CTRL } else { main_key(false) }
    }

    #[test]
    fn mac_defaults_use_cmd() {
        on_platform(true);
        let kb = Keybindings::default();
        assert_eq!(kb.lookup(Key::P, main_key(true)), Some(Command::QuickOpen));
        assert_eq!(kb.lookup(Key::S, main_key(true)), Some(Command::Save));
        assert_eq!(kb.lookup(Key::P, control_key(true)), None, "Control+P is not Go to File on macOS");
        assert_eq!(kb.lookup(Key::Backtick, control_key(true)), Some(Command::ToggleTerminal));
        assert_eq!(kb.lookup(Key::Backtick, main_key(true)), None, "Cmd+` is macOS's window switcher");
        assert_eq!(kb.label(Command::QuickOpen), "Cmd+P");
        assert_eq!(kb.label(Command::ToggleTerminal), "Ctrl+`");
    }

    #[test]
    fn linux_and_windows_defaults_use_ctrl() {
        on_platform(false);
        let kb = Keybindings::default();
        assert_eq!(kb.lookup(Key::P, main_key(false)), Some(Command::QuickOpen));
        assert_eq!(kb.lookup(Key::Backtick, control_key(false)), Some(Command::ToggleTerminal));
        assert_eq!(kb.label(Command::QuickOpen), "Ctrl+P");
    }

    #[test]
    fn every_default_parses_on_both_platforms() {
        for mac in [false, true] {
            on_platform(mac);
            for info in COMMANDS {
                for d in info.defaults {
                    let c = Chord::parse(d).unwrap();
                    assert_eq!(Chord::parse(&c.label()).unwrap(), c, "{d} on mac={mac}");
                }
            }
        }
    }

    #[test]
    fn parse_and_label() {
        on_platform(false);
        let c = Chord::parse("Ctrl+Shift+P").unwrap();
        assert!(c.shift && c.key == Key::P);
        assert_eq!(Chord::parse("ctrl+p").unwrap().key, Key::P);
        assert_eq!(Chord::parse("Ctrl++").unwrap().key, Key::Plus);
        assert_eq!(Chord::parse("Ctrl+`").unwrap().key, Key::Backtick);
        assert_eq!(Chord::parse("F5").unwrap(), Chord { cmd: false, ctrl: false, alt: false, shift: false, key: Key::F5 });
        assert!(Chord::parse("Ctrl+Nope").is_err());
        assert!(Chord::parse("Hyper+P").is_err());
        let c = Chord::parse("Alt+Shift+Z").unwrap();
        assert_eq!(Chord::parse(&c.label()).unwrap(), c, "labels parse back");
    }

    #[test]
    fn defaults_cover_every_command() {
        on_platform(false);
        let kb = Keybindings::default();
        assert_eq!(kb.lookup(Key::P, Modifiers::COMMAND), Some(Command::QuickOpen));
        assert_eq!(kb.lookup(Key::P, Modifiers::COMMAND | Modifiers::SHIFT), Some(Command::CommandLine));
        assert_eq!(kb.lookup(Key::Z, Modifiers::ALT), Some(Command::ToggleWrap));
        assert_eq!(kb.lookup(Key::Plus, Modifiers::COMMAND | Modifiers::SHIFT), Some(Command::ZoomIn));
        assert_eq!(kb.lookup(Key::Q, Modifiers::COMMAND), None);
    }

    #[test]
    fn rebinding_steals_and_round_trips() {
        on_platform(false);
        let mut kb = Keybindings::default();
        let taken = kb.set(Command::ToggleTerminal, Chord::parse("Ctrl+P").unwrap());
        assert_eq!(taken, vec!["Go to File"]);
        assert_eq!(kb.lookup(Key::P, Modifiers::COMMAND), Some(Command::ToggleTerminal));
        assert!(kb.chords(Command::QuickOpen).is_empty());
        kb.maps.push(MapEntry { mode: "insert".into(), keys: "jk".into(), action: "<Esc>".into() });

        let text = kb.to_toml();
        let (back, errors) = Keybindings::parse(&text);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(back.chords(Command::ToggleTerminal), kb.chords(Command::ToggleTerminal));
        assert!(back.chords(Command::QuickOpen).is_empty());
        assert!(back.is_default(Command::Save));
        assert_eq!(back.maps, kb.maps);

        kb.reset(Command::QuickOpen);
        assert!(kb.chords(Command::ToggleTerminal).is_empty(), "reset takes the default back");
    }

    #[test]
    fn bad_entries_are_reported() {
        let (kb, errors) = Keybindings::parse("[shortcuts]\nquick_open = \"Ctrl+Nope\"\nbogus = \"F1\"\n");
        assert_eq!(errors.len(), 2);
        assert!(kb.is_default(Command::QuickOpen));
    }
}
