//! The Vex window: VS Code-style layout around the modal editor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Frame, Galley, Key as EKey, Margin, Modifiers,
    Painter, Rect, Sense, Stroke, Ui,
};

use crate::buffer::{self, tab_size, Buffer, Pos};
use crate::commands::{self, Command, Keybindings};
use crate::diagnostics::{Checker, Grammar, Severity};
use crate::lsp;
use crate::explorer::{self, Explorer};
use crate::git::{self, Git, Repo};
use crate::icons::*;
use crate::keymap::{self, MapMode, Mapping, Origin};
use crate::keys_panel::KeysPanel;
use crate::palette::{self as cmd_palette, Palette};
use crate::search::SearchView;
use crate::session;
use crate::settings::{self, Settings};
use crate::settings_page::SettingsPage;
use crate::syntax;
use crate::terminal::{self, Terminal};
use crate::theme;
use crate::view::View;
use crate::workspace::Workspace;
use crate::vim::{Action, Key, Mode, Vim};

const UI_SIZE: f32 = 13.0;
const TAB_GAP: f32 = 3.0;
const TAB_RADIUS: u8 = 7;

#[derive(Clone, Copy, PartialEq)]
enum PanelTab {
    Problems,
    Terminal,
}

#[derive(Clone, Copy, PartialEq)]
enum SidebarView {
    Explorer,
    Search,
}

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
    sidebar_view: SidebarView,
    search: SearchView,
    settings: Settings,
    ctx: egui::Context,
    quick_open: Option<QuickOpen>,
    palette: Option<Palette>,
    /// Palette entries run lately, newest first.
    palette_recent: Vec<String>,
    drag_anchor: Option<Pos>,
    follow_cursor: bool,
    /// Center the cursor line on the next frame (after jumping to it).
    center_cursor: bool,
    title: String,
    allow_close: bool,
    snapshot: Option<crate::snapshot::Snapshot>,
    /// The editor font currently installed, to reload fonts only when it changes.
    applied_font: Option<String>,
    /// When the user last typed; drives auto-save and cursor blink.
    last_input: f64,
    /// Text events to drop: their key was already used by position (see `vim_key_positions`).
    skip_text: usize,
    terminal: Option<Terminal>,
    /// The bottom panel (Problems and Terminal tabs) is open.
    show_terminal: bool,
    panel_tab: PanelTab,
    terminal_focused: bool,
    /// Wheel movement not yet turned into whole scrollback lines.
    terminal_scroll: f32,
    logo: egui::TextureHandle,
    /// Background syntax checker.
    checker: Checker,
    /// Language servers, for type checking.
    lsp: lsp::Manager,
    git: Git,
    /// Status of the project's git repository, once known (None: not a repo).
    repo: Option<Repo>,
    /// When git status was last asked for.
    git_checked_at: f64,
    /// Whether the window had focus last frame (coming back refreshes git).
    window_focused: bool,
    /// What each language server is busy with, like "Indexing 40%".
    lsp_progress: std::collections::BTreeMap<String, String>,
    /// The language-server settings the servers were started with.
    applied_lsp: Option<(bool, std::collections::BTreeMap<String, String>)>,
    keys: Keybindings,
    keys_panel: Option<KeysPanel>,
    settings_page: Option<SettingsPage>,
    /// Settings changed in the page and not yet written to disk, and when.
    settings_dirty_at: Option<f64>,
}

impl VexApp {
    pub fn new(cc: &eframe::CreationContext<'_>, root: PathBuf, file: Option<PathBuf>) -> Self {
        theme::install(&cc.egui_ctx);
        let icon = eframe::icon_data::from_png_bytes(crate::ICON_PNG).expect("bundled icon is a valid PNG");
        let logo_image = egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
        let logo = cc.egui_ctx.load_texture("vex-logo", logo_image, egui::TextureOptions::LINEAR);
        let mut app = Self {
            ws: Workspace::default(),
            scratch: Buffer::default(),
            vim: Vim::default(),
            explorer: Explorer::new(root),
            show_sidebar: true,
            sidebar_view: SidebarView::Explorer,
            search: {
                let ctx = cc.egui_ctx.clone();
                SearchView::new(move || ctx.request_repaint())
            },
            settings: Settings::default(),
            ctx: cc.egui_ctx.clone(),
            quick_open: None,
            palette: None,
            palette_recent: Vec::new(),
            drag_anchor: None,
            follow_cursor: true,
            center_cursor: false,
            title: String::new(),
            allow_close: false,
            snapshot: crate::snapshot::Snapshot::from_env(),
            applied_font: None,
            last_input: 0.0,
            skip_text: 0,
            terminal: None,
            show_terminal: false,
            panel_tab: PanelTab::Terminal,
            terminal_focused: false,
            terminal_scroll: 0.0,
            logo,
            checker: {
                let ctx = cc.egui_ctx.clone();
                Checker::new(move || ctx.request_repaint())
            },
            lsp: {
                let ctx = cc.egui_ctx.clone();
                lsp::Manager::new(move || ctx.request_repaint())
            },
            git: {
                let ctx = cc.egui_ctx.clone();
                Git::new(move || ctx.request_repaint())
            },
            repo: None,
            git_checked_at: f64::NEG_INFINITY,
            window_focused: true,
            lsp_progress: Default::default(),
            applied_lsp: None,
            keys: Keybindings::default(),
            keys_panel: None,
            settings_page: None,
            settings_dirty_at: None,
        };
        app.reload_settings(&cc.egui_ctx);
        app.load_keybindings();
        app.load_init_script();
        if let Some(f) = file {
            app.open(&f);
        } else if app.settings.restore_session {
            if let Some(s) = session::load(&app.explorer.root) {
                app.ws.restore(&s);
                for g in &app.ws.groups {
                    for t in &g.tabs {
                        if let Some(p) = &app.ws.buffers[t.buf].path {
                            app.explorer.reveal(p);
                        }
                    }
                }
            }
        }
        app
    }

    /// Re-read the settings file and apply it.
    fn reload_settings(&mut self, ctx: &egui::Context) -> bool {
        if let Some(dir) = settings::themes_dir() {
            if let Some(e) = theme::load_theme_files(&dir).into_iter().next() {
                self.error(e);
            }
        }
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
        if let Some(w) = self.settings.apply_globals().into_iter().next() {
            self.error(w);
        }
        theme::apply_visuals(ctx);
        ctx.set_zoom_factor(self.settings.ui_scale);
        let lsp_settings = (self.settings.type_check, self.settings.language_servers.clone());
        if self.applied_lsp.as_ref().is_some_and(|a| *a != lsp_settings) {
            self.lsp.shutdown_all(); // restarted with the new commands as files sync
            self.lsp_progress.clear();
        }
        self.applied_lsp = Some(lsp_settings);
        if self.applied_font.as_deref() != Some(self.settings.font_family.as_str()) {
            let path = (!self.settings.font_family.is_empty()).then(|| PathBuf::from(&self.settings.font_family));
            if let Err(e) = theme::install_fonts(ctx, path.as_deref()) {
                self.error(e);
            }
            self.applied_font = Some(self.settings.font_family.clone());
        }
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

    fn open_settings_page(&mut self) {
        self.quick_open = None;
        self.terminal_focused = false;
        if matches!(self.vim.mode, Mode::Command | Mode::Search) {
            self.vim.mode = Mode::Normal;
        }
        self.settings_page.get_or_insert_with(SettingsPage::default);
    }

    fn settings_page_ui(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        let Some(page) = &mut self.settings_page else { return };
        let out = page.show(ui, rect, &mut self.settings);
        let ctx = ui.ctx().clone();
        if out.reload_themes {
            if let Some(dir) = settings::themes_dir() {
                if let Some(e) = theme::load_theme_files(&dir).into_iter().next() {
                    self.error(e);
                }
            }
            self.apply_settings(&ctx);
        }
        if out.changed {
            self.apply_settings(&ctx);
            self.settings_dirty_at = Some(ctx.input(|i| i.time));
        }
        if out.close {
            self.settings_page = None;
        }
        if out.open_keys {
            self.open_keys_panel();
        }
        if out.open_init_script {
            self.open_init_script();
        }
        if let Some(path) = out.open_file {
            self.save_settings_now();
            self.open(&path);
        }
    }

    /// Write settings changed in the page once they've been still for a moment
    /// (dragging a slider changes them every frame).
    fn save_settings_if_idle(&mut self, ctx: &egui::Context) {
        let Some(t) = self.settings_dirty_at else { return };
        let idle = ctx.input(|i| i.time) - t;
        if idle >= 0.4 || self.settings_page.is_none() {
            self.save_settings_now();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(0.4 - idle));
        }
    }

    fn save_settings_now(&mut self) {
        if self.settings_dirty_at.take().is_some() {
            if let Err(e) = self.settings.save() {
                self.error(format!("Cannot save settings: {e}"));
            }
        }
    }

    fn open_init_script(&mut self) {
        let Some(path) = settings::init_script_path() else { return };
        if !path.exists() {
            let template = "\" Vex startup script: one : command per line, like a .vimrc.\n\
                            \" Saved changes apply right away. Examples:\n\
                            \"\n\
                            \" let mapleader = \"<Space>\"\n\
                            \" inoremap jk <Esc>\n\
                            \" nnoremap <Leader>w :w<CR>\n\
                            \" nnoremap <Leader>t :terminal<CR>\n\
                            \" set relativenumber\n";
            let _ = path.parent().map(std::fs::create_dir_all);
            if let Err(e) = std::fs::write(&path, template) {
                self.error(format!("Cannot create {}: {e}", path.display()));
                return;
            }
        }
        self.open(&path);
    }

    fn open_settings(&mut self) {
        match settings::ensure_file() {
            Ok(p) => self.open(&p),
            Err(e) => self.error(format!("Cannot create settings file: {e}")),
        }
    }

    fn load_keybindings(&mut self) {
        let (keys, errors) = Keybindings::load();
        self.keys = keys;
        self.apply_panel_maps();
        if let Some(e) = errors.into_iter().next() {
            self.error(e);
        }
    }

    /// Install the Vim mappings made in the Keyboard Shortcuts panel.
    fn apply_panel_maps(&mut self) {
        self.vim.keymaps.clear_origin(Origin::Panel);
        for m in &self.keys.maps {
            let Some(mode) = MapMode::from_name(&m.mode) else { continue };
            let lhs = keymap::parse_keys(m.keys.trim(), &self.vim.leader);
            if lhs.is_empty() {
                continue;
            }
            let rhs = keymap::parse_keys(&m.action, &self.vim.leader);
            self.vim.keymaps.insert(Mapping { mode, lhs, rhs, noremap: true, origin: Origin::Panel });
        }
    }

    fn save_keybindings(&mut self) {
        self.apply_panel_maps();
        if let Err(e) = self.keys.save() {
            self.error(format!("Cannot save keybindings: {e}"));
        }
    }

    fn open_keys_panel(&mut self) {
        self.quick_open = None;
        if self.vim.mode == Mode::Command || self.vim.mode == Mode::Search {
            self.vim.mode = Mode::Normal;
        }
        self.keys_panel = Some(KeysPanel::default());
    }

    /// Run init.vex, the vim-style startup script, if it exists.
    fn load_init_script(&mut self) {
        let Some(path) = settings::init_script_path().filter(|p| p.exists()) else { return };
        self.vim.reset_script_state();
        self.run_script(&path, 0);
        self.apply_panel_maps(); // <Leader> may have changed
    }

    /// Run each line of a script as a `:` command. Errors are reported with
    /// their line number; the rest of the script still runs.
    fn run_script(&mut self, path: &Path, depth: usize) {
        if depth > 8 {
            self.error("E169: Command too recursive");
            return;
        }
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.error(format!("Cannot read {}: {e}", path.display()));
                return;
            }
        };
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut first_error = None;
        for (n, line) in text.lines().enumerate() {
            self.vim.map_origin = Origin::Script;
            let result = self.vim.execute(&mut self.scratch, line);
            self.vim.map_origin = Origin::Runtime;
            match result {
                Err(e) => {
                    first_error.get_or_insert(format!("{name}:{}: {e}", n + 1));
                }
                Ok(Some(Action::Source(p))) => {
                    let p = self.resolve(Path::new(&p));
                    self.run_script(&p, depth + 1);
                }
                // A startup script shouldn't write or quit anything.
                Ok(Some(Action::Write(_) | Action::WriteAll | Action::WriteQuit | Action::WriteQuitAll)) => {}
                Ok(Some(Action::Quit { .. } | Action::QuitAll { .. })) => {}
                Ok(Some(action)) => self.handle_action(&self.ctx.clone(), action),
                Ok(None) => {}
            }
        }
        if let Some(e) = first_error {
            self.error(e);
        }
    }

    fn run_command(&mut self, ctx: &egui::Context, c: Command) {
        match c {
            Command::Save => {
                if let Some(b) = self.ws.current() {
                    self.save(Some(b), None);
                }
            }
            Command::CommandLine => {
                self.terminal_focused = false;
                self.quick_open = None;
                self.vim.mode = Mode::Command;
                self.vim.cmdline.clear();
            }
            Command::CommandPalette => {
                self.quick_open = None;
                self.palette = match self.palette {
                    Some(_) => None,
                    None => {
                        self.terminal_focused = false;
                        Some(Palette::new(self.palette_entries(), self.palette_recent.clone()))
                    }
                };
            }
            Command::QuickOpen => {
                self.palette = None;
                self.quick_open = match self.quick_open {
                    Some(_) => None,
                    None => Some(QuickOpen {
                        query: String::new(),
                        files: explorer::all_files(&self.explorer.root),
                        selected: 0,
                    }),
                };
            }
            Command::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            Command::FindInFiles => self.show_search(),
            Command::ToggleTerminal => self.toggle_terminal(),
            Command::ShowProblems => self.toggle_problems(),
            Command::ToggleWrap => {
                self.settings.word_wrap = !self.settings.word_wrap;
                self.follow_cursor = true;
            }
            Command::ToggleMinimap => self.settings.minimap = !self.settings.minimap,
            Command::SplitEditor => self.handle_action(ctx, Action::Split(None)),
            Command::NextTab => self.handle_action(ctx, Action::NextTab),
            Command::PrevTab => self.handle_action(ctx, Action::PrevTab),
            Command::ZoomIn => self.settings.font_size = (self.settings.font_size + 1.0).min(48.0),
            Command::ZoomOut => self.settings.font_size = (self.settings.font_size - 1.0).max(6.0),
            Command::ZoomReset => {
                self.settings.font_size = settings::load().map_or(Settings::default().font_size, |s| s.font_size);
            }
            Command::OpenSettings => self.open_settings_page(),
            Command::Keybindings => self.open_keys_panel(),
            Command::FocusPane(n) => {
                self.ws.focus_group(n as usize - 1);
                self.focus_changed();
                self.terminal_focused = false;
            }
        }
        ctx.request_repaint();
    }

    /// Whether Go to File or the command palette is up (they take the keys).
    fn overlay_open(&self) -> bool {
        self.quick_open.is_some() || self.palette.is_some()
    }

    fn palette_entries(&self) -> Vec<cmd_palette::Entry> {
        use cmd_palette::{Entry, Item};
        let mut out: Vec<Entry> = commands::COMMANDS
            .iter()
            .filter(|i| !matches!(i.command, Command::CommandPalette))
            .map(|i| {
                let label = self.keys.label(i.command);
                Entry { title: i.title.to_string(), detail: if label == "—" { String::new() } else { label }, item: Item::Command(i.command) }
            })
            .collect();
        let extra: [(&str, Item); 13] = [
            ("Color Theme", Item::ThemePicker),
            ("Open Settings File (settings.toml)", Item::OpenSettingsFile),
            ("Open Startup Script (init.vex)", Item::OpenInitScript),
            ("Save All Files", Item::Ex("wa")),
            ("Close Editor", Item::Ex("q")),
            ("Close Other Panes", Item::Ex("only")),
            ("Reveal Active File in Explorer", Item::RevealInExplorer),
            ("Toggle Line Numbers", Item::Ex("set number!")),
            ("Toggle Relative Line Numbers", Item::Ex("set relativenumber!")),
            ("Toggle Indent Guides", Item::Ex("set indentguides!")),
            ("Toggle Breadcrumbs", Item::Ex("set breadcrumbs!")),
            ("Toggle Current Line Highlight", Item::Ex("set cursorline!")),
            ("Clear Search Highlight", Item::Ex("noh")),
        ];
        out.extend(extra.into_iter().map(|(t, item)| Entry { title: t.to_string(), detail: String::new(), item }));
        out
    }

    fn palette_overlay(&mut self, ctx: &egui::Context) {
        use cmd_palette::Item;
        let Some(palette) = &mut self.palette else { return };
        let out = palette.show(ctx);
        if palette.theme_before.is_none() {
            self.palette_recent = palette.recent().to_vec();
        }
        if let Some(name) = out.preview_theme {
            self.settings.theme = name;
            self.apply_settings(ctx);
        }
        if out.close {
            self.palette = None;
        }
        match out.run {
            Some(Item::Command(c)) => self.run_command(ctx, c),
            Some(Item::Ex(line)) => {
                self.run_ex(ctx, line);
                // Toggles from the palette last, like changes in the Settings page.
                if line.starts_with("set ") {
                    self.settings_dirty_at = Some(ctx.input(|i| i.time));
                }
            }
            Some(Item::ThemePicker) => self.palette = Some(Palette::themes(&self.settings.theme)),
            Some(Item::Theme(name)) => {
                self.settings.theme = name;
                self.apply_settings(ctx);
                self.settings_dirty_at = Some(ctx.input(|i| i.time));
            }
            Some(Item::OpenSettingsFile) => self.open_settings(),
            Some(Item::OpenInitScript) => self.open_init_script(),
            Some(Item::RevealInExplorer) => {
                if let Some(path) = self.ws.current_buf().and_then(|b| b.path.clone()) {
                    self.explorer.reveal(&path);
                    self.show_sidebar = true;
                    self.sidebar_view = SidebarView::Explorer;
                }
            }
            None => {}
        }
    }

    /// Run a `:` command line as if it were typed.
    fn run_ex(&mut self, ctx: &egui::Context, line: &str) {
        let buf = match self.ws.current() {
            Some(b) => &mut self.ws.buffers[b],
            None => &mut self.scratch,
        };
        match self.vim.execute(buf, line) {
            Ok(Some(action)) => self.handle_action(ctx, action),
            Ok(None) => {}
            Err(e) => self.error(e),
        }
        self.follow_cursor = true;
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
            Ok(rest) => std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map_or(p.to_path_buf(), |h| PathBuf::from(h).join(rest)),
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
        self.terminal_focused = false;
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
            buf.checked_version = None; // the language may have changed
        }
        if self.settings.trim_trailing_whitespace && buf.trim_trailing_whitespace() {
            buf.clamp_cursor(self.vim.mode == Mode::Insert);
        }
        match buf.save() {
            Ok(bytes) => {
                let msg = format!("\"{}\" {}L, {}B written", buf.name(), buf.line_count(), bytes);
                let is_settings = buf.path.is_some() && buf.path == settings::path();
                let is_script = buf.path.is_some() && buf.path == settings::init_script_path();
                let id = buf.id;
                self.info(msg);
                self.lsp.did_save(id);
                self.explorer.refresh();
                self.git_checked_at = f64::NEG_INFINITY; // the file's status may have changed
                if is_settings && self.reload_settings(&self.ctx.clone()) {
                    self.info("Settings applied");
                }
                if is_script {
                    self.info("init.vex reloaded");
                    self.load_init_script();
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
            Action::OpenSettings => self.open_settings_page(),
            Action::Split(p) => {
                self.ws.split();
                self.focus_changed();
                if let Some(p) = p {
                    self.open(Path::new(&p));
                }
            }
            Action::Source(p) => {
                let p = self.resolve(Path::new(&p));
                self.run_script(&p, 0);
                self.apply_panel_maps();
            }
            Action::App(name) => {
                let c = match name {
                    "terminal" => Command::ToggleTerminal,
                    "files" => Command::QuickOpen,
                    "sidebar" => Command::ToggleSidebar,
                    "settingsfile" => return self.open_settings(),
                    "next-diagnostic" => return self.jump_diagnostic(true),
                    "prev-diagnostic" => return self.jump_diagnostic(false),
                    _ => Command::Keybindings,
                };
                self.run_command(ctx, c);
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
        self.last_input = ctx.input(|i| i.time);
        let buf = match self.ws.current() {
            Some(b) => &mut self.ws.buffers[b],
            None => &mut self.scratch,
        };
        let now = ctx.input(|i| i.time);
        let actions = self.vim.feed(buf, key, now);
        self.after_keys(ctx, actions);
    }

    fn after_keys(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        self.follow_cursor = true;
        if let Some(text) = self.vim.clipboard_out.take() {
            ctx.copy_text(text);
        }
        for a in actions {
            self.handle_action(ctx, a);
        }
    }

    /// Whether a blinking cursor is in its visible half. It stays solid while
    /// typing and starts blinking after a short pause.
    fn cursor_blink_on(&self, ctx: &egui::Context) -> bool {
        if !self.settings.cursor_blink {
            return true;
        }
        let idle = ctx.input(|i| i.time) - self.last_input;
        if idle < 0.5 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(0.5 - idle));
            return true;
        }
        let phase = (idle - 0.5) % 1.0;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(0.5 - phase % 0.5 + 0.01));
        phase >= 0.5
    }

    /// Collect finished syntax checks and language-server results, and send
    /// changed files to both once typing pauses.
    /// Keep git status and the gutter's changed lines up to date.
    fn update_git(&mut self, ctx: &egui::Context) {
        const STATUS_EVERY: f64 = 4.0;
        let (now, focused) = ctx.input(|i| (i.time, i.focused));
        // Coming back to the window: maybe something was committed meanwhile.
        if focused && !self.window_focused {
            self.git_checked_at = f64::NEG_INFINITY;
        }
        self.window_focused = focused;
        if now - self.git_checked_at >= STATUS_EVERY {
            self.git_checked_at = now;
            self.git.refresh_status(&self.explorer.root);
        }
        for done in self.git.results().collect::<Vec<_>>() {
            match done {
                git::Done::Status(repo) => {
                    let new_head = repo.as_ref().map(|r| &r.head) != self.repo.as_ref().map(|r| &r.head);
                    if new_head {
                        for buf in &mut self.ws.buffers {
                            buf.git_version = None; // diff again against the new HEAD
                        }
                    }
                    if repo.is_none() {
                        for buf in &mut self.ws.buffers {
                            buf.git_changes.clear();
                        }
                    }
                    self.repo = repo;
                }
                git::Done::Diff { id, version, changes } => {
                    if let Some(buf) = self.ws.buffers.iter_mut().find(|b| b.id == id && b.version == version) {
                        buf.git_changes = changes;
                    }
                }
            }
        }
        if self.repo.is_none() {
            return;
        }
        let paused = now - self.last_input >= 0.15;
        for buf in &mut self.ws.buffers {
            // Files outside the repository come back with no changes (and are
            // only asked about once).
            let Some(path) = &buf.path else { continue };
            if buf.git_version == Some(buf.version) || (buf.git_version.is_some() && !paused) {
                continue;
            }
            buf.git_version = Some(buf.version);
            let lines = buf.lines.iter().map(|l| l.iter().collect()).collect();
            self.git.diff(buf.id, buf.version, path, lines);
        }
    }

    fn update_diagnostics(&mut self, ctx: &egui::Context) {
        const PAUSE: f64 = 0.15;
        let idle = ctx.input(|i| i.time) - self.last_input;
        let paused = idle >= PAUSE;
        let mut waiting = false;

        // Syntax (tree-sitter).
        let results: Vec<_> = self.checker.results().collect();
        for done in results {
            // A result for an older version is stale; a newer check is on its way.
            if let Some(buf) = self.ws.buffers.iter_mut().find(|b| b.id == done.id && b.version == done.version) {
                buf.syntax_diagnostics = done.diagnostics;
                buf.merge_diagnostics();
            }
        }
        for buf in &mut self.ws.buffers {
            let grammar = Grammar::for_extension(&buf.extension()).filter(|_| self.settings.syntax_check);
            let Some(grammar) = grammar else {
                if !buf.syntax_diagnostics.is_empty() {
                    buf.syntax_diagnostics.clear();
                    buf.merge_diagnostics();
                }
                buf.checked_version = None;
                continue;
            };
            if buf.checked_version == Some(buf.version) {
                continue;
            }
            // A newly opened file is checked right away; edits wait for a pause.
            if buf.checked_version.is_some() && !paused {
                waiting = true;
                continue;
            }
            buf.checked_version = Some(buf.version);
            self.checker.submit(buf.id, buf.version, grammar, buf.text());
        }

        // Types (language servers).
        if self.settings.type_check {
            let root = self.explorer.root.clone();
            for buf in &self.ws.buffers {
                let (Some(path), Some(lang)) = (&buf.path, lsp::language_for(&buf.extension())) else { continue };
                if !paused && self.lsp.is_open(buf.id) {
                    waiting |= self.lsp.is_stale(buf.id, buf.version);
                    continue;
                }
                let text = || buf.text();
                let doc = lsp::Doc { id: buf.id, path, version: buf.version, text: &text, saved: !buf.dirty };
                self.lsp.sync(&doc, &lang, &root, &self.settings.language_servers);
            }
            let open: std::collections::HashSet<u64> = self.ws.buffers.iter().map(|b| b.id).collect();
            self.lsp.close_missing(&open);
            for event in self.lsp.poll() {
                match event {
                    lsp::Event::Diagnostics { path, diagnostics } => {
                        let path = path.canonicalize().unwrap_or(path);
                        let Some(buf) = self.ws.buffers.iter_mut().find(|b| b.path.as_deref() == Some(path.as_path())) else {
                            continue;
                        };
                        buf.lsp_diagnostics = diagnostics.iter().map(|d| d.to_diagnostic(&buf.lines)).collect();
                        buf.merge_diagnostics();
                    }
                    lsp::Event::Progress { server, text } => match text {
                        Some(t) => {
                            self.lsp_progress.insert(server, t);
                        }
                        None => {
                            self.lsp_progress.remove(&server);
                        }
                    },
                    lsp::Event::Exited { server } => {
                        self.lsp_progress.remove(&server);
                        self.error(format!("The {server} language server stopped"));
                    }
                }
            }
        } else if !self.lsp.running().is_empty() || self.ws.buffers.iter().any(|b| !b.lsp_diagnostics.is_empty()) {
            self.lsp.shutdown_all();
            self.lsp_progress.clear();
            for buf in &mut self.ws.buffers {
                buf.lsp_diagnostics.clear();
                buf.merge_diagnostics();
            }
        }
        if waiting {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((PAUSE - idle).max(0.01)));
        }
    }

    /// Move the cursor to the next (or previous) syntax error, wrapping around.
    fn jump_diagnostic(&mut self, forward: bool) {
        let Some(b) = self.ws.current() else { return };
        let buf = &mut self.ws.buffers[b];
        let cur = buf.cursor;
        let target = if forward {
            buf.diagnostics.iter().find(|d| d.start > cur).or(buf.diagnostics.first())
        } else {
            buf.diagnostics.iter().rev().find(|d| d.start < cur).or(buf.diagnostics.last())
        };
        let Some(start) = target.map(|d| d.start) else {
            self.info("No syntax errors");
            return;
        };
        buf.cursor = start;
        buf.clamp_cursor(self.vim.mode == Mode::Insert);
        buf.want_col = buf.visual_col(buf.cursor.line, buf.cursor.col);
        self.follow_cursor = true;
    }

    /// Save files that have a name once typing pauses, if auto-save is on.
    fn auto_save(&mut self, ctx: &egui::Context) {
        if !self.settings.auto_save {
            return;
        }
        let pending: Vec<usize> = (0..self.ws.buffers.len())
            .filter(|&b| self.ws.buffers[b].dirty && self.ws.buffers[b].path.is_some())
            .collect();
        if pending.is_empty() {
            return;
        }
        let delay = self.settings.auto_save_delay_ms as f64 / 1000.0;
        let idle = ctx.input(|i| i.time) - self.last_input;
        if idle < delay {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - idle));
            return;
        }
        let message = self.vim.message.clone();
        for b in pending {
            self.save(Some(b), None);
        }
        // Auto-save is quiet: keep whatever the status bar said before.
        self.vim.message = message;
    }

    /// Keys held for a possible mapping run as typed once the timeout passes.
    fn check_mapping_timeout(&mut self, ctx: &egui::Context) {
        let Some(deadline) = self.vim.typeahead_deadline() else { return };
        let now = ctx.input(|i| i.time);
        if now >= deadline {
            let buf = match self.ws.current() {
                Some(b) => &mut self.ws.buffers[b],
                None => &mut self.scratch,
            };
            let actions = self.vim.flush_typeahead(buf);
            self.after_keys(ctx, actions);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(deadline - now));
        }
    }

    /// Keys go to the terminal when its panel is open and focused.
    fn terminal_has_keys(&self) -> bool {
        self.show_terminal && self.panel_tab == PanelTab::Terminal && self.terminal_focused && self.terminal.is_some() && !self.overlay_open()
    }

    fn toggle_problems(&mut self) {
        if self.show_terminal && self.panel_tab == PanelTab::Problems {
            self.show_terminal = false;
        } else {
            self.show_terminal = true;
            self.panel_tab = PanelTab::Problems;
        }
        self.terminal_focused = false;
    }

    fn toggle_terminal(&mut self) {
        if !self.show_terminal || self.panel_tab != PanelTab::Terminal {
            self.show_terminal = true;
            self.panel_tab = PanelTab::Terminal;
            self.terminal_focused = true;
        } else if self.terminal_focused {
            self.show_terminal = false;
            self.terminal_focused = false;
        } else {
            self.terminal_focused = true;
        }
    }

    /// Route one input event to the terminal. App shortcuts that make sense
    /// there (panel toggles, quick open, zoom, pane focus) still work.
    fn terminal_input(&mut self, ctx: &egui::Context, ev: egui::Event) {
        let Some(t) = &mut self.terminal else { return };
        match ev {
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let app_command = self.keys.lookup(key, modifiers).filter(|c| commands::info_of(*c).in_terminal);
                if let Some(c) = app_command {
                    self.run_command(ctx, c);
                } else if let Some(bytes) = terminal::encode_key(key, modifiers, t.application_cursor()) {
                    t.write(&bytes);
                }
            }
            egui::Event::Text(text) => t.write(text.as_bytes()),
            egui::Event::Paste(text) => t.paste(&text),
            egui::Event::Copy => t.write(&[0x03]), // Ctrl+C interrupts, as in any terminal
            egui::Event::Cut => t.write(&[0x18]),
            _ => {}
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        if self.keys_panel.is_some() || self.settings_page.is_some() {
            return; // the panel and page read their own input
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            if self.keys_panel.is_some() {
                break; // a shortcut just opened it
            }
            if self.terminal_has_keys() {
                self.terminal_input(ctx, ev);
                continue;
            }
            // Typing in the Search box: only app shortcuts, and Esc to go back.
            if self.search.has_focus {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                    if key == EKey::Escape {
                        self.search.has_focus = false;
                        ctx.memory_mut(|m| m.stop_text_input());
                    } else if let Some(c) = self.keys.lookup(key, modifiers) {
                        self.run_command(ctx, c);
                    }
                }
                continue;
            }
            match ev {
                egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } => {
                    if let Some(c) = self.keys.lookup(key, modifiers) {
                        self.run_command(ctx, c);
                        continue;
                    }
                    if self.overlay_open() {
                        continue;
                    }
                    // With key positions on, vim commands follow where a key sits on a
                    // QWERTY keyboard, whatever the layout. The layout's character
                    // for this press comes next as a Text event, which is dropped.
                    let positional = self.settings.vim_key_positions;
                    let plain = !modifiers.ctrl && !modifiers.command && !modifiers.alt;
                    if positional && plain && self.vim.expects_command_key() {
                        if let Some(c) = physical_key.and_then(|pk| keymap::qwerty_char(pk, modifiers.shift)) {
                            self.skip_text += 1;
                            self.feed(ctx, Key::Char(c));
                            continue;
                        }
                    }
                    let key = if positional && modifiers.ctrl { physical_key.unwrap_or(key) } else { key };
                    // Vim's Ctrl keys use the real Ctrl key (on macOS, Cmd is for app shortcuts).
                    let k = if modifiers.ctrl {
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
                egui::Event::Text(t) if !self.overlay_open() => {
                    if self.skip_text > 0 {
                        self.skip_text -= 1;
                        continue;
                    }
                    for c in t.chars() {
                        self.feed(ctx, Key::Char(c));
                    }
                }
                // Ctrl+C: copy the selection in visual mode, otherwise act like Esc (as in vim).
                egui::Event::Copy if !self.overlay_open() => {
                    let key = if matches!(self.vim.mode, Mode::Visual | Mode::VisualLine) { Key::Char('y') } else { Key::Esc };
                    self.feed(ctx, key);
                }
                egui::Event::Paste(t) if !self.overlay_open() => {
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
        // A key's text arrives in the same frame; never let a stale skip eat later typing.
        self.skip_text = 0;
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
        let p = theme::p();

        // The Vex logo in the top-left corner.
        let logo_cell = Rect::from_min_size(rect.min, vec2(48.0, 50.0));
        let logo = Rect::from_center_size(logo_cell.center(), vec2(26.0, 26.0));
        ui.painter().image(self.logo.id(), logo, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        ui.interact(logo_cell, ui.scope_id().with("act-logo"), Sense::hover())
            .on_hover_text(format!("Vex {}", env!("CARGO_PKG_VERSION")));

        let item = |i: usize| Rect::from_min_size(pos2(rect.left(), logo_cell.bottom() + i as f32 * 46.0), vec2(48.0, 46.0));
        let explorer_on = self.show_sidebar && self.sidebar_view == SidebarView::Explorer;
        let search_on = self.show_sidebar && self.sidebar_view == SidebarView::Search;
        let explorer = activity_item(ui, item(0), "act-explorer", explorer_on, &p, draw_files_icon);
        let search = activity_item(ui, item(1), "act-search", search_on, &p, draw_search_icon);
        if explorer.clicked() {
            self.show_sidebar = !explorer_on;
            self.sidebar_view = SidebarView::Explorer;
        }
        if search.clicked() {
            if search_on {
                self.show_sidebar = false;
            } else {
                self.show_search();
            }
        }
        explorer.on_hover_text(format!("Explorer ({})", self.keys.label(Command::ToggleSidebar)));
        search.on_hover_text(format!("Search ({})", self.keys.label(Command::FindInFiles)));

        // Bottom: the gear, like VS Code's Manage button.
        let r = Rect::from_min_size(pos2(rect.left(), rect.bottom() - 50.0), vec2(48.0, 46.0));
        let gear = activity_item(ui, r, "act-settings", self.settings_page.is_some(), &p, draw_gear_icon);
        if gear.clicked() {
            if self.settings_page.is_some() {
                self.settings_page = None;
            } else {
                self.open_settings_page();
            }
        }
        gear.on_hover_text(format!("Settings ({})", self.keys.label(Command::OpenSettings)));
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        match self.sidebar_view {
            SidebarView::Explorer => self.explorer_view(ui),
            SidebarView::Search => {
                if let Some(jump) = self.search.show(ui, &self.explorer.root) {
                    self.open_at(&self.explorer.root.join(&jump.path), jump.line, jump.col);
                    // Highlight the matches in the editor too; n and N go through them.
                    self.vim.search = Some(self.search.query.vim_pattern());
                    self.vim.highlight_search = true;
                    self.search.has_focus = false;
                    ui.ctx().memory_mut(|m| m.stop_text_input());
                }
            }
        }
    }

    /// Open the Search view and put the cursor in its box, starting from the
    /// selected text if there is some.
    fn show_search(&mut self) {
        self.show_sidebar = true;
        self.sidebar_view = SidebarView::Search;
        self.search.focus_requested = true;
        self.terminal_focused = false;
        if let Some(b) = self.ws.current() {
            let buf = &self.ws.buffers[b];
            if let Some((a, e)) = self.vim.selection(buf) {
                if a.line == e.line && matches!(self.vim.mode, Mode::Visual) {
                    self.search.query.text = buf.slice(a, e);
                    self.search.query.text.push(buf.lines[e.line].get(e.col).copied().unwrap_or_default());
                    self.search.query.text.retain(|c| c != '\0');
                    self.vim.mode = Mode::Normal;
                    self.search.start(&self.explorer.root);
                }
            }
        }
    }

    /// Open `path` with the cursor at `line`, `col`.
    fn open_at(&mut self, path: &Path, line: usize, col: usize) {
        let target = self.resolve(path);
        self.open(&target);
        let Some(b) = self.ws.current() else { return };
        let buf = &mut self.ws.buffers[b];
        if buf.path.as_ref() != Some(&target) {
            return; // it failed to open
        }
        let line = line.min(buf.lines.len().saturating_sub(1));
        buf.cursor = Pos::new(line, col.min(buf.lines[line].len()));
        buf.want_col = buf.cursor.col;
        self.follow_cursor = true;
        self.center_cursor = true;
    }

    fn explorer_view(&mut self, ui: &mut Ui) {
        let p = theme::p();
        let w = ui.available_width();
        let r = sidebar_header(ui, "EXPLORER");
        let refresh = Rect::from_center_size(pos2(r.right() - 22.0, r.center().y), vec2(22.0, 22.0));
        let collapse = Rect::from_center_size(pos2(r.right() - 48.0, r.center().y), vec2(22.0, 22.0));
        if header_button(ui, refresh, "explorer-refresh", "Refresh", draw_refresh_icon) {
            self.explorer.refresh();
            self.git.forget_head();
            self.git_checked_at = f64::NEG_INFINITY;
            for buf in &mut self.ws.buffers {
                buf.git_version = None;
            }
        }
        if header_button(ui, collapse, "explorer-collapse", "Collapse Folders", draw_collapse_icon) {
            self.explorer.collapse_all();
        }

        let (r, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
        let painter = ui.painter();
        draw_chevron(painter, pos2(r.left() + 12.0, r.center().y), true, p.text_ui);
        painter.text(
            r.left_center() + vec2(22.0, 0.0),
            Align2::LEFT_CENTER,
            self.explorer.root_name().to_uppercase(),
            FontId::proportional(11.0),
            p.text_bright,
        );

        let active_path = self.ws.current_buf().and_then(|b| b.path.clone());
        let mut clicked: Option<(PathBuf, bool)> = None;
        let rows = self.explorer.rows();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for row in &rows {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                let painter = ui.painter();
                let is_active = active_path.as_ref() == Some(&row.entry.path);
                // Rounded, inset highlights.
                let hl = r.shrink2(vec2(6.0, 0.5));
                if is_active {
                    painter.rect_filled(hl, CornerRadius::same(5), p.list_active);
                } else if resp.hovered() {
                    painter.rect_filled(hl, CornerRadius::same(5), p.list_hover);
                }
                let x = r.left() + 10.0 + row.depth as f32 * 12.0;
                // Indent guides, like VS Code's tree.
                for d in 0..row.depth {
                    let gx = r.left() + 10.0 + d as f32 * 12.0 + 7.0;
                    painter.line_segment([pos2(gx, r.top()), pos2(gx, r.bottom())], Stroke::new(1.0, p.tree_guide));
                }
                // Folders: chevron and folder icon. Files line up with folder icons.
                if row.entry.is_dir {
                    draw_chevron(painter, pos2(x + 7.0, r.center().y), row.expanded, p.text_muted);
                    draw_folder_icon(painter, pos2(x + 23.0, r.center().y), row.expanded);
                } else {
                    draw_file_icon(painter, pos2(x + 23.0, r.center().y), theme::file_color(&row.entry.name));
                }
                // Git status: the name in the status color, and a letter (or a
                // dot, for a folder with changes inside) at the right.
                let status = self.repo.as_ref().and_then(|repo| {
                    if row.entry.is_dir { repo.folders.get(&row.entry.path) } else { repo.files.get(&row.entry.path) }.copied()
                });
                let color = match status {
                    Some(st) => theme::git_color(&p, st),
                    None if is_active => p.text_bright,
                    None => p.text_ui,
                };
                let mut name_right = r.right() - 10.0;
                if let Some(st) = status {
                    let c = theme::git_color(&p, st);
                    if row.entry.is_dir {
                        painter.circle_filled(pos2(r.right() - 18.0, r.center().y), 3.0, c.gamma_multiply(0.8));
                    } else {
                        painter.text(pos2(r.right() - 18.0, r.center().y), Align2::CENTER_CENTER, st.letter(), FontId::proportional(12.0), c);
                    }
                    name_right = r.right() - 30.0;
                }
                let name = painter.layout_no_wrap(row.entry.name.clone(), FontId::proportional(UI_SIZE), color);
                let clip = Rect::from_min_max(pos2(x + 35.0, r.top()), pos2(name_right, r.bottom()));
                painter.with_clip_rect(clip).galley(pos2(x + 35.0, r.center().y - name.size().y / 2.0), name, color);
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
        let mut active_span = None;
        let g = &self.ws.groups[group];
        for (i, t) in g.tabs.iter().enumerate() {
            let b = &self.ws.buffers[t.buf];
            let active = i == g.active;
            let color = match (active, focused_group) {
                (true, true) => p.text_bright,
                (true, false) => p.text_ui,
                _ => p.text_muted,
            };
            let color = if b.diagnostics.iter().any(|d| d.severity == Severity::Error) {
                p.error
            } else if b.diagnostics.is_empty() {
                color
            } else {
                p.warning
            };
            let name = galley(ui.ctx(), b.name(), UI_SIZE, color);
            let w = 12.0 + 16.0 + 4.0 + name.size().x + 8.0 + 20.0 + 6.0;
            // Tabs float a little below the bar's top edge, rounded at the top;
            // the active one joins the editor below it.
            let r = Rect::from_min_size(pos2(x + TAB_GAP, bar.top() + 5.0), vec2(w, bar.height() - 5.0));
            let resp = ui.interact(r, ui.scope_id().with(("tab", group, i)), Sense::click());
            let top_round = CornerRadius { nw: TAB_RADIUS, ne: TAB_RADIUS, sw: 0, se: 0 };
            if active {
                if focused_group {
                    // The accent hugs the rounded top: the tab drawn in accent,
                    // then again 2px lower in the editor color.
                    painter.rect_filled(r, top_round, p.accent);
                    painter.rect_filled(Rect::from_min_max(r.min + vec2(0.0, 2.0), r.max), top_round, p.editor_bg);
                } else {
                    painter.rect_filled(r, top_round, p.editor_bg);
                }
                active_span = Some((r.left(), r.right()));
            } else if resp.hovered() {
                painter.rect_filled(r.shrink2(vec2(0.0, 0.5)), top_round, p.list_hover);
            } else if p.tab_inactive != p.tabs_bg {
                painter.rect_filled(r, top_round, p.tab_inactive);
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
            x += w + TAB_GAP;
        }
        // The bottom border runs under every tab but the active one, which joins the editor.
        let y = bar.bottom() - 0.5;
        let stroke = Stroke::new(1.0, p.separator);
        match active_span {
            Some((a, b)) => {
                painter.line_segment([pos2(bar.left(), y), pos2(a, y)], stroke);
                painter.line_segment([pos2(b, y), pos2(bar.right(), y)], stroke);
            }
            None => {
                painter.line_segment([pos2(bar.left(), y), pos2(bar.right(), y)], stroke);
            }
        }

        // Editor actions at the right end, like VS Code's title-bar buttons.
        let actions = Rect::from_min_max(pos2(bar.right() - 64.0, bar.top()), pos2(bar.right(), bar.bottom() - 1.0));
        painter.rect_filled(actions, 0.0, p.tabs_bg);
        let buttons = [
            ("split", "Split Editor", self.keys.label(Command::SplitEditor)),
            ("minimap", "Toggle Minimap", self.keys.label(Command::ToggleMinimap)),
        ];
        for (i, (kind, title, keys)) in buttons.into_iter().enumerate() {
            let r = Rect::from_center_size(pos2(actions.left() + 18.0 + i as f32 * 26.0, actions.center().y), vec2(22.0, 22.0));
            let resp = ui.interact(r, ui.scope_id().with(("tab-action", group, kind)), Sense::click());
            let on = kind == "minimap" && self.settings.minimap;
            if resp.hovered() || on {
                painter.rect_filled(r, CornerRadius::same(4), p.list_hover);
            }
            let color = if resp.hovered() || on { p.text_bright } else { p.text_muted };
            match kind {
                "split" => draw_split_icon(&painter, r.center(), color),
                _ => draw_minimap_icon(&painter, r.center(), color),
            }
            let hover = if keys == "—" { title.to_string() } else { format!("{title} ({keys})") };
            if resp.on_hover_text(hover).clicked() {
                let ctx = ui.ctx().clone();
                if kind == "split" {
                    self.ws.focus_group(group);
                    self.run_command(&ctx, Command::SplitEditor);
                } else {
                    self.run_command(&ctx, Command::ToggleMinimap);
                }
                return;
            }
        }
        match action {
            Some((i, true)) => {
                self.close_tab(group, i, false);
            }
            Some((i, false)) => {
                self.ws.activate(group, i);
                self.focus_changed();
                self.terminal_focused = false;
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
        let fg = theme::p().status_fg;
        let white = Color32::WHITE;

        // Left: mode badge, then message.
        let mode = self.vim.mode;
        let in_terminal = self.terminal_has_keys();
        let (label, badge_color) = if in_terminal {
            ("TERMINAL", Color32::from_rgb(0x3a, 0x3d, 0x41))
        } else {
            (mode.label(), theme::mode_color(mode))
        };
        let label = galley(ui.ctx(), label, 11.5, white);
        let badge = Rect::from_min_size(rect.min + vec2(6.0, 3.0), vec2(label.size().x + 16.0, rect.height() - 6.0));
        painter.rect_filled(badge, CornerRadius::same(4), badge_color);
        painter.galley(badge.center() - label.size() / 2.0, label, white);
        let mut x = badge.right() + 10.0;
        // The git branch, with commits to push / pull.
        if let Some(repo) = self.repo.as_ref().filter(|r| !r.branch.is_empty()) {
            let mut text = repo.branch.clone();
            if repo.ahead > 0 {
                text.push_str(&format!(" ↑{}", repo.ahead));
            }
            if repo.behind > 0 {
                text.push_str(&format!(" ↓{}", repo.behind));
            }
            draw_branch_icon(painter, pos2(x + 5.0, rect.center().y), fg);
            let r = painter.text(pos2(x + 14.0, rect.center().y), Align2::LEFT_CENTER, text, font.clone(), fg);
            x = r.right() + 14.0;
        }
        if let Some(name) = self.ws.current_buf().map(|b| b.name()) {
            let r = painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, name, font.clone(), fg);
            x = r.right() + 14.0;
        }
        // Syntax errors: a count to click (next error), and the message for the cursor's line.
        let (errors, warnings, cursor_problem) = match self.ws.current_buf() {
            Some(b) => (
                b.diagnostics.iter().filter(|d| d.severity == Severity::Error).count(),
                b.diagnostics.iter().filter(|d| d.severity != Severity::Error).count(),
                b.diagnostics
                    .iter()
                    .find(|d| d.contains(b.cursor))
                    .or_else(|| b.diagnostics.iter().find(|d| d.start.line == b.cursor.line))
                    .map(|d| (d.message.lines().next().unwrap_or("").to_string(), d.severity)),
            ),
            None => (0, 0, None),
        };
        if errors + warnings > 0 {
            let p = theme::p();
            let err_count = galley(ui.ctx(), errors.to_string(), 12.0, fg);
            let warn_count = galley(ui.ctx(), warnings.to_string(), 12.0, fg);
            let warn_x = x + 16.0 + err_count.size().x + 10.0;
            let width = warn_x - x + 16.0 + warn_count.size().x + 6.0;
            let r = Rect::from_min_size(pos2(x - 5.0, rect.top() + 2.0), vec2(width + 5.0, rect.height() - 4.0));
            let resp = ui.interact(r, ui.scope_id().with("status-errors"), Sense::click());
            if resp.hovered() {
                painter.rect_filled(r, CornerRadius::same(3), p.list_hover);
            }
            // Error count: a circled x. Warning count: a triangle.
            draw_error_icon(painter, pos2(x + 6.0, rect.center().y), p.error);
            painter.galley(pos2(x + 16.0, rect.center().y - err_count.size().y / 2.0), err_count, fg);
            draw_warning_icon(painter, pos2(warn_x + 6.0, rect.center().y), p.warning);
            painter.galley(pos2(warn_x + 16.0, rect.center().y - warn_count.size().y / 2.0), warn_count, fg);
            x = r.right() + 12.0;
            let tip = format!("Problems ({}); ]d / [d go to the next / previous one", self.keys.label(Command::ShowProblems));
            if resp.on_hover_text(tip).clicked() {
                self.toggle_problems();
            }
        }
        if let (None, Some((msg, severity))) = (&self.vim.message, cursor_problem) {
            let p = theme::p();
            painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, msg, font.clone(), theme::severity_color(&p, severity));
        }
        if let Some(msg) = &self.vim.message {
            let error = self.vim.message_is_error;
            let g = galley(ui.ctx(), msg.clone(), 12.0, if error { white } else { fg });
            if self.vim.message_is_error {
                let bg = Rect::from_min_size(pos2(x - 6.0, rect.top() + 2.0), vec2(g.size().x + 12.0, rect.height() - 4.0));
                painter.rect_filled(bg, CornerRadius::same(3), Color32::from_rgb(0xc7, 0x2e, 0x0f));
            }
            painter.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, white);
        }

        // Right: cursor position and file info.
        // Items that do something when clicked get a rounded hover highlight.
        #[derive(Clone, Copy)]
        enum Click {
            None,
            GoToLine,
            EditorSettings,
        }
        let buf = self.ws.current_buf();
        let mut items: Vec<(String, Click, &str)> = Vec::new();
        if let Some(b) = buf {
            items.push((syntax::language_name(&b.extension()).to_string(), Click::None, ""));
            items.push((if b.crlf { "CRLF" } else { "LF" }.into(), Click::None, ""));
            items.push(("UTF-8".into(), Click::None, ""));
            let indent = if buffer::expand_tab() { format!("Spaces: {}", tab_size()) } else { format!("Tab Size: {}", tab_size()) };
            items.push((indent, Click::EditorSettings, "Indentation settings"));
            match self.vim.selection(b) {
                Some((a, e)) if mode == Mode::VisualLine => items.push((format!("{} lines", e.line - a.line + 1), Click::None, "")),
                Some((a, e)) => items.push((format!("({} selected)", b.slice(a, e).chars().count() + 1), Click::None, "")),
                None => {}
            }
            let pos = format!("Ln {}, Col {}", b.cursor.line + 1, b.visual_col(b.cursor.line, b.cursor.col) + 1);
            items.push((pos, Click::GoToLine, "Go to line (type a number)"));
        }
        if let Some((server, text)) = self.lsp_progress.iter().next() {
            items.push((format!("{server}: {text}"), Click::None, ""));
        }
        let pending = self.vim.pending_keys();
        if !pending.is_empty() {
            items.push((pending, Click::None, ""));
        }
        let mut rx = rect.right() - 10.0;
        let mut clicked = Click::None;
        for (i, (item, action, tip)) in items.into_iter().enumerate() {
            let g = galley(ui.ctx(), item, 12.0, fg);
            let r = Rect::from_min_max(pos2(rx - g.size().x - 6.0, rect.top() + 2.0), pos2(rx + 6.0, rect.bottom() - 2.0));
            if !matches!(action, Click::None) {
                let resp = ui.interact(r, ui.scope_id().with(("status-item", i)), Sense::click());
                if resp.hovered() {
                    painter.rect_filled(r, CornerRadius::same(4), theme::p().list_hover);
                }
                if resp.on_hover_text(tip).clicked() {
                    clicked = action;
                }
            }
            painter.galley(pos2(rx - g.size().x, rect.center().y - g.size().y / 2.0), g, fg);
            rx = r.left() - 12.0;
        }
        match clicked {
            Click::GoToLine => {
                self.vim.mode = Mode::Command;
                self.vim.cmdline.clear();
            }
            Click::EditorSettings => {
                self.open_settings_page();
                if let Some(page) = &mut self.settings_page {
                    page.show_section("Editor");
                }
            }
            Click::None => {}
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
            draw_welcome(&painter, area, &self.logo, &self.keys);
            return;
        }
        let w = area.width() / n as f32;
        let mut clicked_group = None;
        for g in 0..n {
            let col = Rect::from_min_size(pos2(area.left() + g as f32 * w, area.top()), vec2(w, area.height()));
            let tabs = Rect::from_min_size(col.min, vec2(w, 35.0));
            let crumbs_h = if self.settings.breadcrumbs { 22.0 } else { 0.0 };
            let crumbs = Rect::from_min_size(pos2(col.left(), tabs.bottom()), vec2(w, crumbs_h));
            let body = Rect::from_min_max(pos2(col.left(), crumbs.bottom()), col.max);
            if g >= self.ws.groups.len() {
                break; // a tab close removed this group mid-frame
            }
            self.tab_bar(ui, tabs, g);
            if g >= self.ws.groups.len() {
                break;
            }
            if self.settings.breadcrumbs {
                self.breadcrumbs(ui, crumbs, g);
            }

            let follow = g == self.ws.focus && std::mem::take(&mut self.follow_cursor);
            let center = follow && std::mem::take(&mut self.center_cursor);
            let cursor_visible = self.cursor_blink_on(ui.ctx());
            let editor_focused = !(self.show_terminal && self.panel_tab == PanelTab::Terminal && self.terminal_focused);
            let (vim, settings, drag) = (&mut self.vim, &self.settings, &mut self.drag_anchor);
            let resp = self.ws.with_view(g, |buf, focused| {
                let focused = focused && editor_focused;
                let view = View { buf, vim, settings, focused, follow_cursor: follow, center_cursor: center, drag_anchor: drag, cursor_visible };
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
                ui.painter().line_segment([col.left_top(), col.left_bottom()], Stroke::new(1.0, theme::p().separator));
            }
        }
        if let Some(g) = clicked_group {
            self.terminal_focused = false;
            if g != self.ws.focus {
                self.ws.focus_group(g);
                self.focus_changed();
            }
        }
    }

    // ---- bottom panel ----------------------------------------------------

    /// Every problem in the open files, by file; clicking one goes there.
    fn problems_list(&mut self, ui: &mut Ui, body: Rect) {
        let p = theme::p();
        // Rows: a file header, then its problems.
        let mut files: Vec<usize> = (0..self.ws.buffers.len()).filter(|&b| !self.ws.buffers[b].diagnostics.is_empty()).collect();
        files.sort_by_key(|&b| self.ws.buffers[b].name());
        let rows: Vec<(usize, Option<usize>)> = files
            .iter()
            .flat_map(|&b| std::iter::once((b, None)).chain((0..self.ws.buffers[b].diagnostics.len()).map(move |d| (b, Some(d)))))
            .collect();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink2(vec2(8.0, 2.0))));
        if rows.is_empty() {
            child.add_space(8.0);
            child.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new("No problems have been detected in the open files.").size(12.5).color(p.text_muted));
            });
            return;
        }
        let mut jump = None;
        const ROW_H: f32 = 22.0;
        egui::ScrollArea::vertical().id_salt("problems").auto_shrink(false).show_rows(&mut child, ROW_H, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for &(b, d) in &rows[range] {
                let buf = &self.ws.buffers[b];
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                let painter = ui.painter();
                if resp.hovered() {
                    painter.rect_filled(r, CornerRadius::same(5), p.list_hover);
                }
                let y = r.center().y;
                match d {
                    None => {
                        let name = buf.name();
                        draw_chevron(painter, pos2(r.left() + 10.0, y), true, p.text_muted);
                        draw_file_icon(painter, pos2(r.left() + 26.0, y), theme::file_color(&name));
                        let nr = painter.text(pos2(r.left() + 38.0, y), Align2::LEFT_CENTER, &name, FontId::proportional(13.0), p.text_bright);
                        let mut x = nr.right() + 8.0;
                        if let Some(dir) = buf.path.as_ref().and_then(|p| p.parent()) {
                            let dir = dir.strip_prefix(&self.explorer.root).unwrap_or(dir);
                            if !dir.as_os_str().is_empty() {
                                let dr = painter.text(pos2(x, y), Align2::LEFT_CENTER, dir.display(), FontId::proportional(12.0), p.text_muted);
                                x = dr.right() + 8.0;
                            }
                        }
                        let g = painter.layout_no_wrap(buf.diagnostics.len().to_string(), FontId::proportional(10.5), p.text_bright);
                        let pill = Rect::from_min_size(pos2(x, y - 8.0), vec2((g.size().x + 10.0).max(16.0), 16.0));
                        painter.rect_filled(pill, CornerRadius::same(8), p.input_bg);
                        painter.galley(pill.center() - g.size() / 2.0, g, p.text_bright);
                    }
                    Some(i) => {
                        let diag = &buf.diagnostics[i];
                        let c = pos2(r.left() + 40.0, y);
                        let color = theme::severity_color(&p, diag.severity);
                        match diag.severity {
                            Severity::Error => draw_error_icon(painter, c, color),
                            Severity::Warning => draw_warning_icon(painter, c, color),
                            Severity::Info => draw_info_icon(painter, c, color),
                        }
                        let msg = diag.message.lines().next().unwrap_or("");
                        let where_ = format!("[Ln {}, Col {}]", diag.start.line + 1, diag.start.col + 1);
                        let wg = painter.layout_no_wrap(where_, FontId::proportional(12.0), p.text_muted);
                        let mg = painter.layout_no_wrap(msg.to_string(), FontId::proportional(13.0), p.text_ui);
                        let x = r.left() + 54.0;
                        let max_msg = (r.right() - x - wg.size().x - 16.0).max(40.0);
                        let clip = Rect::from_min_max(pos2(x, r.top()), pos2(x + max_msg, r.bottom()));
                        let msg_w = mg.size().x.min(max_msg);
                        let cut_off = mg.size().x > max_msg;
                        painter.with_clip_rect(clip).galley(pos2(x, y - mg.size().y / 2.0), mg, p.text_ui);
                        painter.galley(pos2(x + msg_w + 8.0, y - wg.size().y / 2.0), wg, p.text_muted);
                        let resp = if diag.message.contains('\n') || cut_off {
                            resp.on_hover_text(diag.message.clone())
                        } else {
                            resp
                        };
                        if resp.clicked() {
                            jump = Some((buf.path.clone(), diag.start));
                        }
                    }
                }
            }
        });
        if let Some((Some(path), pos)) = jump {
            self.open_at(&path, pos.line, pos.col);
        }
    }


    fn terminal_panel(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        // Claim the whole panel; egui sizes panels by what their content uses.
        ui.expand_to_include_rect(rect);
        let p = theme::p();
        let painter = ui.painter_at(rect);
        painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, p.border));

        // Header: the PROBLEMS and TERMINAL tabs, then the panel's buttons.
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), 35.0));
        let (errors, warnings) = self.ws.buffers.iter().flat_map(|b| &b.diagnostics).fold((0, 0), |(e, w), d| {
            if d.severity == Severity::Error { (e + 1, w) } else { (e, w + 1) }
        });
        let mut x = header.left() + 20.0;
        let mut switch = None;
        for (tab, label) in [(PanelTab::Problems, "PROBLEMS"), (PanelTab::Terminal, "TERMINAL")] {
            let active = self.panel_tab == tab;
            let color = if active { p.text_bright } else { p.text_muted };
            let title = painter.text(pos2(x, header.center().y), Align2::LEFT_CENTER, label, FontId::proportional(11.0), color);
            let mut right = title.right();
            // The problem count, as a small pill.
            if tab == PanelTab::Problems && errors + warnings > 0 {
                let g = painter.layout_no_wrap((errors + warnings).to_string(), FontId::proportional(10.5), p.text_bright);
                let pill = Rect::from_min_size(pos2(right + 6.0, header.center().y - 8.0), vec2((g.size().x + 10.0).max(16.0), 16.0));
                painter.rect_filled(pill, CornerRadius::same(8), p.input_bg);
                painter.galley(pill.center() - g.size() / 2.0, g, p.text_bright);
                right = pill.right();
            }
            let hit = Rect::from_min_max(pos2(title.left() - 6.0, header.top() + 4.0), pos2(right + 6.0, header.bottom() - 4.0));
            let resp = ui.interact(hit, ui.scope_id().with(("panel-tab", label)), Sense::click());
            if resp.hovered() && !active {
                painter.text(title.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(11.0), p.text_ui);
            }
            if active {
                let focused = tab == PanelTab::Problems || self.terminal_focused;
                let underline = Rect::from_min_max(pos2(title.left(), title.bottom() + 5.0), pos2(right, title.bottom() + 7.0));
                painter.rect_filled(underline, CornerRadius::same(1), if focused { p.accent } else { p.text_muted });
            }
            if resp.clicked() {
                switch = Some(tab);
            }
            x = right + 24.0;
        }
        if let Some(tab) = switch {
            self.panel_tab = tab;
            self.terminal_focused = tab == PanelTab::Terminal;
        }
        let on_terminal = self.panel_tab == PanelTab::Terminal;
        let close = Rect::from_center_size(pos2(header.right() - 22.0, header.center().y), vec2(22.0, 22.0));
        let kill = Rect::from_center_size(pos2(header.right() - 50.0, header.center().y), vec2(22.0, 22.0));
        if on_terminal {
            if let Some(t) = &self.terminal {
                painter.text(pos2(kill.left() - 10.0, header.center().y), Align2::RIGHT_CENTER, &t.name, FontId::proportional(12.0), p.text_muted);
            }
        }
        let close_resp = ui.interact(close, ui.scope_id().with("term-close"), Sense::click());
        let kill_resp = on_terminal.then(|| ui.interact(kill, ui.scope_id().with("term-kill"), Sense::click()));
        for (r, resp) in [(close, Some(&close_resp)), (kill, kill_resp.as_ref())] {
            if resp.is_some_and(|r| r.hovered()) {
                painter.rect_filled(r, CornerRadius::same(4), p.list_hover);
            }
        }
        draw_x(&painter, close.center(), 4.5, p.text_ui);
        if on_terminal {
            draw_trash_icon(&painter, kill.center(), p.text_ui);
        }
        if close_resp.on_hover_text(format!("Hide Panel ({})", self.keys.label(Command::ToggleTerminal))).clicked() {
            self.show_terminal = false;
            self.terminal_focused = false;
            return;
        }
        if kill_resp.is_some_and(|r| r.on_hover_text("Kill Terminal").clicked()) {
            self.terminal = None;
            self.show_terminal = false;
            self.terminal_focused = false;
            return;
        }
        if !on_terminal {
            let body = Rect::from_min_max(pos2(rect.left(), header.bottom()), rect.max);
            self.problems_list(ui, body);
            return;
        }

        // Screen grid.
        let body = Rect::from_min_max(pos2(rect.left() + 20.0, header.bottom() + 4.0), pos2(rect.right() - 14.0, rect.bottom() - 4.0));
        let size = if self.settings.terminal_font_size > 0.0 { self.settings.terminal_font_size } else { self.settings.font_size };
        let font = FontId::monospace(size);
        let char_w = ui.ctx().fonts_mut(|f| f.glyph_width(&font, 'M'));
        let line_h = (size * 1.3).round();
        let cols = (body.width() / char_w).max(10.0) as u16;
        let rows = (body.height() / line_h).max(2.0) as u16;

        if self.terminal.is_none() {
            let ctx = ui.ctx().clone();
            let shell = Some(self.settings.terminal_shell.trim()).filter(|s| !s.is_empty());
            match Terminal::spawn_shell(&self.explorer.root, shell, rows, cols, move || ctx.request_repaint()) {
                Ok(t) => self.terminal = Some(t),
                Err(e) => {
                    self.error(e);
                    self.show_terminal = false;
                    return;
                }
            }
        }
        let Some(t) = &mut self.terminal else { return };
        if t.has_exited() {
            self.terminal = None;
            self.show_terminal = false;
            self.terminal_focused = false;
            return;
        }
        t.resize(rows, cols);

        let area = Rect::from_min_max(pos2(rect.left(), header.bottom()), rect.max);
        if ui.interact(area, ui.scope_id().with("term-body"), Sense::click()).clicked() {
            self.terminal_focused = true;
        }
        if ui.rect_contains_pointer(area) {
            self.terminal_scroll += ui.input(|i| i.smooth_scroll_delta.y);
            let lines = (self.terminal_scroll / line_h) as isize;
            if lines != 0 {
                self.terminal_scroll -= lines as f32 * line_h;
                t.scroll(lines);
            }
        }

        let focused = self.terminal_focused;
        let painter = ui.painter_at(area);
        t.with_screen(|s| {
            let (_, ncols) = s.size();
            let (rows, ncols) = (rows.min(s.size().0.get()), ncols.get());
            for r in 0..rows {
                let y = body.top() + r as f32 * line_h;
                let mut run = String::new();
                let mut run_start = 0u16;
                let mut run_fg = p.text;
                let flush = |run: &mut String, start: u16, fg: Color32| {
                    if !run.trim_end().is_empty() {
                        let x = body.left() + start as f32 * char_w;
                        painter.text(pos2(x, y + line_h / 2.0), Align2::LEFT_CENTER, run.as_str(), font.clone(), fg);
                    }
                    run.clear();
                };
                let mut c = 0;
                while c < ncols {
                    let Some(cell) = s.cell(r, c) else { break };
                    if cell.is_wide_continuation() {
                        c += 1;
                        continue;
                    }
                    let mut fg = terminal::color(cell.fgcolor(), true, cell.bold(), &p);
                    let mut bg = terminal::color(cell.bgcolor(), false, false, &p);
                    if cell.inverse() {
                        std::mem::swap(&mut fg, &mut bg);
                    }
                    if cell.dim() {
                        fg = fg.gamma_multiply(0.6);
                    }
                    let w: u16 = if cell.is_wide() { 2 } else { 1 };
                    if bg != p.panel_bg {
                        let x = body.left() + c as f32 * char_w;
                        painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(char_w * w as f32, line_h)), 0.0, bg);
                    }
                    let text = if cell.has_contents() { cell.contents() } else { " " };
                    // ASCII cells of one color share a text run; anything else is drawn alone
                    // so wide and fallback-font glyphs stay on the grid.
                    if fg != run_fg || !text.is_ascii() || w > 1 {
                        flush(&mut run, run_start, run_fg);
                        run_start = c;
                        run_fg = fg;
                    }
                    run.push_str(text);
                    if !text.is_ascii() || w > 1 {
                        flush(&mut run, run_start, run_fg);
                        run_start = c + w;
                    }
                    if cell.underline() {
                        let x = body.left() + c as f32 * char_w;
                        painter.line_segment([pos2(x, y + line_h - 2.0), pos2(x + char_w * w as f32, y + line_h - 2.0)], Stroke::new(1.0, fg));
                    }
                    c += w;
                }
                flush(&mut run, run_start, run_fg);
            }
            if !s.hide_cursor() && s.scrollback() == 0 {
                let (cr, cc) = s.cursor_position();
                let cell = Rect::from_min_size(
                    pos2(body.left() + cc as f32 * char_w, body.top() + cr as f32 * line_h),
                    vec2(char_w, line_h),
                );
                if focused {
                    painter.rect_filled(cell, 0.0, p.cursor);
                    if let Some(ch) = s.cell(cr, cc).filter(|c| c.has_contents()) {
                        painter.text(cell.left_center(), Align2::LEFT_CENTER, ch.contents(), font.clone(), p.panel_bg);
                    }
                } else {
                    painter.rect_stroke(cell, 0.0, Stroke::new(1.0, p.cursor), egui::StrokeKind::Inside);
                }
            }
        });
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
                    painter.rect_filled(r, CornerRadius::same(5), theme::p().list_selected);
                } else if resp.hovered() {
                    painter.rect_filled(r, CornerRadius::same(5), theme::p().list_hover);
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
    fn on_exit(&mut self) {
        // Remember the open files for next time (not for test snapshots).
        if self.settings.restore_session && self.snapshot.is_none() {
            let s = self.ws.session();
            if let Err(e) = session::save(&self.explorer.root, &s) {
                eprintln!("vex: could not save the session: {e}");
            }
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(s) = &mut self.snapshot {
            raw_input.events.extend(s.take_events());
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(s) = &mut self.snapshot {
            s.tick(&ctx);
        }
        self.handle_input(&ctx);
        self.check_mapping_timeout(&ctx);
        self.auto_save(&ctx);
        self.update_diagnostics(&ctx);
        self.update_git(&ctx);
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
        if self.show_terminal {
            egui::Panel::bottom("terminal")
                .resizable(true)
                .default_size(260.0)
                .size_range(90.0..=900.0)
                .frame(Frame::NONE.fill(theme::p().panel_bg))
                .show(ui, |ui| self.terminal_panel(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if self.settings_page.is_some() {
                self.settings_page_ui(ui);
            } else {
                self.editor_area(ui);
            }
        });
        self.save_settings_if_idle(&ctx);

        self.quick_open_overlay(&ctx);
        self.palette_overlay(&ctx);
        self.cmdline_overlay(&ctx);
        if let Some(panel) = &mut self.keys_panel {
            let script_maps: Vec<Mapping> =
                self.vim.keymaps.maps.iter().filter(|m| m.origin == Origin::Script).cloned().collect();
            let out = panel.show(&ctx, &mut self.keys, &script_maps);
            if out.changed {
                self.save_keybindings();
            }
            if out.close {
                self.keys_panel = None;
            }
            if out.open_script {
                self.open_init_script();
            }
        }

        // Keep egui widgets from holding keyboard focus; keys belong to the editor.
        let typing = self.search.has_focus || self.search.focus_requested;
        if !self.overlay_open() && self.keys_panel.is_none() && self.settings_page.is_none() && !typing {
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

fn draw_welcome(p: &Painter, rect: Rect, logo: &egui::TextureHandle, keys: &Keybindings) {
    let c = rect.center();
    let pal = theme::p();
    let logo_rect = Rect::from_center_size(pos2(c.x, c.y - 170.0), vec2(104.0, 104.0));
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    p.image(logo.id(), logo_rect, uv, Color32::WHITE);
    p.text(pos2(c.x, logo_rect.bottom() + 22.0), Align2::CENTER_CENTER, "Vex", FontId::proportional(24.0), pal.text_bright);
    p.text(
        pos2(c.x, logo_rect.bottom() + 44.0),
        Align2::CENTER_CENTER,
        format!("version {}", env!("CARGO_PKG_VERSION")),
        FontId::proportional(12.0),
        pal.text_muted,
    );
    let rows = [
        ("Show All Commands", keys.label(Command::CommandPalette)),
        ("Go to File", keys.label(Command::QuickOpen)),
        ("Search in Files", keys.label(Command::FindInFiles)),
        ("Command Line", ":".into()),
        ("Open File", ":e <path>".into()),
        ("Toggle Sidebar", keys.label(Command::ToggleSidebar)),
        ("Terminal", keys.label(Command::ToggleTerminal)),
        ("Keyboard Shortcuts", keys.label(Command::Keybindings)),
        ("Save", format!(":w  /  {}", keys.label(Command::Save))),
    ];
    for (i, (label, keys)) in rows.iter().enumerate() {
        let y = c.y - 16.0 + i as f32 * 30.0;
        p.text(pos2(c.x - 12.0, y), Align2::RIGHT_CENTER, *label, FontId::proportional(13.0), pal.text_muted);
        // Each key combination as keycaps: "Ctrl+P" → [Ctrl] [P].
        let mut x = c.x + 12.0;
        for (j, part) in keys.split("  /  ").enumerate() {
            if j > 0 {
                let r = p.text(pos2(x, y), Align2::LEFT_CENTER, "or", FontId::proportional(12.0), pal.text_muted);
                x = r.right() + 8.0;
            }
            let caps: Vec<&str> = if part.len() > 1 && part.contains('+') && !part.ends_with('+') { part.split('+').collect() } else { vec![part] };
            for cap in caps {
                let g = p.layout_no_wrap(cap.to_string(), FontId::monospace(12.0), pal.text_ui);
                let chip = Rect::from_min_size(pos2(x, y - 10.0), vec2(g.size().x + 12.0, 20.0));
                p.rect_filled(chip, CornerRadius::same(4), pal.input_bg);
                p.rect_stroke(chip, CornerRadius::same(4), Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
                // A slightly darker bottom edge, like a key.
                p.line_segment([chip.left_bottom() + vec2(3.0, -0.5), chip.right_bottom() + vec2(-3.0, -0.5)], Stroke::new(1.5, pal.border));
                p.galley(pos2(chip.left() + 6.0, y - g.size().y / 2.0), g, pal.text_ui);
                x = chip.right() + 4.0;
            }
            x += 4.0;
        }
    }
}
