//! Integrated terminal: a shell on a pseudo-terminal, with output parsed by
//! `vt100` into a screen grid the panel draws.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{Color32, Key as EKey, Modifiers};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::theme::Palette;

const SCROLLBACK: usize = 5000;

pub struct Terminal {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    exited: Arc<AtomicBool>,
    size: (u16, u16),
    pub name: String,
}

fn nz(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n.max(1)).expect("max(1) is nonzero")
}

impl Terminal {
    /// Start a shell in `cwd`: `shell` if given, else `$SHELL` on Unix and
    /// PowerShell on Windows.
    pub fn spawn_shell(
        cwd: &Path,
        shell: Option<&str>,
        rows: u16,
        cols: u16,
        notify: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        if let Some(shell) = shell {
            let mut parts = shell.split_whitespace();
            let program = parts.next().unwrap_or(shell);
            let args: Vec<&str> = parts.collect();
            return Self::spawn(program, &args, cwd, rows, cols, notify);
        }
        if cfg!(windows) {
            return Self::spawn("powershell.exe", &["-NoLogo"], cwd, rows, cols, notify);
        }
        let fallback = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/bash" };
        let shell = std::env::var("SHELL").unwrap_or_else(|_| fallback.into());
        Self::spawn(&shell, &[], cwd, rows, cols, notify)
    }

    /// Start `program` on a new pty. `notify` runs (on a background thread)
    /// whenever output arrives or the program exits.
    pub fn spawn(
        program: &str,
        args: &[&str],
        cwd: &Path,
        rows: u16,
        cols: u16,
        notify: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let size = PtySize { rows: rows.max(1), cols: cols.max(1), pixel_width: 0, pixel_height: 0 };
        let pair = native_pty_system().openpty(size).map_err(|e| format!("cannot open pty: {e}"))?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "vex");
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("cannot start {program}: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(nz(rows), nz(cols), SCROLLBACK)));
        let exited = Arc::new(AtomicBool::new(false));
        {
            let parser = parser.clone();
            let exited = exited.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            parser.lock().unwrap().process(&buf[..n]);
                            notify();
                        }
                    }
                }
                exited.store(true, Ordering::Relaxed);
                notify();
            });
        }
        let name = Path::new(program).file_name().map_or(program.to_string(), |n| n.to_string_lossy().into_owned());
        Ok(Self { parser, writer, master: pair.master, child, exited, size: (rows, cols), name })
    }

    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::Relaxed)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
        // Typing returns the view to the live screen.
        self.parser.lock().unwrap().screen_mut().set_scrollback(0);
    }

    pub fn paste(&mut self, text: &str) {
        let bracketed = self.parser.lock().unwrap().screen().bracketed_paste();
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        if bracketed {
            self.write(format!("\x1b[200~{text}\x1b[201~").as_bytes());
        } else {
            self.write(text.as_bytes());
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(2), cols.max(10));
        if (rows, cols) == self.size {
            return;
        }
        self.size = (rows, cols);
        self.parser.lock().unwrap().set_size(nz(rows), nz(cols));
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }

    /// Scroll the view into history by `lines` (positive = back in time).
    pub fn scroll(&mut self, lines: isize) {
        let mut p = self.parser.lock().unwrap();
        if p.screen().alternate_screen() {
            return;
        }
        let cur = p.screen().scrollback() as isize;
        p.screen_mut().set_scrollback((cur + lines).max(0) as usize);
    }

    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        f(self.parser.lock().unwrap().screen())
    }

    pub fn application_cursor(&self) -> bool {
        self.with_screen(|s| s.application_cursor())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Bytes an xterm sends for a key press, or `None` for keys the terminal ignores.
pub fn encode_key(key: EKey, m: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl.
    let modp = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl);
    let csi_mod = |base: &str, final_: char| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[1;{modp}{final_}").into_bytes()
        } else if app_cursor && base == "O" {
            format!("\x1bO{final_}").into_bytes()
        } else {
            format!("\x1b[{final_}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modp > 1 { format!("\x1b[{n};{modp}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() }
    };
    if m.ctrl && !m.alt {
        let name = key.name();
        let mut chars = name.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if c.is_ascii_alphabetic() {
                return Some(vec![(c.to_ascii_uppercase() as u8) & 0x1f]);
            }
        }
        match key {
            EKey::OpenBracket => return Some(vec![0x1b]),
            EKey::Backslash => return Some(vec![0x1c]),
            EKey::CloseBracket => return Some(vec![0x1d]),
            EKey::Space => return Some(vec![0x00]),
            _ => {}
        }
    }
    Some(match key {
        EKey::Enter => b"\r".to_vec(),
        EKey::Backspace if m.ctrl || m.alt => b"\x1b\x7f".to_vec(),
        EKey::Backspace => b"\x7f".to_vec(),
        EKey::Tab if m.shift => b"\x1b[Z".to_vec(),
        EKey::Tab => b"\t".to_vec(),
        EKey::Escape => b"\x1b".to_vec(),
        EKey::ArrowUp => csi_mod("O", 'A'),
        EKey::ArrowDown => csi_mod("O", 'B'),
        EKey::ArrowRight => csi_mod("O", 'C'),
        EKey::ArrowLeft => csi_mod("O", 'D'),
        EKey::Home => csi_mod("O", 'H'),
        EKey::End => csi_mod("O", 'F'),
        EKey::Insert => tilde(2),
        EKey::Delete => tilde(3),
        EKey::PageUp => tilde(5),
        EKey::PageDown => tilde(6),
        EKey::F1 => b"\x1bOP".to_vec(),
        EKey::F2 => b"\x1bOQ".to_vec(),
        EKey::F3 => b"\x1bOR".to_vec(),
        EKey::F4 => b"\x1bOS".to_vec(),
        EKey::F5 => tilde(15),
        EKey::F6 => tilde(17),
        EKey::F7 => tilde(18),
        EKey::F8 => tilde(19),
        EKey::F9 => tilde(20),
        EKey::F10 => tilde(21),
        EKey::F11 => tilde(23),
        EKey::F12 => tilde(24),
        _ => return None,
    })
}

/// Screen color for a vt100 color; `fg` picks which default applies.
pub fn color(c: vt100::Color, fg: bool, bold: bool, p: &Palette) -> Color32 {
    match c {
        vt100::Color::Default => {
            if fg {
                p.text
            } else {
                p.panel_bg
            }
        }
        // Bold text in one of the 8 base colors uses the bright variant, like most terminals.
        vt100::Color::Idx(i) if i < 8 && bold && fg => p.ansi[i as usize + 8],
        vt100::Color::Idx(i) if i < 16 => p.ansi[i as usize],
        vt100::Color::Idx(i) if i < 232 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        vt100::Color::Idx(i) => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    #[test]
    fn runs_a_program_and_captures_output() {
        let dir = std::env::temp_dir();
        let t = Terminal::spawn("/bin/sh", &["-c", "printf 'vex \\033[31mred\\033[0m ok'"], &dir, 5, 40, || {}).unwrap();
        let start = Instant::now();
        while !t.has_exited() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let (text, red) = t.with_screen(|s| (s.contents(), s.cell(0, 4).map(|c| c.fgcolor())));
        assert!(text.contains("vex red ok"), "screen was: {text:?}");
        assert_eq!(red, Some(vt100::Color::Idx(1)));
    }

    #[cfg(unix)]
    #[test]
    fn input_reaches_the_program() {
        let dir = std::env::temp_dir();
        let mut t = Terminal::spawn("/bin/sh", &["-c", "read x; echo got:$x"], &dir, 5, 40, || {}).unwrap();
        t.write(b"hello\r");
        let start = Instant::now();
        while !t.with_screen(|s| s.contents().contains("got:hello")) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(t.with_screen(|s| s.contents()).contains("got:hello"));
    }

    #[test]
    fn key_encoding() {
        let none = Modifiers::NONE;
        assert_eq!(encode_key(EKey::Enter, none, false), Some(b"\r".to_vec()));
        assert_eq!(encode_key(EKey::ArrowUp, none, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode_key(EKey::ArrowUp, none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(encode_key(EKey::ArrowLeft, Modifiers::CTRL, false), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(encode_key(EKey::D, Modifiers::CTRL, false), Some(vec![4]));
        assert_eq!(encode_key(EKey::Delete, none, false), Some(b"\x1b[3~".to_vec()));
        assert_eq!(encode_key(EKey::A, none, false), None, "plain letters arrive as text");
    }

    #[test]
    fn color_cube() {
        let p = &crate::theme::DARK_MODERN;
        assert_eq!(color(vt100::Color::Idx(196), true, false, p), Color32::from_rgb(255, 0, 0));
        assert_eq!(color(vt100::Color::Idx(232), true, false, p), Color32::from_rgb(8, 8, 8));
        assert_eq!(color(vt100::Color::Idx(1), true, true, p), p.ansi[9]);
    }
}
