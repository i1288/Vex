//! Dev aid for checking the UI without a screenshot tool:
//!
//! `VEX_SNAPSHOT=out.png VEX_KEYS='jjdd<wait>:w<cr>' vex [path]`
//!
//! replays the keys as real input (`<esc> <cr> <bs> <tab> <c-x> <cs-x>`; `<wait>`
//! lets a few frames pass, at least `VEX_SNAPSHOT_DELAY_MS`), saves a PNG of the
//! window and exits.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, ColorImage, Event, Key, Modifiers};

pub struct Snapshot {
    out: PathBuf,
    groups: Vec<Vec<Event>>,
    frame: u32,
    requested: bool,
    delay: Duration,
    last_step: Instant,
    step: usize,
}

impl Snapshot {
    pub fn from_env() -> Option<Self> {
        let out = PathBuf::from(std::env::var_os("VEX_SNAPSHOT")?);
        let keys = std::env::var("VEX_KEYS").unwrap_or_default();
        let groups = keys.split("<wait>").map(parse_keys).collect();
        let delay = std::env::var("VEX_SNAPSHOT_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        Some(Self {
            out,
            groups,
            frame: 0,
            requested: false,
            delay: Duration::from_millis(delay),
            last_step: Instant::now(),
            step: 0,
        })
    }

    /// Inject this frame's input; request the screenshot once all keys are in.
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        ctx.request_repaint();
        if self.frame < 5 || self.frame % 3 != 0 || self.last_step.elapsed() < self.delay {
            return;
        }
        self.last_step = Instant::now();
        self.step += 1;
        {
            let i = self.step - 1;
            if let Some(group) = self.groups.get(i) {
                ctx.input_mut(|inp| inp.events.extend(group.iter().cloned()));
                return;
            }
            if !self.requested && i >= self.groups.len() + 1 {
                self.requested = true;
                let out = self.out.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(
                    move |img: Arc<ColorImage>| {
                        if let Err(e) = std::fs::write(&out, encode_png(&img)) {
                            eprintln!("snapshot: {e}");
                        }
                        std::process::exit(0);
                    },
                )));
            }
        }
    }
}

fn key_event(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn parse_keys(s: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            if let Some(end) = rest.find('>') {
                let name = &rest[1..end];
                let ev = match name {
                    "esc" => Some(key_event(Key::Escape, Modifiers::NONE)),
                    "cr" => Some(key_event(Key::Enter, Modifiers::NONE)),
                    "bs" => Some(key_event(Key::Backspace, Modifiers::NONE)),
                    "tab" => Some(key_event(Key::Tab, Modifiers::NONE)),
                    "down" => Some(key_event(Key::ArrowDown, Modifiers::NONE)),
                    n if n.starts_with("c-") || n.starts_with("cs-") => {
                        let (mods, k) = match n.strip_prefix("cs-") {
                            Some(k) => (Modifiers::COMMAND | Modifiers::SHIFT, k),
                            None => (Modifiers::COMMAND, &n[2..]),
                        };
                        Key::from_name(&k.to_uppercase()).map(|k| key_event(k, mods))
                    }
                    _ => None,
                };
                if let Some(ev) = ev {
                    out.push(ev);
                    rest = &rest[end + 1..];
                    continue;
                }
            }
        }
        out.push(Event::Text(c.to_string()));
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Minimal PNG encoder (RGBA, stored/uncompressed deflate).
fn encode_png(img: &ColorImage) -> Vec<u8> {
    let [w, h] = img.size;
    let mut raw = Vec::with_capacity(h * (w * 4 + 1));
    for row in img.pixels.chunks(w) {
        raw.push(0);
        for p in row {
            raw.extend_from_slice(&p.to_array());
        }
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = b.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    for (kind, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut chunk = kind.to_vec();
        chunk.extend_from_slice(&data);
        png.extend_from_slice(&chunk);
        png.extend_from_slice(&crc32(&chunk).to_be_bytes());
    }
    png
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = !0u32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}
