//! The Vex window: VS Code-style layout around the modal editor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Frame, Galley, Key as EKey, Margin, Modifiers,
    Painter, Rect, Sense, Shape, Stroke, Ui,
};

use crate::buffer::{self, tab_size, Buffer, Pos};
use crate::explorer::{self, Explorer};
use crate::settings::{self, Settings};
use crate::syntax;
use crate::theme;
use crate::view::View;
use crate::workspace::Workspace;
use crate::vim::{Action, Key, Mode, Vim};

const UI_SIZE: f32 = 13.0;

struct QuickOpen {
    query: String,
    files: Vec<PathBuf>,
    selected: usize,
}

pub struct VexApp {
    ws: Workspace,
    /// Receives keys when no file is open, so `:` commands still work.
    scratch: Buffer,
    vim: Vim,
    explorer: Explorer,
    show_sidebar: bool,
    settings: Settings,
    ctx: egui::Context,
    quick_open: Option<QuickOpen>,
    drag_anchor: Option<Pos>,
    follow_cursor: bool,
    title: String,
    allow_close: bool,
    snapshot: Option<crate::snapshot::Snapshot>,
}

impl VexApp {
    pub fn new(cc: &eframe::CreationContext<'_>, root: PathBuf, file: Option<PathBuf>) -> Self {
        theme::install(&cc.egui_ctx);
        let mut app = Self {
            ws: Workspace::default(),
            scratch: Buffer::default(),
            vim: Vim::default(),
            explorer: Explorer::new(root),
            show_sidebar: true,
            settings: Settings::default(),
            ctx: cc.egui_ctx.clone(),
            quick_open: None,
            drag_anchor: None,
            follow_cursor: true,
            title: String::new(),
            allow_close: false,
            snapshot: crate::snapshot::Snapshot::from_env(),
        };
        app.reload_settings(&cc.egui_ctx);
        if let Some(f) = file {
            app.open(&f);
        }
        app
    }

    /// Re-read the settings file and apply it.
    fn reload_settings(&mut self, ctx: &egui::Context) -> bool {
        match settings::load() {
            Ok(s) => {
                self.settings = s;
                self.apply_settings(ctx);
                true
            }
            Err(e) => {
                self.error(e);
                false
            }
        }
    }

    fn apply_settings(&mut self, ctx: &egui::Context) {
        self.settings.apply_globals();
        theme::apply_visuals(ctx);
        self.follow_cursor = true;
    }

    fn set_options(&mut self, ctx: &egui::Context, args: &str) {
        if args.trim().is_empty() {
            let msg = self.settings.summary();
            self.info(msg);
            return;
        }
        for arg in args.split_whitespace() {
            match self.settings.set(arg) {
                Ok(Some(msg)) => self.info(msg),
                Ok(None) => {}
                Err(e) => {
                    self.error(e);
                    break;
                }
            }
        }
        self.apply_settings(ctx);
    }

    fn open_settings(&mut self) {
        match settings::ensure_file() {
            Ok(p) => self.open(&p),
            Err(e) => self.error(format!("Cannot create settings file: {e}")),
        }
    }

    fn info(&mut self, msg: impl Into<String>) {
        self.vim.message = Some(msg.into());
        self.vim.message_is_error = false;
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.vim.message = Some(msg.into());
        self.vim.message_is_error = true;
    }

    /// Something changed which view is focused.
    fn focus_changed(&mut self) {
        if self.vim.mode != Mode::Insert || self.ws.current().is_none() {
            self.vim.mode = Mode::Normal;
        }
        self.follow_cursor = true;
    }

    fn resolve(&self, p: &Path) -> PathBuf {
        let p = match p.strip_prefix("~") {
            Ok(rest) => std::env::var_os("HOME").map_or(p.to_path_buf(), |h| PathBuf::from(h).join(rest)),
            Err(_) => p.to_path_buf(),
        };
        let p = if p.is_absolute() { p } else { self.explorer.root.join(p) };
        p.canonicalize().unwrap_or(p)
    }

    /// Open `path` in the focused pane (switching to it if already open there).
    fn open(&mut self, path: &Path) {
        let path = self.resolve(path);
        if path.is_dir() {
            self.error(format!("\"{}\" is a directory", path.display()));
            return;
        }
        if let Some(b) = self.ws.find(&path) {
            self.ws.show(b);
        } else {
            match Buffer::open(&path) {
                Ok(b) => {
                    self.ws.add_buffer(b);
                    self.explorer.reveal(&path);
                }
                Err(e) => {
                    self.error(format!("Cannot open {}: {e}", path.display()));
                    return;
                }
            }
        }
        self.vim.mode = Mode::Normal;
        self.follow_cursor = true;
    }

    fn close_tab(&mut self, group: usize, tab: usize, force: bool) -> bool {
        match self.ws.close_tab(group, tab, force) {
            Ok(()) => {
                self.focus_changed();
                true
            }
            Err(e) => {
                self.error(e);
                false
            }
        }
    }

    fn close_current(&mut self, force: bool) -> bool {
        let g = self.ws.focus;
        let active = self.ws.groups[g].active;
        self.close_tab(g, active, force)
    }

    /// Write buffer `b`, or the scratch buffer when `b` is `None` and a path is given.
    fn save(&mut self, b: Option<usize>, path: Option<String>) -> bool {
        let path = path.map(|p| self.resolve(Path::new(&p)));
        let b = match b {
            Some(b) => b,
            None if path.is_some() => self.ws.add_buffer(std::mem::take(&mut self.scratch)),
            None => {
                self.error("E32: No file name");
                return false;
            }
        };
        let buf = &mut self.ws.buffers[b];
        if let Some(p) = path {
            buf.path = Some(p);
        }
        match buf.save() {
            Ok(bytes) => {
                let msg = format!("\"{}\" {}L, {}B written", buf.name(), buf.line_count(), bytes);
                let is_settings = buf.path.is_some() && buf.path == settings::path();
                self.info(msg);
                self.explorer.refresh();
                if is_settings && self.reload_settings(&self.ctx.clone()) {
                    self.info("Settings applied");
                }
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

    /// `:q` on an empty pane closes the pane; on the last empty pane, the window.
    fn quit_empty(&mut self, ctx: &egui::Context) {
        if !self.ws.close_empty_group() {
            self.quit_window(ctx);
        }
        self.focus_changed();
    }

    fn handle_action(&mut self, ctx: &egui::Context, action: Action) {
        let cur = self.ws.current();
        match action {
            Action::Write(p) => {
                self.save(cur, p);
            }
            Action::WriteAll => {
                for b in 0..self.ws.buffers.len() {
                    if self.ws.buffers[b].dirty {
                        self.save(Some(b), None);
                    }
                }
            }
            Action::WriteQuit => match cur {
                None => self.quit_empty(ctx),
                Some(b) => {
                    if self.save(Some(b), None) {
                        self.close_current(true);
                    }
                }
            },
            Action::WriteQuitAll => {
                let ok = (0..self.ws.buffers.len()).all(|b| !self.ws.buffers[b].dirty || self.save(Some(b), None));
                if ok {
                    self.quit_window(ctx);
                }
            }
            Action::Quit { force } => match cur {
                None => self.quit_empty(ctx),
                Some(_) => {
                    self.close_current(force);
                }
            },
            Action::QuitAll { force } => {
                if let Some(b) = self.ws.buffers.iter().find(|b| b.dirty).filter(|_| !force) {
                    let msg = format!("E162: No write since last change for buffer \"{}\"", b.name());
                    self.error(msg);
                } else {
                    self.quit_window(ctx);
                }
            }
            Action::Edit(p) => self.open(Path::new(&p)),
            Action::NextTab => {
                self.ws.cycle_tab(1);
                self.focus_changed();
            }
            Action::PrevTab => {
                self.ws.cycle_tab(-1);
                self.focus_changed();
            }
            Action::Set(args) => self.set_options(ctx, &args),
            Action::OpenSettings => self.open_settings(),
            Action::Split(p) => {
                self.ws.split();
                self.focus_changed();
                if let Some(p) = p {
                    self.open(Path::new(&p));
                }
            }
            Action::Window(c) => {
                match c {
                    'h' => self.ws.focus_step(-1, false),
                    'l' => self.ws.focus_step(1, false),
                    'w' => self.ws.focus_step(1, true),
                    'W' => self.ws.focus_step(-1, true),
                    'v' | 's' => return self.handle_action(ctx, Action::Split(None)),
                    'q' | 'c' => return self.handle_action(ctx, Action::Quit { force: false }),
                    'o' => {
                        if let Err(e) = self.ws.only(false) {
                            self.error(e);
                        }
                    }
                    _ => {}
                }
                self.focus_changed();
            }
        }
    }

    // ---- input ---------------------------------------------------------

    fn feed(&mut self, ctx: &egui::Context, key: Key) {
        let buf = match self.ws.current() {
            Some(b) => &mut self.ws.buffers[b],
            None => &mut self.scratch,
        };
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
                if let Some(b) = self.ws.current() {
                    self.save(Some(b), None);
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
            EKey::Tab | EKey::PageDown if !shift => self.handle_action(ctx, Action::NextTab),
            EKey::Tab | EKey::PageUp => self.handle_action(ctx, Action::PrevTab),
            EKey::Backslash => self.handle_action(ctx, Action::Split(None)),
            EKey::Num1 | EKey::Num2 | EKey::Num3 | EKey::Num4 | EKey::Num5 => {
                let n = key.name().parse::<usize>().unwrap_or(1);
                self.ws.focus_group(n - 1);
                self.focus_changed();
            }
            EKey::Equals | EKey::Plus => self.settings.font_size = (self.settings.font_size + 1.0).min(48.0),
            EKey::Minus => self.settings.font_size = (self.settings.font_size - 1.0).max(6.0),
            EKey::Num0 => {
                self.settings.font_size = settings::load().map_or(Settings::default().font_size, |s| s.font_size);
            }
            EKey::Comma => self.open_settings(),
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
                    if modifiers.alt && !modifiers.command && key == EKey::Z {
                        self.settings.word_wrap = !self.settings.word_wrap;
                        self.follow_cursor = true;
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
                    let buf = match self.ws.current() {
                        Some(b) => &mut self.ws.buffers[b],
                        None => &mut self.scratch,
                    };
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
        let dirty: Vec<String> = self.ws.buffers.iter().filter(|b| b.dirty).map(|b| b.name()).collect();
        if !dirty.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.error(format!("Unsaved changes in {} — :wqa to save and quit, :qa! to discard", dirty.join(", ")));
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let root = self.explorer.root_name();
        let title = match self.ws.current_buf() {
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
        let color = if active || resp.hovered() { theme::p().bar_fg } else { theme::p().icon_inactive };
        if active {
            painter.rect_filled(Rect::from_min_size(r.min, vec2(2.0, 48.0)), 0.0, theme::p().bar_fg);
        }
        draw_files_icon(painter, r.center(), color);
        if resp.clicked() {
            self.show_sidebar = !self.show_sidebar;
        }
        resp.on_hover_text("Explorer (Ctrl+B)");

        let r = icon(1);
        let resp = ui.interact(r, ui.scope_id().with("act-search"), Sense::click());
        let color = if self.quick_open.is_some() || resp.hovered() { theme::p().bar_fg } else { theme::p().icon_inactive };
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
            theme::p().text_ui,
        );

        let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
        let painter = ui.painter();
        draw_chevron(painter, pos2(r.left() + 12.0, r.center().y), true, theme::p().text_ui);
        painter.text(
            r.left_center() + vec2(22.0, 0.0),
            Align2::LEFT_CENTER,
            self.explorer.root_name().to_uppercase(),
            FontId::proportional(11.0),
            theme::p().text_ui,
        );
        if resp.on_hover_text("Click to refresh").clicked() {
            self.explorer.refresh();
        }

        let active_path = self.ws.current_buf().and_then(|b| b.path.clone());
        let mut clicked: Option<(PathBuf, bool)> = None;
        let rows = self.explorer.rows();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for row in &rows {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                let painter = ui.painter();
                let is_active = active_path.as_ref() == Some(&row.entry.path);
                if is_active {
                    painter.rect_filled(r, 0.0, theme::p().list_active);
                } else if resp.hovered() {
                    painter.rect_filled(r, 0.0, theme::p().list_hover);
                }
                let x = r.left() + 8.0 + row.depth as f32 * 12.0;
                // Indent guides, like VS Code's tree.
                for d in 0..row.depth {
                    let gx = r.left() + 8.0 + d as f32 * 12.0 + 7.0;
                    painter.line_segment([pos2(gx, r.top()), pos2(gx, r.bottom())], Stroke::new(1.0, theme::p().tree_guide));
                }
                if row.entry.is_dir {
                    draw_chevron(painter, pos2(x + 7.0, r.center().y), row.expanded, theme::p().text_ui);
                } else {
                    draw_file_icon(painter, pos2(x + 8.0, r.center().y), theme::file_color(&row.entry.name));
                }
                painter.text(
                    pos2(x + 20.0, r.center().y),
                    Align2::LEFT_CENTER,
                    &row.entry.name,
                    FontId::proportional(UI_SIZE),
                    theme::p().text_ui,
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

    fn tab_bar(&mut self, ui: &mut Ui, bar: Rect, group: usize) {
        let painter = ui.painter_at(bar);
        let p = theme::p();
        painter.rect_filled(bar, 0.0, p.tabs_bg);
        let focused_group = group == self.ws.focus;
        let mut x = bar.left();
        let mut action: Option<(usize, bool)> = None; // (index, close)
        let g = &self.ws.groups[group];
        for (i, t) in g.tabs.iter().enumerate() {
            let b = &self.ws.buffers[t.buf];
            let active = i == g.active;
            let color = match (active, focused_group) {
                (true, true) => p.text_bright,
                (true, false) => p.text_ui,
                _ => p.text_muted,
            };
            let name = galley(ui.ctx(), b.name(), UI_SIZE, color);
            let w = 12.0 + 16.0 + 4.0 + name.size().x + 8.0 + 20.0 + 6.0;
            let r = Rect::from_min_size(pos2(x, bar.top()), vec2(w, bar.height()));
            let resp = ui.interact(r, ui.scope_id().with(("tab", group, i)), Sense::click());
            painter.rect_filled(r, 0.0, if active { p.editor_bg } else { p.tab_inactive });
            painter.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, p.tabs_bg));
            if active && focused_group {
                painter.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, p.status_bg));
            }
            draw_file_icon(&painter, pos2(r.left() + 20.0, r.center().y), theme::file_color(&b.name()));
            painter.galley(pos2(r.left() + 32.0, r.center().y - name.size().y / 2.0), name, p.text_ui);

            let close = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(20.0, 20.0));
            let close_resp = ui.interact(close, ui.scope_id().with(("tab-close", group, i)), Sense::click());
            if close_resp.hovered() {
                painter.rect_filled(close, CornerRadius::same(4), p.list_hover);
            }
            if b.dirty && !close_resp.hovered() {
                painter.circle_filled(close.center(), 4.0, p.text_ui);
            } else if active || resp.hovered() || close_resp.hovered() {
                draw_x(&painter, close.center(), 4.0, p.text_ui);
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
                self.close_tab(group, i, false);
            }
            Some((i, false)) => {
                self.ws.activate(group, i);
                self.focus_changed();
            }
            None => {}
        }
    }

    fn breadcrumbs(&mut self, ui: &mut Ui, r: Rect, group: usize) {
        let painter = ui.painter_at(r);
        painter.rect_filled(r, 0.0, theme::p().editor_bg);
        let g = &self.ws.groups[group];
        let Some(path) = g.tabs.get(g.active).and_then(|t| self.ws.buffers[t.buf].path.as_ref()) else { return };
        let rel = path.strip_prefix(&self.explorer.root).unwrap_or(path);
        let parts: Vec<String> = rel.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let mut x = r.left() + 16.0;
        for (i, part) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            if last {
                draw_file_icon(&painter, pos2(x + 6.0, r.center().y), theme::file_color(part));
                x += 16.0;
            }
            let rect = painter.text(pos2(x, r.center().y), Align2::LEFT_CENTER, part, FontId::proportional(UI_SIZE), theme::p().text_muted);
            x = rect.right() + 6.0;
            if !last {
                draw_chevron(&painter, pos2(x + 3.0, r.center().y), false, theme::p().text_muted);
                x += 14.0;
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let painter = ui.painter();
        let font = FontId::proportional(12.0);
        let white = theme::p().bar_fg;

        // Left: mode badge, then message.
        let mode = self.vim.mode;
        let label = galley(ui.ctx(), mode.label(), 12.0, white);
        let badge = Rect::from_min_size(rect.min, vec2(label.size().x + 20.0, rect.height()));
        painter.rect_filled(badge, 0.0, theme::mode_color(mode));
        painter.galley(badge.center() - label.size() / 2.0, label, white);
        let mut x = badge.right() + 10.0;
        if let Some(name) = self.ws.current_buf().map(|b| b.name()) {
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
        let buf = self.ws.current_buf();
        let mut items: Vec<String> = Vec::new();
        if let Some(b) = buf {
            items.push(syntax::language_name(&b.extension()).to_string());
            items.push("LF".into());
            items.push("UTF-8".into());
            items.push(if buffer::expand_tab() { format!("Spaces: {}", tab_size()) } else { format!("Tab Size: {}", tab_size()) });
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

    /// The editor area: one column per group, each with tabs, breadcrumbs and a view.
    fn editor_area(&mut self, ui: &mut Ui) {
        let area = ui.available_rect_before_wrap();
        let n = self.ws.groups.len();
        if n == 1 && self.ws.groups[0].tabs.is_empty() {
            let painter = ui.painter_at(area);
            painter.rect_filled(area, 0.0, theme::p().editor_bg);
            draw_welcome(&painter, area);
            return;
        }
        let w = area.width() / n as f32;
        let mut clicked_group = None;
        for g in 0..n {
            let col = Rect::from_min_size(pos2(area.left() + g as f32 * w, area.top()), vec2(w, area.height()));
            let tabs = Rect::from_min_size(col.min, vec2(w, 35.0));
            let crumbs = Rect::from_min_size(pos2(col.left(), tabs.bottom()), vec2(w, 22.0));
            let body = Rect::from_min_max(pos2(col.left(), crumbs.bottom()), col.max);
            if g >= self.ws.groups.len() {
                break; // a tab close removed this group mid-frame
            }
            self.tab_bar(ui, tabs, g);
            if g >= self.ws.groups.len() {
                break;
            }
            self.breadcrumbs(ui, crumbs, g);

            let follow = g == self.ws.focus && std::mem::take(&mut self.follow_cursor);
            let (vim, settings, drag) = (&mut self.vim, &self.settings, &mut self.drag_anchor);
            let resp = self.ws.with_view(g, |buf, focused| {
                let view = View { buf, vim, settings, focused, follow_cursor: follow, drag_anchor: drag };
                view.show(ui, body, ui.scope_id().with(("editor", g)))
            });
            match resp {
                Some(r) if r.clicked() || r.drag_started() => clicked_group = Some(g),
                Some(_) => {}
                None => {
                    // An empty pane beside others.
                    let painter = ui.painter_at(body);
                    painter.rect_filled(body, 0.0, theme::p().editor_bg);
                    painter.text(body.center(), Align2::CENTER_CENTER, "Empty pane — :e <file> or Ctrl+P", FontId::proportional(UI_SIZE), theme::p().text_muted);
                    if ui.interact(body, ui.scope_id().with(("empty", g)), Sense::click()).clicked() {
                        clicked_group = Some(g);
                    }
                }
            }
            if g > 0 {
                ui.painter().line_segment([col.left_top(), col.left_bottom()], Stroke::new(1.0, theme::p().border));
            }
        }
        if let Some(g) = clicked_group.filter(|&g| g != self.ws.focus) {
            self.ws.focus_group(g);
            self.focus_changed();
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
                ui.label(egui::RichText::new("  No matching files").color(theme::p().text_muted));
                ui.add_space(4.0);
            }
            for (i, (_, path)) in matches.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                let painter = ui.painter();
                if i == qo.selected {
                    painter.rect_filled(r, CornerRadius::same(3), theme::p().list_selected);
                } else if resp.hovered() {
                    painter.rect_filled(r, CornerRadius::same(3), theme::p().list_hover);
                }
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                draw_file_icon(painter, pos2(r.left() + 14.0, r.center().y), theme::file_color(&name));
                let nr = painter.text(pos2(r.left() + 26.0, r.center().y), Align2::LEFT_CENTER, &name, FontId::proportional(UI_SIZE), theme::p().text_bright);
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    painter.text(
                        pos2(nr.right() + 8.0, r.center().y),
                        Align2::LEFT_CENTER,
                        dir.to_string_lossy(),
                        FontId::proportional(12.0),
                        theme::p().text_muted,
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
            painter.rect(r, CornerRadius::same(2), theme::p().input_bg, Stroke::new(1.0, theme::p().focus_border), egui::StrokeKind::Inside);
            let tr = painter.text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, &text, font.clone(), theme::p().text_bright);
            let cw = ctx.fonts_mut(|f| f.glyph_width(&font, 'M'));
            painter.rect_filled(Rect::from_min_size(pos2(tr.right() + 1.0, r.center().y - 9.0), vec2(cw, 18.0)), 0.0, theme::p().cursor);
            let hint = if prefix == ":" {
                "w  save  ·  q  close  ·  wq  save+close  ·  e <file>  open  ·  %s/a/b/g  replace  ·  <N>  go to line"
            } else {
                "Enter to search  ·  n / N  next / previous  ·  :noh  clear highlight"
            };
            ui.add_space(4.0);
            ui.label(egui::RichText::new(hint).size(11.5).color(theme::p().text_muted));
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
            .frame(Frame::NONE.fill(theme::p().status_bg))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("activity")
            .exact_size(48.0)
            .resizable(false)
            .frame(Frame::NONE.fill(theme::p().activity_bg))
            .show(ui, |ui| self.activity_bar(ui));
        if self.show_sidebar {
            egui::Panel::left("sidebar")
                .default_size(250.0)
                .size_range(160.0..=600.0)
                .resizable(true)
                .frame(Frame::NONE.fill(theme::p().sidebar_bg))
                .show(ui, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.editor_area(ui));

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
                .fill(theme::p().widget_bg)
                .stroke(Stroke::new(1.0, theme::p().border))
                .corner_radius(6)
                .inner_margin(6)
                .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(140) })
                .show(ui, |ui| {
                    ui.set_width(width);
                    add(ui);
                });
        });
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
    p.rect_filled(front, 0.0, theme::p().activity_bg);
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
        p.text(pos2(c.x - 10.0, y), Align2::RIGHT_CENTER, *label, FontId::proportional(13.0), theme::p().text_muted);
        p.text(pos2(c.x + 10.0, y), Align2::LEFT_CENTER, *keys, FontId::monospace(12.5), theme::p().text_ui);
    }
}
