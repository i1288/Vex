//! User settings, read from `~/.config/vex/settings.toml` and changed for the
//! session with vim-style `:set`.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{buffer, theme};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub theme: String,
    pub font_size: f32,
    pub tab_size: usize,
    pub insert_spaces: bool,
    pub word_wrap: bool,
    pub line_numbers: bool,
    pub relative_line_numbers: bool,
    pub indent_guides: bool,
    pub scroll_off: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            font_size: 14.0,
            tab_size: 4,
            insert_spaces: true,
            word_wrap: false,
            line_numbers: true,
            relative_line_numbers: false,
            indent_guides: true,
            scroll_off: 4,
        }
    }
}

/// Written when the settings file is first opened.
const TEMPLATE: &str = r#"# Vex settings. Save this file to apply changes.
# For changes that last only this session, use :set (e.g. :set ts=2, :set rnu).

# "dark" or "light"
theme = "dark"

# Editor font size in points (Ctrl+= / Ctrl+- zoom for the session)
font_size = 14.0

# Width of a tab and of one indent level
tab_size = 4

# Indent with spaces (true) or tab characters (false)
insert_spaces = true

# Wrap long lines at the window edge (Alt+Z toggles)
word_wrap = false

line_numbers = true
relative_line_numbers = false
indent_guides = true

# Lines kept visible above and below the cursor
scroll_off = 4
"#;

pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("vex").join("settings.toml"))
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

/// Path of the settings file, creating it from the template if needed.
pub fn ensure_file() -> io::Result<PathBuf> {
    let path = path().ok_or_else(|| io::Error::other("no HOME or XDG_CONFIG_HOME"))?;
    if !path.exists() {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&path, TEMPLATE)?;
    }
    Ok(path)
}

impl Settings {
    fn validate(&self) -> Result<(), String> {
        if !matches!(self.theme.as_str(), "dark" | "light") {
            return Err(format!("unknown theme \"{}\" (use \"dark\" or \"light\")", self.theme));
        }
        if !(1..=16).contains(&self.tab_size) {
            return Err("tab_size must be between 1 and 16".into());
        }
        if !(6.0..=48.0).contains(&self.font_size) {
            return Err("font_size must be between 6 and 48".into());
        }
        Ok(())
    }

    /// Push settings that live outside the app struct (indentation, palette).
    pub fn apply_globals(&self) {
        buffer::set_indent(self.tab_size, self.insert_spaces);
        theme::set(&self.theme);
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
                "theme" | "colorscheme" => next.theme = value.to_string(),
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
        *self.flag(name).ok_or_else(|| format!("E518: Unknown option: {name}"))? = value;
        Ok(None)
    }

    fn flag_name(&self, name: &str) -> bool {
        self.clone().flag(name).is_some()
    }

    fn flag(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "number" | "nu" => &mut self.line_numbers,
            "relativenumber" | "rnu" => &mut self.relative_line_numbers,
            "expandtab" | "et" => &mut self.insert_spaces,
            "wrap" => &mut self.word_wrap,
            "indentguides" | "ig" => &mut self.indent_guides,
            _ => return None,
        })
    }

    /// One-line summary for a bare `:set`.
    pub fn summary(&self) -> String {
        format!(
            "theme={} fontsize={} ts={} {}expandtab {}wrap {}number {}relativenumber so={}",
            self.theme,
            self.font_size,
            self.tab_size,
            if self.insert_spaces { "" } else { "no" },
            if self.word_wrap { "" } else { "no" },
            if self.line_numbers { "" } else { "no" },
            if self.relative_line_numbers { "" } else { "no" },
            self.scroll_off,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_matches_defaults() {
        assert_eq!(parse(TEMPLATE).unwrap(), Settings::default());
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
        assert!(parse("theme = \"neon\"").is_err());
        assert!(parse("tab_size = 0").is_err());
        assert!(parse("tab_size = \"x\"").is_err());
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
