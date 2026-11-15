//! The Vex window: VS Code-style layout around the modal editor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Frame, Galley, Key as EKey, Margin, Modifiers,
    Painter, Rect, Sense, Shape, Stroke, Ui,
};

use crate::buffer::{Buffer, Pos, TAB_WIDTH};
use crate::explorer::{self, Explorer};
use crate::syntax;
use crate::theme;
use crate::vim::{self, Action, Key, Mode, Vim};

const UI_SIZE: f32 = 13.0;
const DEFAULT_FONT_SIZE: f32 = 14.0;
const SCROLL_OFF: usize = 4;

struct QuickOpen {
    query: String,
    files: Vec<PathBuf>,
    selected: usize,
}

pub struct VexApp {
    tabs: Vec<Buffer>,
    active: usize,
    /// Receives keys when no file is open, so `:` commands still work.
    scratch: Buffer,
    vim: Vim,
    explorer: Explorer,
    show_sidebar: bool,
    font_size: f32,
    quick_open: Option<QuickOpen>,
    drag_anchor: Option<Pos>,
    follow_cursor: bool,
    title: String,
    allow_close: bool,
    snapshot: Option<crate::snapshot::Snapshot>,
}

impl VexApp {
    pub fn new(cc: &eframe::CreationContext<'_>, root: PathBuf, file: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            scratch: Buffer::default(),
            vim: Vim::default(),
            explorer: Explorer::new(root),
            show_sidebar: true,
            font_size: DEFAULT_FONT_SIZE,
            quick_open: None,
            drag_anchor: None,
            follow_cursor: true,
            title: String::new(),
            allow_close: false,
            snapshot: crate::snapshot::Snapshot::from_env(),
        };
        if let Some(f) = file {
            app.open(&f);
        }
        app
    }

    fn info(&mut self, msg: impl Into<String>) {
        self.vim.message = Some(msg.into());
        self.vim.message_is_error = false;
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.vim.message = Some(msg.into());
        self.vim.message_is_error = true;
    }

    fn resolve(&self, p: &Path) -> PathBuf {
        let p = match p.strip_prefix("~") {
            Ok(rest) => std::env::var_os("HOME").map_or(p.to_path_buf(), |h| PathBuf::from(h).join(rest)),
            Err(_) => p.to_path_buf(),
        };
        let p = if p.is_absolute() { p } else { self.explorer.root.join(p) };
        p.canonicalize().unwrap_or(p)
    }

    fn open(&mut self, path: &Path) {
        let path = self.resolve(path);
        if path.is_dir() {
            self.error(format!("\"{}\" is a directory", path.display()));
            return;
        }
        if let Some(i) = self.tabs.iter().position(|b| b.path.as_deref() == Some(path.as_path())) {
            self.switch_to(i);
            return;
        }
        match Buffer::open(&path) {
            Ok(b) => {
                self.tabs.push(b);
                self.switch_to(self.tabs.len() - 1);
                self.explorer.reveal(&path);
            }
            Err(e) => self.error(format!("Cannot open {}: {e}", path.display())),
        }
    }

    fn switch_to(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.active = i;
            self.vim.mode = Mode::Normal;
            self.follow_cursor = true;
        }
    }

    fn cycle_tab(&mut self, delta: isize) {
        let n = self.tabs.len() as isize;
        if n > 0 {
            self.switch_to((self.active as isize + delta).rem_euclid(n) as usize);
        }
    }

    fn close_tab(&mut self, i: usize, force: bool) -> bool {
        if i >= self.tabs.len() {
            return false;
        }
        if self.tabs[i].dirty && !force {
            self.error("E37: No write since last change (add ! to override)");
            return false;
        }
        self.tabs.remove(i);
        if self.active > i || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.vim.mode = Mode::Normal;
        self.follow_cursor = true;
        true
    }

    fn save(&mut self, i: usize, path: Option<String>) -> bool {
        let path = path.map(|p| self.resolve(Path::new(&p)));
        if self.tabs.is_empty() {
            if path.is_none() {
                self.error("E32: No file name");
                return false;
            }
            self.tabs.push(std::mem::take(&mut self.scratch));
            self.active = 0;
        }
        let buf = &mut self.tabs[i];
        if let Some(p) = path {
            buf.path = Some(p);
        }
        match buf.save() {
            Ok(bytes) => {
                let msg = format!("\"{}\" {}L, {}B written", buf.name(), buf.line_count(), bytes);
                self.info(msg);
                self.explorer.refresh();
                true
            }
            Err(e) => {
                self.error(e.to_string());
                false
            }
        }
    }

    fn quit_window(&mut self, ctx: &egui::Context) {
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_action(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Write(p) => {
                self.save(self.active, p);
            }
            Action::WriteAll => {
                for i in 0..self.tabs.len() {
                    if self.tabs[i].dirty {
                        self.save(i, None);
                    }
                }
            }
            Action::WriteQuit => {
                if self.tabs.is_empty() {
                    self.quit_window(ctx);
                } else if self.save(self.active, None) {
                    self.close_tab(self.active, true);
                }
            }
            Action::WriteQuitAll => {
                let ok = (0..self.tabs.len()).all(|i| !self.tabs[i].dirty || self.save(i, None));
                if ok {
                    self.quit_window(ctx);
                }
            }
            Action::Quit { force } => {
                if self.tabs.is_empty() {
                    self.quit_window(ctx);
                } else {
                    self.close_tab(self.active, force);
                }
            }
            Action::QuitAll { force } => {
                if let Some(b) = self.tabs.iter().find(|b| b.dirty).filter(|_| !force) {
                    let msg = format!("E162: No write since last change for buffer \"{}\"", b.name());
                    self.error(msg);
                } else {
                    self.quit_window(ctx);
                }
            }
            Action::Edit(p) => self.open(Path::new(&p)),
            Action::NextTab => self.cycle_tab(1),
            Action::PrevTab => self.cycle_tab(-1),
        }
    }

    // ---- input ---------------------------------------------------------

    fn feed(&mut self, ctx: &egui::Context, key: Key) {
        let buf = if self.tabs.is_empty() { &mut self.scratch } else { &mut self.tabs[self.active] };
        let action = self.vim.handle_key(buf, key);
        self.follow_cursor = true;
        if let Some(text) = self.vim.clipboard_out.take() {
            ctx.copy_text(text);
        }
        if let Some(a) = action {
            self.handle_action(ctx, a);
        }
    }

    /// App-level Ctrl shortcuts. Returns true if the key was used.
    fn shortcut(&mut self, ctx: &egui::Context, key: EKey, shift: bool) -> bool {
        match key {
            EKey::S => {
                if !self.tabs.is_empty() {
                    self.save(self.active, None);
                }
            }
            EKey::P if shift => {
                self.quick_open = None;
                self.vim.mode = Mode::Command;
                self.vim.cmdline.clear();
            }
            EKey::P => {
                self.quick_open = match self.quick_open {
                    Some(_) => None,
                    None => Some(QuickOpen {
                        query: String::new(),
                        files: explorer::all_files(&self.explorer.root),
                        selected: 0,
                    }),
                };
            }
            EKey::B => self.show_sidebar = !self.show_sidebar,
            EKey::Tab | EKey::PageDown if !shift => self.cycle_tab(1),
            EKey::Tab | EKey::PageUp => self.cycle_tab(-1),
            EKey::Equals | EKey::Plus => self.font_size = (self.font_size + 1.0).min(32.0),
            EKey::Minus => self.font_size = (self.font_size - 1.0).max(8.0),
            EKey::Num0 => self.font_size = DEFAULT_FONT_SIZE,
            _ => return false,
        }
        ctx.request_repaint();
        true
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if modifiers.command && !modifiers.alt && self.shortcut(ctx, key, modifiers.shift) {
                        continue;
                    }
                    if self.quick_open.is_some() {
                        continue;
                    }
                    let k = if modifiers.command {
                        match key {
                            EKey::OpenBracket => Some(Key::Esc),
                            _ => {
                                let mut name = key.name().chars();
                                match (name.next(), name.next()) {
                                    (Some(c), None) if c.is_ascii_alphabetic() => Some(Key::Ctrl(c.to_ascii_lowercase())),
                                    _ => None,
                                }
                            }
                        }
                    } else {
                        match key {
                            EKey::Escape => Some(Key::Esc),
                            EKey::Enter => Some(Key::Enter),
                            EKey::Backspace => Some(Key::Backspace),
                            EKey::Delete => Some(Key::Delete),
                            EKey::Tab => Some(Key::Tab),
                            EKey::ArrowLeft => Some(Key::Left),
                            EKey::ArrowRight => Some(Key::Right),
                            EKey::ArrowUp => Some(Key::Up),
                            EKey::ArrowDown => Some(Key::Down),
                            EKey::Home => Some(Key::Home),
                            EKey::End => Some(Key::End),
                            EKey::PageUp => Some(Key::PageUp),
                            EKey::PageDown => Some(Key::PageDown),
                            _ => None,
                        }
                    };
                    if let Some(k) = k {
                        self.feed(ctx, k);
                    }
                }
                egui::Event::Text(t) if self.quick_open.is_none() => {
                    for c in t.chars() {
                        self.feed(ctx, Key::Char(c));
                    }
                }
                egui::Event::Paste(t) if self.quick_open.is_none() => {
                    let buf = if self.tabs.is_empty() { &mut self.scratch } else { &mut self.tabs[self.active] };
                    self.vim.paste_text(buf, &t);
                    self.follow_cursor = true;
                }
                _ => {}
            }
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        let dirty: Vec<String> = self.tabs.iter().filter(|b| b.dirty).map(|b| b.name()).collect();
        if !dirty.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.error(format!("Unsaved changes in {} — :wqa to save and quit, :qa! to discard", dirty.join(", ")));
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let root = self.explorer.root_name();
        let title = match self.tabs.get(self.active) {
            Some(b) => format!("{}{} — {} — Vex", if b.dirty { "● " } else { "" }, b.name(), root),
            None => format!("{root} — Vex"),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    // ---- chrome --------------------------------------------------------

    fn activity_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let painter = ui.painter();
        let icon = |i: usize| Rect::from_min_size(pos2(rect.left(), rect.top() + i as f32 * 48.0), vec2(48.0, 48.0));

        let r = icon(0);
        let resp = ui.interact(r, ui.scope_id().with("act-explorer"), Sense::click());
        let active = self.show_sidebar;
        let color = if active || resp.hovered() { theme::TEXT_BRIGHT } else { theme::ICON_INACTIVE };
        if active {
            painter.rect_filled(Rect::from_min_size(r.min, vec2(2.0, 48.0)), 0.0, theme::TEXT_BRIGHT);
        }
        draw_files_icon(painter, r.center(), color);
        if resp.clicked() {
            self.show_sidebar = !self.show_sidebar;
        }
        resp.on_hover_text("Explorer (Ctrl+B)");

        let r = icon(1);
        let resp = ui.interact(r, ui.scope_id().with("act-search"), Sense::click());
        let color = if self.quick_open.is_some() || resp.hovered() { theme::TEXT_BRIGHT } else { theme::ICON_INACTIVE };
        draw_search_icon(painter, r.center(), color);
        if resp.clicked() {
            self.shortcut(ui.ctx(), EKey::P, false);
        }
        resp.on_hover_text("Go to File (Ctrl+P)");
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, 35.0), Sense::hover());
        ui.painter().text(
            r.left_center() + vec2(20.0, 0.0),
            Align2::LEFT_CENTER,
            "EXPLORER",
            FontId::proportional(11.0),
            theme::TEXT_UI,
        );

        let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
        let painter = ui.painter();
        draw_chevron(painter, pos2(r.left() + 12.0, r.center().y), true, theme::TEXT_UI);
        painter.text(
            r.left_center() + vec2(22.0, 0.0),
            Align2::LEFT_CENTER,
            self.explorer.root_name().to_uppercase(),
            FontId::proportional(11.0),
            theme::TEXT_UI,
        );
        if resp.on_hover_text("Click to refresh").clicked() {
            self.explorer.refresh();
        }

        let active_path = self.tabs.get(self.active).and_then(|b| b.path.clone());
        let mut clicked: Option<(PathBuf, bool)> = None;
        let rows = self.explorer.rows();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for row in &rows {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                let painter = ui.painter();
                let is_active = active_path.as_ref() == Some(&row.entry.path);
                if is_active {
                    painter.rect_filled(r, 0.0, theme::LIST_ACTIVE);
                } else if resp.hovered() {
                    painter.rect_filled(r, 0.0, theme::LIST_HOVER);
                }
                let x = r.left() + 8.0 + row.depth as f32 * 12.0;
                // Indent guides, like VS Code's tree.
                for d in 0..row.depth {
                    let gx = r.left() + 8.0 + d as f32 * 12.0 + 7.0;
                    painter.line_segment([pos2(gx, r.top()), pos2(gx, r.bottom())], Stroke::new(1.0, Color32::from_gray(0x45)));
                }
                if row.entry.is_dir {
                    draw_chevron(painter, pos2(x + 7.0, r.center().y), row.expanded, theme::TEXT_UI);
                } else {
                    draw_file_icon(painter, pos2(x + 8.0, r.center().y), theme::file_color(&row.entry.name));
                }
                painter.text(
                    pos2(x + 20.0, r.center().y),
                    Align2::LEFT_CENTER,
                    &row.entry.name,
                    FontId::proportional(UI_SIZE),
                    theme::TEXT_UI,
                );
                if resp.clicked() {
                    clicked = Some((row.entry.path.clone(), row.entry.is_dir));
                }
            }
        });
        match clicked {
            Some((p, true)) => self.explorer.toggle(&p),
            Some((p, false)) => self.open(&p),
            None => {}
        }
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 35.0), Sense::hover());
        let painter = ui.painter_at(bar);
        painter.rect_filled(bar, 0.0, theme::TABS_BG);
        let mut x = bar.left();
        let mut action: Option<(usize, bool)> = None; // (index, close)
        for (i, b) in self.tabs.iter().enumerate() {
            let active = i == self.active;
            let name = galley(ui.ctx(), b.name(), UI_SIZE, if active { theme::TEXT_BRIGHT } else { theme::TEXT_MUTED });
            let w = 12.0 + 16.0 + 4.0 + name.size().x + 8.0 + 20.0 + 6.0;
            let r = Rect::from_min_size(pos2(x, bar.top()), vec2(w, bar.height()));
            let resp = ui.interact(r, ui.scope_id().with(("tab", i)), Sense::click());
            painter.rect_filled(r, 0.0, if active { theme::EDITOR_BG } else { theme::TAB_INACTIVE });
            painter.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, theme::TABS_BG));
            if active {
                painter.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, theme::STATUS_BG));
            }
            draw_file_icon(&painter, pos2(r.left() + 20.0, r.center().y), theme::file_color(&b.name()));
            painter.galley(pos2(r.left() + 32.0, r.center().y - name.size().y / 2.0), name, theme::TEXT_UI);

            let close = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(20.0, 20.0));
            let close_resp = ui.interact(close, ui.scope_id().with(("tab-close", i)), Sense::click());
            if close_resp.hovered() {
                painter.rect_filled(close, CornerRadius::same(4), Color32::from_gray(0x50));
            }
            if b.dirty && !close_resp.hovered() {
                painter.circle_filled(close.center(), 4.0, theme::TEXT_UI);
            } else if active || resp.hovered() || close_resp.hovered() {
                draw_x(&painter, close.center(), 4.0, theme::TEXT_UI);
            }
            if close_resp.clicked() || resp.middle_clicked() {
                action = Some((i, true));
            } else if resp.clicked() {
                action = Some((i, false));
            }
            x += w;
        }
        match action {
            Some((i, true)) => {
                self.close_tab(i, false);
            }
            Some((i, false)) => self.switch_to(i),
            None => {}
        }
    }

    fn breadcrumbs(&mut self, ui: &mut Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        let painter = ui.painter_at(r);
        painter.rect_filled(r, 0.0, theme::EDITOR_BG);
        let Some(path) = self.tabs.get(self.active).and_then(|b| b.path.as_ref()) else { return };
        let rel = path.strip_prefix(&self.explorer.root).unwrap_or(path);
        let parts: Vec<String> = rel.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let mut x = r.left() + 16.0;
        for (i, part) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            if last {
                draw_file_icon(&painter, pos2(x + 6.0, r.center().y), theme::file_color(part));
                x += 16.0;
            }
            let rect = painter.text(pos2(x, r.center().y), Align2::LEFT_CENTER, part, FontId::proportional(UI_SIZE), theme::TEXT_MUTED);
            x = rect.right() + 6.0;
            if !last {
                draw_chevron(&painter, pos2(x + 3.0, r.center().y), false, theme::TEXT_MUTED);
                x += 14.0;
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let painter = ui.painter();
        let font = FontId::proportional(12.0);
        let white = theme::TEXT_BRIGHT;

        // Left: mode badge, then message.
        let mode = self.vim.mode;
        let label = galley(ui.ctx(), mode.label(), 12.0, white);
        let badge = Rect::from_min_size(rect.min, vec2(label.size().x + 20.0, rect.height()));
        painter.rect_filled(badge, 0.0, theme::mode_color(mode));
        painter.galley(badge.center() - label.size() / 2.0, label, white);
        let mut x = badge.right() + 10.0;
        if let Some(name) = self.tabs.get(self.active).map(|b| b.name()) {
            let r = painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, name, font.clone(), white);
            x = r.right() + 14.0;
        }
        if let Some(msg) = &self.vim.message {
            let g = galley(ui.ctx(), msg.clone(), 12.0, white);
            if self.vim.message_is_error {
                let bg = Rect::from_min_size(pos2(x - 6.0, rect.top() + 2.0), vec2(g.size().x + 12.0, rect.height() - 4.0));
                painter.rect_filled(bg, CornerRadius::same(3), Color32::from_rgb(0xc7, 0x2e, 0x0f));
            }
            painter.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, white);
        }

        // Right: cursor position and file info.
        let buf = self.tabs.get(self.active);
        let mut items: Vec<String> = Vec::new();
        if let Some(b) = buf {
            items.push(syntax::language_name(&b.extension()).to_string());
            items.push("LF".into());
            items.push("UTF-8".into());
            items.push("Spaces: 4".into());
            match self.vim.selection(b) {
                Some((a, e)) if mode == Mode::VisualLine => items.push(format!("{} lines", e.line - a.line + 1)),
                Some((a, e)) => items.push(format!("({} selected)", b.slice(a, e).chars().count() + 1)),
                None => {}
            }
            items.push(format!("Ln {}, Col {}", b.cursor.line + 1, b.visual_col(b.cursor.line, b.cursor.col) + 1));
        }
        let pending = self.vim.pending_keys();
        if !pending.is_empty() {
            items.push(pending);
        }
        let mut rx = rect.right() - 10.0;
        for item in items {
            let r = painter.text(pos2(rx, rect.center().y), Align2::RIGHT_CENTER, item, font.clone(), white);
            rx = r.left() - 18.0;
        }
    }

    // ---- editor --------------------------------------------------------

    fn editor(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::EDITOR_BG);
        if self.tabs.is_empty() {
            draw_welcome(&painter, rect);
            return;
        }
        let ctx = ui.ctx().clone();
        let font = FontId::monospace(self.font_size);
        let char_w = ctx.fonts_mut(|f| f.glyph_width(&font, 'M'));
        let line_h = (self.font_size * 1.4).round();
        let buf = &mut self.tabs[self.active];
        let vim = &mut self.vim;

        let digits = buf.line_count().to_string().len().max(3);
        let gutter_w = 18.0 + digits as f32 * char_w + 26.0;
        let text_rect = Rect::from_min_max(pos2(rect.left() + gutter_w, rect.top()), rect.max);
        let text_x0 = text_rect.left() + 4.0;
        vim.page_lines = ((rect.height() / line_h) as usize).max(1);

        // Scroll: wheel, then keep the cursor in view after keyboard input.
        let max_scroll = (buf.line_count().saturating_sub(1)) as f32 * line_h;
        if ui.rect_contains_pointer(rect) {
            let d = ui.input(|i| i.smooth_scroll_delta);
            buf.scroll_y -= d.y;
            buf.scroll_x -= d.x;
        }
        if self.follow_cursor {
            self.follow_cursor = false;
            let so = (SCROLL_OFF as f32 * line_h).min(rect.height() / 3.0);
            let cy = buf.cursor.line as f32 * line_h;
            if cy - so < buf.scroll_y {
                buf.scroll_y = cy - so;
            } else if cy + line_h + so > buf.scroll_y + rect.height() {
                buf.scroll_y = cy + line_h + so - rect.height();
            }
            let cx = buf.visual_col(buf.cursor.line, buf.cursor.col) as f32 * char_w;
            let text_w = text_rect.width() - 20.0;
            if cx < buf.scroll_x {
                buf.scroll_x = (cx - text_w / 3.0).max(0.0);
            } else if cx + char_w * 2.0 > buf.scroll_x + text_w {
                buf.scroll_x = cx + char_w * 2.0 - text_w + text_w / 3.0;
            }
        }
        buf.scroll_y = buf.scroll_y.clamp(0.0, max_scroll);
        buf.scroll_x = buf.scroll_x.max(0.0);
        let (scroll_x, scroll_y) = (buf.scroll_x, buf.scroll_y);

        // Mouse: click places the cursor, drag selects in visual mode.
        let hit = |buf: &Buffer, p: egui::Pos2| {
            let l = (((p.y - rect.top() + scroll_y) / line_h).max(0.0) as usize).min(buf.last_line());
            let v = ((p.x - text_x0 + scroll_x) / char_w).round().max(0.0) as usize;
            Pos::new(l, buf.col_from_visual(l, v))
        };
        if let Some(p) = resp.interact_pointer_pos() {
            let pos = hit(buf, p);
            if resp.drag_started() {
                self.drag_anchor = Some(pos);
            } else if resp.dragged() {
                if let Some(a) = self.drag_anchor.filter(|a| *a != pos) {
                    if !matches!(vim.mode, Mode::Visual | Mode::VisualLine) {
                        vim.mode = Mode::Visual;
                        vim.visual_anchor = a;
                    }
                    buf.cursor = pos;
                    buf.clamp_cursor(false);
                }
            }
            if resp.clicked() || resp.drag_started() {
                if vim.mode != Mode::Insert {
                    vim.mode = Mode::Normal;
                }
                buf.cursor = pos;
                buf.clamp_cursor(vim.mode == Mode::Insert);
                buf.want_col = buf.visual_col(pos.line, buf.cursor.col);
            }
        }
        if resp.drag_stopped() {
            self.drag_anchor = None;
        }

        let first = (scroll_y / line_h) as usize;
        let last = ((scroll_y + rect.height()) / line_h).ceil() as usize;
        let last = last.min(buf.last_line());
        let y_of = |l: usize| rect.top() + l as f32 * line_h - scroll_y;
        let x_of = |v: usize| text_x0 + v as f32 * char_w - scroll_x;
        let text_painter = painter.with_clip_rect(text_rect);
        let selection = vim.selection(buf);
        let search = vim.search.clone().filter(|s| vim.highlight_search && !s.is_empty());
        let lang = syntax::lang_for(&buf.extension());

        // Block-comment state at the first visible line.
        let mut in_block = false;
        if let Some(lang) = lang {
            for l in 0..first.min(buf.line_count()) {
                syntax::highlight(lang, &buf.lines[l], &mut in_block);
            }
        }

        for l in first..=last {
            let y = y_of(l);
            let line = &buf.lines[l];
            let row = Rect::from_min_size(pos2(text_rect.left(), y), vec2(text_rect.width(), line_h));

            if l == buf.cursor.line && selection.is_none() {
                text_painter.rect_stroke(row.shrink(1.0), 0.0, Stroke::new(2.0, theme::CURRENT_LINE), egui::StrokeKind::Inside);
            }
            if let Some((a, b)) = selection {
                if (a.line..=b.line).contains(&l) {
                    let len = line.len();
                    let (x0, x1) = if vim.mode == Mode::VisualLine {
                        (text_rect.left(), text_rect.right())
                    } else {
                        let s = if l == a.line { a.col } else { 0 };
                        let e = if l == b.line { b.col + 1 } else { len + 1 };
                        let ve = if e > len { buf.visual_col(l, len) + 1 } else { buf.visual_col(l, e) };
                        (x_of(buf.visual_col(l, s.min(len))), x_of(ve))
                    };
                    text_painter.rect_filled(Rect::from_x_y_ranges(x0..=x1, y..=y + line_h), 0.0, theme::SELECTION);
                }
            }
            if let Some(pat) = &search {
                let n = pat.chars().count();
                for c in vim::search_matches(line, pat) {
                    let r = Rect::from_x_y_ranges(
                        x_of(buf.visual_col(l, c))..=x_of(buf.visual_col(l, c + n)),
                        y + 1.0..=y + line_h - 1.0,
                    );
                    text_painter.rect_filled(r, 2.0, theme::SEARCH_MATCH);
                    text_painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_rgb(0x8a, 0x4b, 0x14)), egui::StrokeKind::Inside);
                }
            }

            // Indent guides.
            let indent = indent_width(buf, l);
            for k in 0..indent / TAB_WIDTH {
                let gx = x_of(k * TAB_WIDTH).round() + 0.5;
                text_painter.line_segment([pos2(gx, y), pos2(gx, y + line_h)], Stroke::new(1.0, theme::INDENT_GUIDE));
            }

            // Text.
            let mut job = LayoutJob::default();
            let spans = match lang {
                Some(lang) => syntax::highlight(lang, line, &mut in_block),
                None => vec![(0, line.len(), syntax::Tok::Plain)],
            };
            let mut v = 0;
            for (s, e, tok) in spans {
                let mut text = String::new();
                for &c in &line[s..e] {
                    if c == '\t' {
                        let w = TAB_WIDTH - v % TAB_WIDTH;
                        text.extend(std::iter::repeat_n(' ', w));
                        v += w;
                    } else {
                        text.push(c);
                        v += 1;
                    }
                }
                job.append(&text, 0.0, TextFormat::simple(font.clone(), tok.color()));
            }
            let g = ctx.fonts_mut(|f| f.layout_job(job));
            let gy = y + (line_h - g.size().y) / 2.0;
            text_painter.galley(pos2(text_x0 - scroll_x, gy), g, theme::TEXT);

            // Line number.
            let num_color = if l == buf.cursor.line { theme::LINE_NUMBER_ACTIVE } else { theme::LINE_NUMBER };
            painter.text(
                pos2(rect.left() + 18.0 + digits as f32 * char_w, y + line_h / 2.0),
                Align2::RIGHT_CENTER,
                (l + 1).to_string(),
                font.clone(),
                num_color,
            );
        }

        // Cursor.
        let c = buf.cursor;
        if (first..=last).contains(&c.line) {
            let line = &buf.lines[c.line];
            let len = line.len();
            let vx = buf.visual_col(c.line, c.col.min(len));
            let ch = line.get(c.col).copied();
            let cells = if ch == Some('\t') { TAB_WIDTH - vx % TAB_WIDTH } else { 1 };
            let y = y_of(c.line);
            let x = x_of(vx);
            match vim.mode {
                Mode::Insert => {
                    text_painter.rect_filled(Rect::from_min_size(pos2(x - 1.0, y), vec2(2.0, line_h)), 0.0, theme::CURSOR);
                }
                Mode::Command | Mode::Search => {
                    let r = Rect::from_min_size(pos2(x, y), vec2(char_w * cells as f32, line_h));
                    text_painter.rect_stroke(r, 0.0, Stroke::new(1.0, theme::CURSOR), egui::StrokeKind::Inside);
                }
                _ => {
                    let r = Rect::from_min_size(pos2(x, y), vec2(char_w * cells as f32, line_h));
                    text_painter.rect_filled(r, 0.0, theme::CURSOR);
                    if let Some(ch) = ch.filter(|c| !c.is_whitespace()) {
                        text_painter.text(pos2(x, y + line_h / 2.0), Align2::LEFT_CENTER, ch, font.clone(), theme::EDITOR_BG);
                    }
                }
            }
        }

        // Vertical scrollbar with a cursor marker, like VS Code's overview ruler.
        let total = max_scroll + rect.height();
        if total > rect.height() + 1.0 {
            let track = Rect::from_min_max(pos2(rect.right() - 14.0, rect.top()), rect.max);
            let thumb_h = (rect.height() / total * rect.height()).max(24.0);
            let range = (rect.height() - thumb_h).max(1.0);
            let sresp = ui.interact(track, ui.scope_id().with("vscroll"), Sense::click_and_drag());
            if sresp.dragged() {
                buf.scroll_y = (buf.scroll_y + sresp.drag_delta().y * max_scroll / range).clamp(0.0, max_scroll);
            }
            let ty = rect.top() + buf.scroll_y / max_scroll.max(1.0) * range;
            let thumb = Rect::from_min_size(pos2(track.left(), ty), vec2(14.0, thumb_h));
            let alpha = if sresp.dragged() || sresp.hovered() { 0x90 } else { 0x50 };
            painter.rect_filled(thumb, 0.0, Color32::from_rgba_unmultiplied(0x79, 0x79, 0x79, alpha));
            let my = rect.top() + buf.cursor.line as f32 / buf.line_count().max(1) as f32 * rect.height();
            painter.rect_filled(Rect::from_min_size(pos2(track.left(), my), vec2(14.0, 2.0)), 0.0, theme::CURSOR);
        }
    }

    // ---- overlays ------------------------------------------------------

    fn quick_open_overlay(&mut self, ctx: &egui::Context) {
        let Some(qo) = &mut self.quick_open else { return };
        let mut open: Option<PathBuf> = None;
        let mut close = ctx.input(|i| i.key_pressed(EKey::Escape));

        let mut matches: Vec<(i32, &PathBuf)> = qo
            .files
            .iter()
            .filter_map(|f| explorer::fuzzy_score(&qo.query, &f.to_string_lossy()).map(|s| (s, f)))
            .collect();
        matches.sort_by(|a, b| b.0.cmp(&a.0));
        matches.truncate(12);

        let (down, up, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, EKey::ArrowDown),
                i.consume_key(Modifiers::NONE, EKey::ArrowUp),
                i.consume_key(Modifiers::NONE, EKey::Enter),
            )
        });
        if down {
            qo.selected = (qo.selected + 1).min(matches.len().saturating_sub(1));
        }
        if up {
            qo.selected = qo.selected.saturating_sub(1);
        }
        qo.selected = qo.selected.min(matches.len().saturating_sub(1));
        if enter {
            open = matches.get(qo.selected).map(|m| m.1.clone());
            close |= open.is_some();
        }

        overlay_frame(ctx, "quick-open", |ui| {
            let te = egui::TextEdit::singleline(&mut qo.query)
                .hint_text("Search files by name")
                .font(FontId::proportional(14.0))
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(6, 4));
            let r = ui.add(te);
            r.request_focus();
            if r.changed() {
                qo.selected = 0;
            }
            ui.add_space(4.0);
            if matches.is_empty() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("  No matching files").color(theme::TEXT_MUTED));
                ui.add_space(4.0);
            }
            for (i, (_, path)) in matches.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                let painter = ui.painter();
                if i == qo.selected {
                    painter.rect_filled(r, CornerRadius::same(3), theme::LIST_SELECTED);
                } else if resp.hovered() {
                    painter.rect_filled(r, CornerRadius::same(3), theme::LIST_HOVER);
                }
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                draw_file_icon(painter, pos2(r.left() + 14.0, r.center().y), theme::file_color(&name));
                let nr = painter.text(pos2(r.left() + 26.0, r.center().y), Align2::LEFT_CENTER, &name, FontId::proportional(UI_SIZE), theme::TEXT_BRIGHT);
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    painter.text(
                        pos2(nr.right() + 8.0, r.center().y),
                        Align2::LEFT_CENTER,
                        dir.to_string_lossy(),
                        FontId::proportional(12.0),
                        theme::TEXT_MUTED,
                    );
                }
                if resp.clicked() {
                    open = Some((*path).clone());
                    close = true;
                }
            }
        });

        if close {
            self.quick_open = None;
        }
        if let Some(p) = open {
            let full = self.explorer.root.join(p);
            self.open(&full);
        }
    }

    fn cmdline_overlay(&mut self, ctx: &egui::Context) {
        let prefix = match self.vim.mode {
            Mode::Command => ":",
            Mode::Search if self.vim.search_backward => "?",
            Mode::Search => "/",
            _ => return,
        };
        let text = format!("{prefix}{}", self.vim.cmdline);
        let font = FontId::monospace(14.0);
        overlay_frame(ctx, "cmdline", |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
            let painter = ui.painter();
            painter.rect(r, CornerRadius::same(2), theme::INPUT_BG, Stroke::new(1.0, theme::FOCUS_BORDER), egui::StrokeKind::Inside);
            let tr = painter.text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, &text, font.clone(), theme::TEXT_BRIGHT);
            let cw = ctx.fonts_mut(|f| f.glyph_width(&font, 'M'));
            painter.rect_filled(Rect::from_min_size(pos2(tr.right() + 1.0, r.center().y - 9.0), vec2(cw, 18.0)), 0.0, theme::CURSOR);
            let hint = if prefix == ":" {
                "w  save  ·  q  close  ·  wq  save+close  ·  e <file>  open  ·  %s/a/b/g  replace  ·  <N>  go to line"
            } else {
                "Enter to search  ·  n / N  next / previous  ·  :noh  clear highlight"
            };
            ui.add_space(4.0);
            ui.label(egui::RichText::new(hint).size(11.5).color(theme::TEXT_MUTED));
        });
    }
}

impl eframe::App for VexApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(s) = &mut self.snapshot {
            s.tick(&ctx);
        }
        self.handle_input(&ctx);
        self.handle_close_request(&ctx);
        self.update_title(&ctx);

        egui::Panel::bottom("status")
            .exact_size(22.0)
            .resizable(false)
            .frame(Frame::NONE.fill(theme::STATUS_BG))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("activity")
            .exact_size(48.0)
            .resizable(false)
            .frame(Frame::NONE.fill(theme::ACTIVITY_BG))
            .show(ui, |ui| self.activity_bar(ui));
        if self.show_sidebar {
            egui::Panel::left("sidebar")
                .default_size(250.0)
                .size_range(160.0..=600.0)
                .resizable(true)
                .frame(Frame::NONE.fill(theme::SIDEBAR_BG))
                .show(ui, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if !self.tabs.is_empty() {
                self.tab_bar(ui);
                self.breadcrumbs(ui);
            }
            self.editor(ui);
        });

        self.quick_open_overlay(&ctx);
        self.cmdline_overlay(&ctx);

        // Keep egui widgets from holding keyboard focus; keys belong to the editor.
        if self.quick_open.is_none() {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
        }
    }
}

// ---- drawing helpers ------------------------------------------------------

fn galley(ctx: &egui::Context, text: impl Into<String>, size: f32, color: Color32) -> Arc<Galley> {
    ctx.fonts_mut(|f| f.layout_no_wrap(text.into(), FontId::proportional(size), color))
}

fn overlay_frame(ctx: &egui::Context, id: &str, add: impl FnOnce(&mut Ui)) {
    let width = (ctx.content_rect().width() * 0.6).clamp(320.0, 620.0);
    egui::Area::new(egui::Id::unique(id))
        .anchor(Align2::CENTER_TOP, vec2(0.0, 8.0))
        .order(egui::Order::Foreground)
        .fade_in(false)
        .show(ctx, |ui| {
            Frame::NONE
                .fill(theme::WIDGET_BG)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(6)
                .inner_margin(6)
                .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(140) })
                .show(ui, |ui| {
                    ui.set_width(width);
                    add(ui);
                });
        });
}

/// Visual width of a line's indentation; blank lines borrow it from neighbors
/// so indent guides run through them.
fn indent_width(buf: &Buffer, l: usize) -> usize {
    let measure = |l: usize| -> Option<usize> {
        let line = &buf.lines[l];
        let n = line.iter().take_while(|c| c.is_whitespace()).count();
        (n < line.len()).then(|| buf.visual_col(l, n))
    };
    if let Some(w) = measure(l) {
        return w;
    }
    let prev = (l.saturating_sub(50)..l).rev().find_map(measure).unwrap_or(0);
    let next = (l + 1..(l + 50).min(buf.line_count())).find_map(measure).unwrap_or(0);
    prev.min(next)
}

fn draw_chevron(p: &Painter, c: egui::Pos2, down: bool, color: Color32) {
    let s = 3.5;
    let pts = if down {
        vec![pos2(c.x - s, c.y - s / 2.0), pos2(c.x, c.y + s / 2.0), pos2(c.x + s, c.y - s / 2.0)]
    } else {
        vec![pos2(c.x - s / 2.0, c.y - s), pos2(c.x + s / 2.0, c.y), pos2(c.x - s / 2.0, c.y + s)]
    };
    p.add(Shape::line(pts, Stroke::new(1.3, color)));
}

fn draw_x(p: &Painter, c: egui::Pos2, s: f32, color: Color32) {
    let st = Stroke::new(1.3, color);
    p.line_segment([pos2(c.x - s, c.y - s), pos2(c.x + s, c.y + s)], st);
    p.line_segment([pos2(c.x + s, c.y - s), pos2(c.x - s, c.y + s)], st);
}

/// A small page with a folded corner.
fn draw_file_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let (w, h, f) = (9.0, 12.0, 3.0);
    let (l, t) = (c.x - w / 2.0, c.y - h / 2.0);
    let pts = vec![
        pos2(l, t),
        pos2(l + w - f, t),
        pos2(l + w, t + f),
        pos2(l + w, t + h),
        pos2(l, t + h),
    ];
    p.add(Shape::convex_polygon(pts, color.gamma_multiply(0.25), Stroke::new(1.2, color)));
}

fn draw_files_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let st = Stroke::new(1.5, color);
    let back = Rect::from_min_size(pos2(c.x - 5.0, c.y - 11.0), vec2(13.0, 17.0));
    let front = Rect::from_min_size(pos2(c.x - 9.0, c.y - 6.0), vec2(13.0, 17.0));
    p.line_segment([back.left_top(), back.right_top()], st);
    p.line_segment([back.right_top(), back.right_bottom()], st);
    p.line_segment([back.left_top(), pos2(back.left(), front.top())], st);
    p.line_segment([back.right_bottom(), pos2(front.right(), back.bottom())], st);
    p.rect_filled(front, 0.0, theme::ACTIVITY_BG);
    p.rect_stroke(front, 1.0, st, egui::StrokeKind::Middle);
}

fn draw_search_icon(p: &Painter, c: egui::Pos2, color: Color32) {
    let st = Stroke::new(1.6, color);
    p.circle_stroke(pos2(c.x + 2.0, c.y - 2.0), 7.0, st);
    p.line_segment([pos2(c.x - 3.0, c.y + 3.0), pos2(c.x - 9.0, c.y + 9.0)], Stroke::new(2.2, color));
}

fn draw_welcome(p: &Painter, rect: Rect) {
    let c = rect.center();
    p.text(pos2(c.x, c.y - 110.0), Align2::CENTER_CENTER, "Vex", FontId::proportional(64.0), Color32::from_gray(0x3c));
    let rows = [
        ("Go to File", "Ctrl + P"),
        ("Command Line", ":"),
        ("Open File", ":e <path>"),
        ("Toggle Sidebar", "Ctrl + B"),
        ("Save", ":w  /  Ctrl + S"),
        ("Search in File", "/"),
        ("Quit", ":q"),
    ];
    for (i, (label, keys)) in rows.iter().enumerate() {
        let y = c.y - 30.0 + i as f32 * 26.0;
        p.text(pos2(c.x - 10.0, y), Align2::RIGHT_CENTER, *label, FontId::proportional(13.0), theme::TEXT_MUTED);
        p.text(pos2(c.x + 10.0, y), Align2::LEFT_CENTER, *keys, FontId::monospace(12.5), theme::TEXT_UI);
    }
}
