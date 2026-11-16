//! The Keyboard Shortcuts panel: rebind app shortcuts by pressing keys, and
//! manage simple Vim mappings without writing a script.

use eframe::egui::{self, vec2, Align2, Color32, CornerRadius, FontId, Frame, Key, Rect, RichText, Sense, Stroke, Ui};

use crate::commands::{info_of, Chord, Command, Keybindings, MapEntry, COMMANDS};
use crate::keymap::{self, Mapping};
use crate::theme;

#[derive(PartialEq)]
enum Tab {
    Shortcuts,
    Mappings,
}

#[derive(Default)]
pub struct Outcome {
    pub close: bool,
    /// Shortcuts or mappings changed: save them (and re-apply mappings).
    pub changed: bool,
    pub open_script: bool,
}

pub struct KeysPanel {
    tab: Tab,
    filter: String,
    /// Waiting for the user to press the new keys for this command.
    capturing: Option<Command>,
    notice: Option<String>,
}

impl Default for KeysPanel {
    fn default() -> Self {
        Self { tab: Tab::Shortcuts, filter: String::new(), capturing: None, notice: None }
    }
}

/// Ready-made mappings offered as one-click examples.
const EXAMPLES: &[(&str, &str, &str, &str)] = &[
    ("insert", "jk", "<Esc>", "Type jk quickly to leave Insert mode"),
    ("normal", "<Space>w", ":w<CR>", "Space then w saves the file"),
    ("normal", "<Space>t", ":terminal<CR>", "Space then t toggles the terminal"),
    ("normal", "<Space>f", ":files<CR>", "Space then f opens Go to File"),
    ("normal", "H", "^", "H jumps to the start of the line"),
    ("normal", "L", "$", "L jumps to the end of the line"),
];

impl KeysPanel {
    pub fn show(&mut self, ctx: &egui::Context, keys: &mut Keybindings, script_maps: &[Mapping]) -> Outcome {
        let mut out = Outcome::default();
        let p = theme::p();

        if let Some(cmd) = self.capturing {
            self.capture(ctx, keys, cmd, &mut out);
        } else if ctx.input(|i| i.key_pressed(Key::Escape)) {
            out.close = true;
        }

        // Dim everything behind the panel; clicking outside it closes it.
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("keys-backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(120));
                if ui.allocate_rect(screen, Sense::click()).clicked() && self.capturing.is_none() {
                    out.close = true;
                }
            });

        let width = (screen.width() - 40.0).clamp(420.0, 760.0);
        let height = (screen.height() - 80.0).clamp(300.0, 560.0);
        egui::Area::new(egui::Id::unique("keys-panel"))
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .show(ctx, |ui| {
                Frame::NONE
                    .fill(p.widget_bg)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(8)
                    .inner_margin(16)
                    .shadow(egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(150) })
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.set_height(height);
                        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                        ui.spacing_mut().button_padding = vec2(8.0, 3.0);
                        self.contents(ui, keys, script_maps, &mut out);
                    });
            });
        out
    }

    fn capture(&mut self, ctx: &egui::Context, keys: &mut Keybindings, cmd: Command, out: &mut Outcome) {
        // Take every input event so nothing else reacts to the keys being recorded.
        let events = ctx.input_mut(|i| std::mem::take(&mut i.events));
        let ctrl = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
        for ev in events {
            let chord = match ev {
                egui::Event::Key { key: Key::Escape, pressed: true, modifiers, .. } if modifiers.is_none() => {
                    self.capturing = None;
                    return;
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => Chord::from_event(key, modifiers),
                // The clipboard keys arrive as clipboard events, not key presses.
                egui::Event::Copy => Chord::parse(&format!("{ctrl}+C")).expect("valid"),
                egui::Event::Cut => Chord::parse(&format!("{ctrl}+X")).expect("valid"),
                egui::Event::Paste(_) => Chord::parse(&format!("{ctrl}+V")).expect("valid"),
                _ => continue,
            };
            let taken = keys.set(cmd, chord);
            self.notice = Some(if taken.is_empty() {
                format!("{} is now {}", info_of(cmd).title, chord.label())
            } else {
                format!("{} is now {} (removed it from {})", info_of(cmd).title, chord.label(), taken.join(", "))
            });
            self.capturing = None;
            out.changed = true;
            return;
        }
    }

    fn contents(&mut self, ui: &mut Ui, keys: &mut Keybindings, script_maps: &[Mapping], out: &mut Outcome) {
        let p = theme::p();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Keyboard Shortcuts").size(18.0).color(p.text_bright));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    out.close = true;
                }
            });
        });
        ui.horizontal(|ui| {
            for (tab, label) in [(Tab::Shortcuts, "Shortcuts"), (Tab::Mappings, "Vim Mappings")] {
                let selected = self.tab == tab;
                let text = RichText::new(label).size(13.5).color(if selected { p.text_bright } else { p.text_muted });
                let resp = ui.add(egui::Label::new(text).sense(Sense::click()));
                if selected {
                    let r = resp.rect;
                    let line = Rect::from_min_max(r.left_bottom() + vec2(0.0, 2.0), r.right_bottom() + vec2(0.0, 4.0));
                    ui.painter().rect_filled(line, CornerRadius::same(1), p.accent);
                }
                if resp.clicked() {
                    self.tab = tab;
                    self.capturing = None;
                }
                ui.add_space(12.0);
            }
        });
        ui.add_space(4.0);
        ui.separator();
        match self.tab {
            Tab::Shortcuts => self.shortcuts_tab(ui, keys, out),
            Tab::Mappings => self.mappings_tab(ui, keys, script_maps, out),
        }
    }

    fn shortcuts_tab(&mut self, ui: &mut Ui, keys: &mut Keybindings, out: &mut Outcome) {
        let p = theme::p();
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Search commands or keys")
                .desired_width(f32::INFINITY),
        );
        let filter = self.filter.to_lowercase();
        let footer = 46.0;
        // Full-width rows: command, its keys, and the buttons at the right edge.
        let row = |ui: &mut Ui, h: f32, add: &mut dyn FnMut(&mut Ui, Rect)| {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(vec2(10.0, 0.0))).layout(layout), |ui| add(ui, r));
        };
        const TITLE_W: f32 = 190.0;
        ui.add_space(2.0);
        row(ui, 24.0, &mut |ui, _| {
            column_text(ui, TITLE_W, "Command", 12.0, p.text_muted);
            column_text(ui, 100.0, "Keybinding", 12.0, p.text_muted);
        });
        egui::ScrollArea::vertical().max_height(ui.available_height() - footer).auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for info in COMMANDS {
                let labels: Vec<String> = keys.chords(info.command).iter().map(Chord::label).collect();
                let matches = info.title.to_lowercase().contains(&filter) || labels.iter().any(|l| l.to_lowercase().contains(&filter));
                if !matches {
                    continue;
                }
                row(ui, 32.0, &mut |ui, r| {
                    let capturing = self.capturing == Some(info.command);
                    if capturing {
                        ui.painter().rect_filled(r, CornerRadius::same(6), p.list_active);
                    } else if ui.rect_contains_pointer(r) {
                        ui.painter().rect_filled(r, CornerRadius::same(6), p.list_hover);
                    }
                    column_text(ui, TITLE_W, info.title, 13.0, p.text_bright);
                    if capturing {
                        ui.label(RichText::new("Press the new keys…  (Esc to cancel)").color(p.focus_border));
                    } else if labels.is_empty() {
                        ui.label(RichText::new("not set").color(p.text_muted).italics());
                    } else {
                        for l in &labels {
                            key_chip(ui, l);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !labels.is_empty() && ui.button("Remove").clicked() {
                            keys.unbind(info.command);
                            self.notice = Some(format!("{} has no shortcut now", info.title));
                            out.changed = true;
                        }
                        if !keys.is_default(info.command) && ui.button("Reset").clicked() {
                            let taken = keys.reset(info.command);
                            self.notice = Some(if taken.is_empty() {
                                format!("{} reset to {}", info.title, keys.label(info.command))
                            } else {
                                format!("{} reset (removed it from {})", info.title, taken.join(", "))
                            });
                            out.changed = true;
                        }
                        if ui.button("Change").on_hover_text("Click, then press the keys you want").clicked() {
                            self.capturing = Some(info.command);
                            self.notice = None;
                        }
                    });
                });
            }
        });
        ui.separator();
        let text = self.notice.clone().unwrap_or_else(|| "Changes are saved right away to keybindings.toml in your Vex config folder.".into());
        ui.label(RichText::new(text).color(p.text_muted));
    }

    fn mappings_tab(&mut self, ui: &mut Ui, keys: &mut Keybindings, script_maps: &[Mapping], out: &mut Outcome) {
        let p = theme::p();
        ui.label(
            RichText::new(
                "Make keys do something else, Vim style. Write keys like <Space>, <CR> (Enter), <Esc>, <Tab>, \
                 <C-s> (Ctrl+S) or <Leader>. A mapping can run a command: :w<CR> saves, :terminal<CR> opens the terminal.",
            )
            .color(p.text_ui),
        );
        let mut remove = None;
        egui::ScrollArea::vertical().max_height(ui.available_height() - 150.0).auto_shrink(false).show(ui, |ui| {
            egui::Grid::new("maps-grid").num_columns(4).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                if !keys.maps.is_empty() {
                    ui.label(RichText::new("Mode").color(p.text_muted));
                    ui.label(RichText::new("When I type").color(p.text_muted));
                    ui.label(RichText::new("Do this").color(p.text_muted));
                    ui.label("");
                    ui.end_row();
                }
                for (i, m) in keys.maps.iter_mut().enumerate() {
                    let mode = keymap::MapMode::from_name(&m.mode).unwrap_or(keymap::MapMode::Normal);
                    egui::ComboBox::from_id_salt(("map-mode", i)).width(90.0).selected_text(capitalize(mode.name())).show_ui(ui, |ui| {
                        for option in ["normal", "insert", "visual"] {
                            if ui.selectable_value(&mut m.mode, option.to_string(), capitalize(option)).changed() {
                                out.changed = true;
                            }
                        }
                    });
                    let mono = FontId::monospace(13.0);
                    out.changed |= ui
                        .add(egui::TextEdit::singleline(&mut m.keys).font(mono.clone()).desired_width(150.0).hint_text("e.g. <Space>w"))
                        .changed();
                    out.changed |= ui
                        .add(egui::TextEdit::singleline(&mut m.action).font(mono).desired_width(260.0).hint_text("e.g. :w<CR>"))
                        .changed();
                    if ui.button("Delete").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if keys.maps.is_empty() {
                ui.label(RichText::new("No mappings yet. Add one below, or start from an example.").color(p.text_muted).italics());
            }
            ui.add_space(6.0);
            if ui.button("+ Add mapping").clicked() {
                keys.maps.push(MapEntry { mode: "normal".into(), keys: String::new(), action: String::new() });
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Examples").color(p.text_muted));
            for &(mode, lhs, rhs, desc) in EXAMPLES {
                let exists = keys.maps.iter().any(|m| m.mode == mode && m.keys == lhs);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!exists, egui::Button::new(if exists { "Added" } else { "Add" })).clicked() {
                        keys.maps.push(MapEntry { mode: mode.into(), keys: lhs.into(), action: rhs.into() });
                        out.changed = true;
                    }
                    ui.label(RichText::new(desc).color(p.text_ui));
                });
            }
        });
        if let Some(i) = remove {
            keys.maps.remove(i);
            out.changed = true;
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("From your init.vex script: {}", script_maps.len())).color(p.text_ui));
            if ui.button("Open init.vex").on_hover_text("Vim-style script run at startup, like a .vimrc").clicked() {
                out.open_script = true;
                out.close = true;
            }
        });
        egui::ScrollArea::vertical().id_salt("script-maps").max_height(60.0).show(ui, |ui| {
            for m in script_maps {
                ui.label(
                    RichText::new(format!("{}  {}  →  {}", m.mode.letter(), keymap::format_keys(&m.lhs), keymap::format_keys(&m.rhs)))
                        .font(FontId::monospace(12.5))
                        .color(p.text_muted),
                );
            }
        });
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// Left-aligned text in a column of fixed width.
fn column_text(ui: &mut Ui, width: f32, text: &str, size: f32, color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, text, FontId::proportional(size), color);
}

/// A key combination drawn as a keycap-style chip.
fn key_chip(ui: &mut Ui, label: &str) {
    let p = theme::p();
    let g = ui.painter().layout_no_wrap(label.to_string(), FontId::monospace(12.5), p.text_bright);
    let (r, _) = ui.allocate_exact_size(vec2(g.size().x + 14.0, 22.0), Sense::hover());
    ui.painter().rect(r, CornerRadius::same(5), p.input_bg, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    ui.painter().galley(r.center() - g.size() / 2.0, g, p.text_bright);
}
