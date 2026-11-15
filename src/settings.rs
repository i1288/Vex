//! User settings, read from `~/.config/vex/settings.toml`, edited in the
//! Settings page, and changed for the session with vim-style `:set`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{buffer, theme};

pub const CURSOR_STYLES: &[&str] = &["block", "line", "underline"];
pub const WHITESPACE_MODES: &[&str] = &["none", "boundary", "all"];

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    // Appearance
    pub theme: String,
    pub ui_scale: f32,
    pub font_size: f32,
    /// Path to a .ttf/.otf editor font; empty for the built-in one.
    pub font_family: String,
    pub line_height: f32,
    pub cursor_style: String,
    pub cursor_blink: bool,
    // Editor
    pub tab_size: usize,
    pub insert_spaces: bool,
    pub word_wrap: bool,
    pub minimap: bool,
    pub line_numbers: bool,
    pub relative_line_numbers: bool,
    pub indent_guides: bool,
    pub highlight_current_line: bool,
    pub render_whitespace: String,
    pub breadcrumbs: bool,
    pub scroll_off: usize,
    // Files
    pub auto_save: bool,
    pub auto_save_delay_ms: u64,
    pub trim_trailing_whitespace: bool,
    // Terminal
    /// 0 means the editor font size.
    pub terminal_font_size: f32,
    /// Empty for the default shell.
    pub terminal_shell: String,
    /// Color overrides on top of the theme: field name → "#rrggbb".
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            ui_scale: 1.0,
            font_size: 14.0,
            font_family: String::new(),
            line_height: 1.4,
            cursor_style: "block".into(),
            cursor_blink: false,
            tab_size: 4,
            insert_spaces: true,
            word_wrap: false,
            minimap: true,
            line_numbers: true,
            relative_line_numbers: false,
            indent_guides: true,
            highlight_current_line: true,
            render_whitespace: "none".into(),
            breadcrumbs: true,
            scroll_off: 4,
            auto_save: false,
            auto_save_delay_ms: 1000,
            trim_trailing_whitespace: false,
            terminal_font_size: 0.0,
            terminal_shell: String::new(),
            colors: BTreeMap::new(),
        }
    }
}

/// `$XDG_CONFIG_HOME/vex`, `%APPDATA%\vex` on Windows, else `~/.config/vex`.
pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| if cfg!(windows) { std::env::var_os("APPDATA").map(PathBuf::from) } else { None })
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("vex"))
}

pub fn path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("settings.toml"))
}

/// The vim-style startup script.
pub fn init_script_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("init.vex"))
}

/// Folder for user theme files.
pub fn themes_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("themes"))
}

pub fn parse(text: &str) -> Result<Settings, String> {
    let s: Settings = toml::from_str(text).map_err(|e| format!("settings.toml: {}", e.message()))?;
    s.validate()?;
    Ok(s)
}

/// Load the settings file; a missing file gives the defaults.
pub fn load() -> Result<Settings, String> {
    let Some(path) = path() else { return Ok(Settings::default()) };
    match fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Path of the settings file, creating it with the defaults if needed.
pub fn ensure_file() -> io::Result<PathBuf> {
    let path = path().ok_or_else(|| io::Error::other("no HOME or XDG_CONFIG_HOME"))?;
    if !path.exists() {
        Settings::default().save()?;
    }
    Ok(path)
}

fn quote(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

impl Settings {
    fn validate(&self) -> Result<(), String> {
        let range = |ok: bool, msg: &str| if ok { Ok(()) } else { Err(msg.to_string()) };
        range((1..=16).contains(&self.tab_size), "tab_size must be between 1 and 16")?;
        range((6.0..=48.0).contains(&self.font_size), "font_size must be between 6 and 48")?;
        range((0.5..=3.0).contains(&self.ui_scale), "ui_scale must be between 0.5 and 3")?;
        range((1.0..=3.0).contains(&self.line_height), "line_height must be between 1 and 3")?;
        range(self.terminal_font_size == 0.0 || (6.0..=48.0).contains(&self.terminal_font_size), "terminal_font_size must be 0 or between 6 and 48")?;
        range((100..=60_000).contains(&self.auto_save_delay_ms), "auto_save_delay_ms must be between 100 and 60000")?;
        if !CURSOR_STYLES.contains(&self.cursor_style.as_str()) {
            return Err(format!("cursor_style must be one of {}", CURSOR_STYLES.join(", ")));
        }
        if !WHITESPACE_MODES.contains(&self.render_whitespace.as_str()) {
            return Err(format!("render_whitespace must be one of {}", WHITESPACE_MODES.join(", ")));
        }
        Ok(())
    }

    /// Push settings that live outside the app struct (indentation, palette).
    /// Returns warnings, such as an unknown theme or color name.
    pub fn apply_globals(&self) -> Vec<String> {
        buffer::set_indent(self.tab_size, self.insert_spaces);
        theme::activate(&self.theme, &self.colors)
    }

    /// The settings file, with a comment on every option.
    pub fn to_toml(&self) -> String {
        let b = |v: bool| v.to_string();
        let mut out = String::from(
            "# Vex settings. Edit here or in the Settings page (Ctrl+,); saving applies them.\n\
             # For changes that last only this session, use :set (e.g. :set ts=2, :set rnu).\n",
        );
        let mut section = |title: &str| out.push_str(&format!("\n# ---- {title} ----\n"));
        section("Appearance");
        let mut lines: Vec<(&str, String)> = vec![
            ("Theme: dark, light, dark-plus, light-plus, monokai, dracula, one-dark, nord, gruvbox-dark,\n# tokyo-night, catppuccin-mocha, solarized-dark, solarized-light, github-light, or one from themes/", format!("theme = {}", quote(&self.theme))),
            ("Size of the whole interface (1.0 = 100%)", format!("ui_scale = {}", self.ui_scale)),
            ("Editor font size in points", format!("font_size = {:.1}", self.font_size)),
            ("Path to a monospace .ttf/.otf font; empty for the built-in font", format!("font_family = {}", quote(&self.font_family))),
            ("Line height as a multiple of the font size", format!("line_height = {}", self.line_height)),
            ("Cursor in Normal mode: block, line or underline", format!("cursor_style = {}", quote(&self.cursor_style))),
            ("Blink the cursor", format!("cursor_blink = {}", b(self.cursor_blink))),
        ];
        let flush = |out: &mut String, lines: &mut Vec<(&str, String)>| {
            for (comment, line) in lines.drain(..) {
                out.push_str(&format!("# {comment}\n{line}\n"));
            }
        };
        flush(&mut out, &mut lines);
        out.push_str("\n# ---- Editor ----\n");
        lines = vec![
            ("Width of a tab and of one indent level", format!("tab_size = {}", self.tab_size)),
            ("Indent with spaces (true) or tab characters (false)", format!("insert_spaces = {}", b(self.insert_spaces))),
            ("Wrap long lines at the window edge (Alt+Z toggles)", format!("word_wrap = {}", b(self.word_wrap))),
            ("Code overview on the right edge of the editor", format!("minimap = {}", b(self.minimap))),
            ("Show line numbers", format!("line_numbers = {}", b(self.line_numbers))),
            ("Number lines relative to the cursor", format!("relative_line_numbers = {}", b(self.relative_line_numbers))),
            ("Vertical lines at each indent level", format!("indent_guides = {}", b(self.indent_guides))),
            ("Outline the line the cursor is on", format!("highlight_current_line = {}", b(self.highlight_current_line))),
            ("Show spaces and tabs: none, boundary (leading/trailing) or all", format!("render_whitespace = {}", quote(&self.render_whitespace))),
            ("Show the file path above the editor", format!("breadcrumbs = {}", b(self.breadcrumbs))),
            ("Lines kept visible above and below the cursor", format!("scroll_off = {}", self.scroll_off)),
        ];
        flush(&mut out, &mut lines);
        out.push_str("\n# ---- Files ----\n");
        lines = vec![
            ("Save files automatically after you stop typing", format!("auto_save = {}", b(self.auto_save))),
            ("How long to wait before auto-saving, in milliseconds", format!("auto_save_delay_ms = {}", self.auto_save_delay_ms)),
            ("Remove spaces at the ends of lines when saving", format!("trim_trailing_whitespace = {}", b(self.trim_trailing_whitespace))),
        ];
        flush(&mut out, &mut lines);
        out.push_str("\n# ---- Terminal ----\n");
        lines = vec![
            ("Terminal font size; 0 uses the editor font size", format!("terminal_font_size = {:.1}", self.terminal_font_size)),
            ("Shell to run; empty for your default shell", format!("terminal_shell = {}", quote(&self.terminal_shell))),
        ];
        flush(&mut out, &mut lines);
        out.push_str("\n# Color overrides on top of the theme, e.g. keyword = \"#ff79c6\".\n# The Settings page lists every color name.\n[colors]\n");
        for (k, v) in &self.colors {
            out.push_str(&format!("{k} = {}\n", quote(v)));
        }
        out
    }

    pub fn save(&self) -> io::Result<()> {
        let path = path().ok_or_else(|| io::Error::other("no HOME or XDG_CONFIG_HOME"))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.to_toml())
    }

    /// Apply one `:set` argument: `wrap`, `nonu`, `rnu!`, `ts=2`, `theme=light`,
    /// or `ts?` to query. Returns a message to show, if any.
    pub fn set(&mut self, arg: &str) -> Result<Option<String>, String> {
        if let Some((name, value)) = arg.split_once('=') {
            let mut next = self.clone();
            let num = || value.parse::<usize>().map_err(|_| format!("E521: Number required after =: {arg}"));
            match name {
                "tabstop" | "ts" | "shiftwidth" | "sw" | "softtabstop" | "sts" => next.tab_size = num()?,
                "scrolloff" | "so" => next.scroll_off = num()?,
                "fontsize" | "fs" => next.font_size = num()? as f32,
                "theme" | "colorscheme" => {
                    let p = theme::find(value).ok_or_else(|| format!("E185: Cannot find color scheme '{value}'"))?;
                    next.theme = p.name.to_string();
                }
                _ => return Err(format!("E518: Unknown option: {name}")),
            }
            next.validate()?;
            *self = next;
            return Ok(None);
        }
        if let Some(name) = arg.strip_suffix('?') {
            let value = match name {
                "tabstop" | "ts" | "shiftwidth" | "sw" => self.tab_size.to_string(),
                "scrolloff" | "so" => self.scroll_off.to_string(),
                "fontsize" | "fs" => self.font_size.to_string(),
                "theme" | "colorscheme" => self.theme.clone(),
                _ => {
                    let on = *self.flag(name).ok_or_else(|| format!("E518: Unknown option: {name}"))?;
                    return Ok(Some(format!("{}{name}", if on { "  " } else { "no" })));
                }
            };
            return Ok(Some(format!("  {name}={value}")));
        }
        let (name, value) = if let Some(n) = arg.strip_suffix('!') {
            let cur = *self.flag(n).ok_or_else(|| format!("E518: Unknown option: {n}"))?;
            (n, !cur)
        } else if let Some(n) = arg.strip_prefix("no").filter(|n| self.flag_name(n)) {
            (n, false)
        } else {
            (arg, true)
        };
        if name == "list" {
            // Vim's 'list' shows whitespace.
            self.render_whitespace = if value { "all" } else { "none" }.into();
            return Ok(None);
        }
        *self.flag(name).ok_or_else(|| format!("E518: Unknown option: {name}"))? = value;
        Ok(None)
    }

    fn flag_name(&self, name: &str) -> bool {
        name == "list" || self.clone().flag(name).is_some()
    }

    fn flag(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "number" | "nu" => &mut self.line_numbers,
            "relativenumber" | "rnu" => &mut self.relative_line_numbers,
            "expandtab" | "et" => &mut self.insert_spaces,
            "wrap" => &mut self.word_wrap,
            "minimap" | "mm" => &mut self.minimap,
            "indentguides" | "ig" => &mut self.indent_guides,
            "cursorline" | "cul" => &mut self.highlight_current_line,
            "cursorblink" => &mut self.cursor_blink,
            "breadcrumbs" => &mut self.breadcrumbs,
            "autosave" => &mut self.auto_save,
            _ => return None,
        })
    }

    /// One-line summary for a bare `:set`.
    pub fn summary(&self) -> String {
        let no = |v: bool| if v { "" } else { "no" };
        format!(
            "theme={} fontsize={} ts={} {}expandtab {}wrap {}minimap {}number {}relativenumber so={}",
            self.theme,
            self.font_size,
            self.tab_size,
            no(self.insert_spaces),
            no(self.word_wrap),
            no(self.minimap),
            no(self.line_numbers),
            no(self.relative_line_numbers),
            self.scroll_off,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_file_reads_back() {
        assert_eq!(parse(&Settings::default().to_toml()).unwrap(), Settings::default());
        let mut s = Settings::default();
        s.theme = "dracula".into();
        s.font_family = "/fonts/My \"Mono\".ttf".into();
        s.colors.insert("keyword".into(), "#ff79c6".into());
        s.render_whitespace = "all".into();
        s.ui_scale = 1.25;
        assert_eq!(parse(&s.to_toml()).unwrap(), s);
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let s = parse("theme = \"light\"\ntab_size = 2").unwrap();
        assert_eq!(s.theme, "light");
        assert_eq!(s.tab_size, 2);
        assert!(s.line_numbers);
    }

    #[test]
    fn bad_files_are_rejected() {
        assert!(parse("tab_sise = 2").is_err());
        assert!(parse("tab_size = 0").is_err());
        assert!(parse("tab_size = \"x\"").is_err());
        assert!(parse("cursor_style = \"triangle\"").is_err());
        assert!(parse("ui_scale = 9.0").is_err());
    }

    #[test]
    fn unknown_theme_warns_when_applied() {
        // Theme files are loaded at runtime, so the name is checked on apply.
        let s = parse("theme = \"neon\"").unwrap();
        assert_eq!(s.apply_globals().len(), 1);
    }

    #[test]
    fn themes_and_minimap() {
        for name in ["dark", "light", "dark-plus", "light-plus", "dark+", "light-modern", "dracula", "Nord"] {
            let mut s = Settings::default();
            assert!(s.set(&format!("theme={name}")).is_ok(), "{name}");
        }
        let mut s = Settings::default();
        assert!(s.minimap, "minimap is on by default");
        s.set("nominimap").unwrap();
        assert!(!s.minimap);
        s.set("minimap!").unwrap();
        assert!(s.minimap);
        s.set("list").unwrap();
        assert_eq!(s.render_whitespace, "all");
        s.set("nolist").unwrap();
        assert_eq!(s.render_whitespace, "none");
    }

    #[test]
    fn set_arguments() {
        let mut s = Settings::default();
        s.set("ts=2").unwrap();
        s.set("noet").unwrap();
        s.set("rnu").unwrap();
        s.set("nu!").unwrap();
        s.set("theme=light").unwrap();
        assert_eq!((s.tab_size, s.insert_spaces, s.relative_line_numbers, s.line_numbers), (2, false, true, false));
        assert_eq!(s.theme, "light");
        assert_eq!(s.set("ts?").unwrap().as_deref(), Some("  ts=2"));
        assert_eq!(s.set("rnu?").unwrap().as_deref(), Some("  rnu"));
        assert!(s.set("ts=0").is_err());
        assert!(s.set("theme=neon").is_err());
        assert!(s.set("bogus").is_err());
        assert_eq!(s.tab_size, 2, "failed :set leaves settings unchanged");
    }
}
