//! The command palette (Ctrl+Shift+P): every command by name, with its
//! shortcut, plus a color-theme picker that previews themes as you move
//! through them.

use eframe::egui::{self, pos2, vec2, Align2, CornerRadius, FontId, Key, Modifiers, Sense, Ui};

use crate::commands::Command;
use crate::explorer::{fuzzy_positions, fuzzy_score};
use crate::icons::*;
use crate::theme;

/// What a palette entry does when chosen.
#[derive(Clone, PartialEq, Debug)]
pub enum Item {
    Command(Command),
    /// A `:` command line, run as if typed.
    Ex(&'static str),
    /// Open the theme list.
    ThemePicker,
    Theme(String),
    OpenSettingsFile,
    OpenInitScript,
    RevealInExplorer,
}

#[derive(Clone)]
pub struct Entry {
    pub title: String,
    /// Shortcut shown at the right ("Ctrl+B"), or a short note.
    pub detail: String,
    pub item: Item,
}

pub struct Palette {
    query: String,
    selected: usize,
    entries: Vec<Entry>,
    /// Picking a theme: the theme to go back to on Esc.
    pub theme_before: Option<String>,
    /// Titles of recently run entries, newest first (shown on top).
    recent: Vec<String>,
}

#[derive(Default)]
pub struct Outcome {
    pub run: Option<Item>,
    /// Show this theme now (moving through the theme list).
    pub preview_theme: Option<String>,
    pub close: bool,
}

impl Palette {
    pub fn new(entries: Vec<Entry>, recent: Vec<String>) -> Self {
        Self { query: String::new(), selected: 0, entries, theme_before: None, recent }
    }

    /// The theme list, starting on the current theme.
    pub fn themes(current: &str) -> Self {
        let entries: Vec<Entry> = theme::all()
            .iter()
            .map(|p| Entry {
                title: p.title.to_string(),
                detail: if p.dark { "dark".into() } else { "light".into() },
                item: Item::Theme(p.name.to_string()),
            })
            .collect();
        let selected = entries.iter().position(|e| e.item == Item::Theme(current.to_string())).unwrap_or(0);
        Self { query: String::new(), selected, entries, theme_before: Some(current.to_string()), recent: Vec::new() }
    }

    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// Entries matching the query, best first, with the matched char positions.
    fn matches(&self) -> Vec<(&Entry, Vec<usize>)> {
        if self.query.trim().is_empty() {
            let mut out: Vec<(&Entry, Vec<usize>)> = Vec::new();
            for title in &self.recent {
                if let Some(e) = self.entries.iter().find(|e| &e.title == title) {
                    out.push((e, Vec::new()));
                }
            }
            out.extend(self.entries.iter().filter(|e| !self.recent.contains(&e.title)).map(|e| (e, Vec::new())));
            return out;
        }
        let mut scored: Vec<(i32, &Entry)> =
            self.entries.iter().filter_map(|e| fuzzy_score(&self.query, &e.title).map(|s| (s, e))).collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, e)| (e, fuzzy_positions(&self.query, &e.title).unwrap_or_default())).collect()
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome {
        let mut out = Outcome::default();
        let picking_theme = self.theme_before.is_some();
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let count = self.matches().len();
        let before = self.selected;
        if down && count > 0 {
            self.selected = (self.selected + 1) % count;
        }
        if up && count > 0 {
            self.selected = (self.selected + count - 1) % count;
        }
        self.selected = self.selected.min(count.saturating_sub(1));
        let mut chosen = enter.then_some(self.selected);
        let recent_count = if self.query.trim().is_empty() { self.recent.iter().filter(|t| self.entries.iter().any(|e| &e.title == *t)).count() } else { 0 };

        overlay_frame(ctx, "palette", |ui: &mut Ui| {
            let p = theme::p();
            let hint = if picking_theme { "Select Color Theme (Up/Down to preview)" } else { "Type a command" };
            let edit = egui::TextEdit::singleline(&mut self.query)
                .hint_text(hint)
                .font(FontId::proportional(14.0))
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(8, 5));
            let r = ui.add(edit);
            r.request_focus();
            if r.changed() {
                self.selected = 0;
            }
            ui.add_space(6.0);
            let matches = self.matches();
            if matches.is_empty() {
                ui.add_space(2.0);
                ui.label(egui::RichText::new("  No matching commands").color(p.text_muted));
                ui.add_space(6.0);
            }
            let row_h = 26.0;
            // A fixed height for the rows there are (up to 12), so the list
            // never keeps an old, smaller size when the entries change.
            let list_h = row_h * matches.len().min(12) as f32;
            egui::ScrollArea::vertical()
                .id_salt(("palette-list", picking_theme))
                .min_scrolled_height(list_h)
                .max_height(list_h)
                .auto_shrink([false, true])
                .show_rows(ui, row_h, matches.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for i in range {
                        let (entry, hits) = &matches[i];
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                        let painter = ui.painter();
                        let selected = i == self.selected;
                        if selected {
                            painter.rect_filled(r, CornerRadius::same(5), p.list_selected);
                            if down || up {
                                ui.scroll_to_rect(r, None);
                            }
                        } else if resp.hovered() {
                            painter.rect_filled(r, CornerRadius::same(5), p.list_hover);
                        }
                        let color = if selected { p.text_bright } else { p.text_ui };
                        let job = highlighted_job(&entry.title, hits, FontId::proportional(13.0), color, p.text_bright);
                        let g = ui.ctx().fonts_mut(|f| f.layout_job(job));
                        painter.galley(pos2(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, color);
                        let right = r.right() - 8.0;
                        if !entry.detail.is_empty() {
                            if matches!(entry.item, Item::Command(_)) {
                                keycaps_right(painter, right, r.center().y, &entry.detail, p.text_ui);
                            } else {
                                painter.text(pos2(right, r.center().y), Align2::RIGHT_CENTER, &entry.detail, FontId::proportional(12.0), p.text_muted);
                            }
                        } else if i < recent_count {
                            painter.text(pos2(right, r.center().y), Align2::RIGHT_CENTER, "recently used", FontId::proportional(12.0), p.text_muted);
                        }
                        // A line under the recently used group.
                        if recent_count > 0 && i + 1 == recent_count && i + 1 < matches.len() {
                            let y = r.bottom() - 0.5;
                            painter.line_segment([pos2(r.left() + 6.0, y), pos2(r.right() - 6.0, y)], egui::Stroke::new(1.0, p.separator.gamma_multiply(2.0)));
                        }
                        if resp.clicked() {
                            chosen = Some(i);
                        }
                    }
                });
        });

        let matches = self.matches();
        if picking_theme && (self.selected != before || chosen.is_some()) {
            if let Some((Entry { item: Item::Theme(name), .. }, _)) = matches.get(self.selected) {
                out.preview_theme = Some(name.clone());
            }
        }
        if let Some(i) = chosen {
            if let Some((entry, _)) = matches.get(i) {
                out.run = Some(entry.item.clone());
                if !picking_theme {
                    let title = entry.title.clone();
                    self.recent.retain(|t| *t != title);
                    self.recent.insert(0, title);
                    self.recent.truncate(5);
                }
            }
            out.close = true;
        }
        if esc {
            if let Some(t) = &self.theme_before {
                out.preview_theme = Some(t.clone());
            }
            out.close = true;
        }
        out
    }
}
