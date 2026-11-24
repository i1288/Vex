//! The Settings page: every option with a description and the right control,
//! searchable, grouped into sections, applied live.

use std::path::PathBuf;

use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Ui};

use crate::settings::{self, Settings, CURSOR_STYLES, WHITESPACE_MODES};
use crate::theme::{self, Palette, COLOR_FIELDS};

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Appearance,
    Theme,
    Editor,
    Files,
    Terminal,
    Keyboard,
}

const SECTIONS: &[(Section, &str)] = &[
    (Section::Appearance, "Appearance"),
    (Section::Theme, "Theme & Colors"),
    (Section::Editor, "Editor"),
    (Section::Files, "Files"),
    (Section::Terminal, "Terminal"),
    (Section::Keyboard, "Keyboard"),
];

#[derive(Default)]
pub struct Outcome {
    /// Settings changed: apply them now and save soon.
    pub changed: bool,
    pub close: bool,
    pub open_keys: bool,
    pub open_init_script: bool,
    pub open_file: Option<PathBuf>,
    /// Theme files changed on disk: reload them.
    pub reload_themes: bool,
}

pub struct SettingsPage {
    section: Section,
    search: String,
    color_search: String,
    /// Monospace fonts found on the system, scanned the first time they're needed.
    fonts: Option<Vec<(String, PathBuf)>>,
    /// Installed language servers (name → found command), looked up once.
    servers: Option<Vec<(String, Option<String>, &'static str)>>,
    new_theme_name: String,
    notice: Option<String>,
}

impl Default for SettingsPage {
    fn default() -> Self {
        Self {
            section: Section::Appearance,
            search: String::new(),
            color_search: String::new(),
            fonts: None,
            servers: None,
            new_theme_name: "My Theme".into(),
            notice: None,
        }
    }
}

/// Lays out one setting: title, description, control. Hidden when it doesn't
/// match the search.
struct Rows {
    search: String,
    shown: usize,
}

impl Rows {
    fn row(&mut self, ui: &mut Ui, title: &str, desc: &str, control: impl FnOnce(&mut Ui) -> bool) -> bool {
        if !self.search.is_empty() && !format!("{title} {desc}").to_lowercase().contains(&self.search) {
            return false;
        }
        self.shown += 1;
        let p = theme::p();
        ui.add_space(12.0);
        ui.label(RichText::new(title).size(14.0).color(p.text_bright).strong());
        ui.add_space(2.0);
        ui.label(RichText::new(desc).size(12.5).color(p.text_muted));
        ui.add_space(6.0);
        let changed = control(ui);
        ui.add_space(6.0);
        changed
    }
}

fn combo(ui: &mut Ui, id: &str, value: &mut String, options: &[(&str, &str)]) -> bool {
    let mut changed = false;
    let current = options.iter().find(|(v, _)| v == value).map_or(value.as_str(), |(_, l)| l).to_string();
    egui::ComboBox::from_id_salt(id).width(220.0).selected_text(current).show_ui(ui, |ui| {
        for (v, label) in options {
            changed |= ui.selectable_value(value, v.to_string(), *label).changed();
        }
    });
    changed
}

impl SettingsPage {
    /// Jump to the section with this title.
    pub fn show_section(&mut self, title: &str) {
        if let Some(&(section, _)) = SECTIONS.iter().find(|s| s.1 == title) {
            self.section = section;
            self.search.clear();
        }
    }

    pub fn show(&mut self, ui: &mut Ui, rect: Rect, st: &mut Settings) -> Outcome {
        let mut out = Outcome::default();
        let p = theme::p();
        ui.painter().rect_filled(rect, 0.0, p.editor_bg);
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) && !ui.ctx().memory(|m| m.focused().is_some()) {
            out.close = true;
        }

        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(28.0, 18.0))));
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        ui.spacing_mut().button_padding = vec2(10.0, 4.0);
        ui.spacing_mut().interact_size.y = 24.0;

        // Header: title, search, file link, close.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").size(22.0).color(p.text_bright));
            ui.add_space(24.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search settings")
                    .desired_width(320.0)
                    .margin(egui::Margin::symmetric(8, 5)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    out.close = true;
                }
                if ui.button("Open settings.toml").on_hover_text("Edit the settings file directly").clicked() {
                    out.open_file = settings::ensure_file().ok();
                    out.close = true;
                }
            });
        });
        ui.add_space(10.0);
        let sep_y = ui.cursor().top();
        ui.painter().line_segment([pos2(rect.left(), sep_y), pos2(rect.right(), sep_y)], Stroke::new(1.0, p.separator));
        ui.add_space(8.0);

        let search = self.search.trim().to_lowercase();
        let body = ui.available_rect_before_wrap();
        let nav = Rect::from_min_size(body.min, vec2(190.0, body.height()));
        let content = Rect::from_min_max(pos2(nav.right() + 24.0, body.top()), body.max);

        // Section list.
        let mut nav_ui = ui.new_child(egui::UiBuilder::new().max_rect(nav));
        for &(section, label) in SECTIONS {
            let (r, resp) = nav_ui.allocate_exact_size(vec2(nav.width(), 30.0), Sense::click());
            let selected = search.is_empty() && self.section == section;
            if selected {
                nav_ui.painter().rect_filled(r, CornerRadius::same(6), p.list_active);
                // A short pill inside the highlight, like the activity bar's marker.
                let marker = Rect::from_center_size(pos2(r.left() + 5.0, r.center().y), vec2(3.0, 14.0));
                nav_ui.painter().rect_filled(marker, CornerRadius::same(2), p.accent);
            } else if resp.hovered() {
                nav_ui.painter().rect_filled(r, CornerRadius::same(6), p.list_hover);
            }
            let color = if selected { p.text_bright } else { p.text_ui };
            nav_ui.painter().text(r.left_center() + vec2(16.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(13.5), color);
            if resp.clicked() {
                self.section = section;
                self.search.clear();
            }
        }

        // Settings.
        let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content));
        egui::ScrollArea::vertical().id_salt(("settings-scroll", search.is_empty(), self.section as u8)).auto_shrink(false).show(&mut content_ui, |ui| {
            ui.set_max_width(content.width().min(760.0));
            let mut rows = Rows { search: search.clone(), shown: 0 };
            let sections: Vec<Section> = if search.is_empty() { vec![self.section] } else { SECTIONS.iter().map(|s| s.0).collect() };
            for section in sections {
                if search.is_empty() {
                    let title = SECTIONS.iter().find(|s| s.0 == section).map_or("", |s| s.1);
                    ui.label(RichText::new(title).size(18.0).color(p.text_bright));
                }
                out.changed |= match section {
                    Section::Appearance => self.appearance(ui, &mut rows, st),
                    Section::Theme => self.theme(ui, &mut rows, st, &mut out),
                    Section::Editor => self.editor(ui, &mut rows, st),
                    Section::Files => files(ui, &mut rows, st),
                    Section::Terminal => terminal(ui, &mut rows, st),
                    Section::Keyboard => keyboard(ui, &mut rows, st, &mut out),
                };
            }
            if rows.shown == 0 {
                ui.add_space(20.0);
                ui.label(RichText::new(format!("No settings match \"{}\"", self.search.trim())).color(p.text_muted));
            }
            if let Some(n) = &self.notice {
                ui.add_space(16.0);
                ui.label(RichText::new(n).color(p.accent));
            }
            ui.add_space(40.0);
        });
        out
    }

    fn appearance(&mut self, ui: &mut Ui, rows: &mut Rows, st: &mut Settings) -> bool {
        let mut changed = false;
        changed |= rows.row(ui, "Interface size", "Scales the whole window: text, icons and panels.", |ui| {
            let mut changed = false;
            let label = format!("{:.0}%", st.ui_scale * 100.0);
            egui::ComboBox::from_id_salt("ui-scale").width(120.0).selected_text(label).show_ui(ui, |ui| {
                for pct in [75, 80, 90, 100, 110, 125, 150, 175, 200] {
                    changed |= ui.selectable_value(&mut st.ui_scale, pct as f32 / 100.0, format!("{pct}%")).changed();
                }
            });
            changed
        });
        changed |= rows.row(ui, "Editor font", "Any monospace font installed on your computer.", |ui| {
            let fonts = self.fonts.get_or_insert_with(theme::find_mono_fonts);
            let current = if st.font_family.is_empty() {
                "Hack (built-in)".to_string()
            } else {
                fonts
                    .iter()
                    .find(|(_, path)| path.to_string_lossy() == st.font_family)
                    .map_or_else(|| st.font_family.clone(), |(name, _)| name.clone())
            };
            let mut changed = false;
            egui::ComboBox::from_id_salt("font-family").width(280.0).selected_text(current).show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut st.font_family, String::new(), "Hack (built-in)").changed();
                for (name, path) in fonts.iter() {
                    changed |= ui.selectable_value(&mut st.font_family, path.to_string_lossy().into_owned(), name).changed();
                }
            });
            if fonts.is_empty() {
                ui.label(RichText::new("No other monospace fonts found.").color(theme::p().text_muted));
            }
            changed
        });
        changed |= rows.row(ui, "Font size", "Size of code in the editor. Ctrl+= and Ctrl+- zoom for the session.", |ui| {
            ui.add(egui::Slider::new(&mut st.font_size, 8.0..=32.0).step_by(1.0).suffix(" pt")).changed()
        });
        changed |= rows.row(ui, "Line height", "Space between lines, as a multiple of the font size.", |ui| {
            ui.add(egui::Slider::new(&mut st.line_height, 1.0..=2.2).step_by(0.05).fixed_decimals(2)).changed()
        });
        changed |= rows.row(ui, "Cursor style", "Shape of the cursor in Normal mode (Insert mode always uses a line).", |ui| {
            let options: Vec<(&str, &str)> = CURSOR_STYLES.iter().map(|s| (*s, *s)).collect();
            combo(ui, "cursor-style", &mut st.cursor_style, &options)
        });
        changed |= rows.row(ui, "Cursor blinking", "Blink the cursor when you stop typing.", |ui| ui.checkbox(&mut st.cursor_blink, "Blink").changed());
        changed
    }

    fn theme(&mut self, ui: &mut Ui, rows: &mut Rows, st: &mut Settings, out: &mut Outcome) -> bool {
        let mut changed = false;
        changed |= rows.row(ui, "Color theme", "Click a theme to use it. Your own theme files appear here too.", |ui| {
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
                for pal in theme::all() {
                    let selected = theme::find(&st.theme).is_some_and(|t| t.name == pal.name);
                    if theme_card(ui, &pal, selected).clicked() && !selected {
                        st.theme = pal.name.to_string();
                        changed = true;
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Reload theme files").on_hover_text("Pick up changes to files in the themes folder").clicked() {
                    out.reload_themes = true;
                }
            });
            changed
        });
        changed |= rows.row(
            ui,
            "Customize colors",
            "Change any color of the current theme. Changes are saved in settings.toml under [colors]; Reset brings back the theme's color.",
            |ui| self.colors(ui, st),
        );
        rows.row(
            ui,
            "Save as a new theme",
            "Write the current colors (including your changes) to a theme file you can share or edit.",
            |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_theme_name).desired_width(200.0));
                    if ui.button("Save theme").clicked() {
                        let name = self.new_theme_name.trim().to_string();
                        match settings::themes_dir().map(|d| theme::export_current(&d, &name)) {
                            Some(Ok(path)) => {
                                self.notice = Some(format!("Saved {}. It is now in the theme list.", path.display()));
                                out.reload_themes = true;
                            }
                            Some(Err(e)) => self.notice = Some(format!("Could not save the theme: {e}")),
                            None => self.notice = Some("No config folder found".into()),
                        }
                    }
                });
                false
            },
        );
        changed
    }

    fn colors(&mut self, ui: &mut Ui, st: &mut Settings) -> bool {
        let mut changed = false;
        let p = theme::p();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.color_search).hint_text("Filter colors").desired_width(220.0));
            if !st.colors.is_empty() && ui.button(format!("Reset all ({})", st.colors.len())).clicked() {
                st.colors.clear();
                changed = true;
            }
        });
        let filter = self.color_search.to_lowercase();
        egui::Grid::new("color-grid").num_columns(3).spacing(vec2(16.0, 6.0)).striped(true).show(ui, |ui| {
            for (key, label) in COLOR_FIELDS {
                if !filter.is_empty() && !label.to_lowercase().contains(&filter) && !key.contains(&filter) {
                    continue;
                }
                ui.label(RichText::new(*label).color(p.text_ui));
                let mut color = p.color(key).unwrap_or(Color32::BLACK);
                if egui::color_picker::color_edit_button_srgba(ui, &mut color, egui::color_picker::Alpha::OnlyBlend).changed() {
                    st.colors.insert(key.to_string(), theme::to_hex(color));
                    changed = true;
                }
                if st.colors.contains_key(*key) {
                    if ui.small_button("Reset").clicked() {
                        st.colors.remove(*key);
                        changed = true;
                    }
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
        changed
    }
}

impl SettingsPage {
    fn editor(&mut self, ui: &mut Ui, rows: &mut Rows, st: &mut Settings) -> bool {
        let mut changed = editor(ui, rows, st);
        changed |= rows.row(
            ui,
            "Type checking",
            "Finds type errors, unknown names and other mistakes using each language's language server, the same tools VS Code uses. Servers that are installed start automatically; install others to add languages. To use a different server, set its command under [language_servers] in settings.toml.",
            |ui| {
                let changed = ui.checkbox(&mut st.type_check, "Use language servers").changed();
                let servers = self.servers.get_or_insert_with(|| {
                    crate::lsp::SERVERS
                        .iter()
                        .map(|(key, name, hint)| {
                            let found = crate::lsp::language_for(match *key {
                                "rust" => "rs",
                                "python" => "py",
                                "typescript" => "ts",
                                "c" => "c",
                                "go" => "go",
                                "bash" => "sh",
                                _ => "java",
                            })
                            .and_then(|lang| crate::lsp::resolve_command(&lang, &st.language_servers))
                            .map(|cmd| cmd.join(" "));
                            (name.to_string(), found, *hint)
                        })
                        .collect()
                });
                let p = theme::p();
                egui::Grid::new("servers").num_columns(2).spacing(vec2(18.0, 4.0)).show(ui, |ui| {
                    for (name, found, hint) in servers.iter() {
                        ui.label(RichText::new(name).color(p.text_ui));
                        match found {
                            Some(cmd) => ui.label(RichText::new(format!("✔ {cmd}")).color(p.string)),
                            None => ui.label(RichText::new(format!("not installed: {hint}")).color(p.text_muted)),
                        };
                        ui.end_row();
                    }
                });
                changed
            },
        );
        changed
    }
}

fn editor(ui: &mut Ui, rows: &mut Rows, st: &mut Settings) -> bool {
    let mut changed = false;
    changed |= rows.row(ui, "Tab size", "Width of a tab, and of one indent level.", |ui| {
        ui.add(egui::DragValue::new(&mut st.tab_size).range(1..=16)).changed()
    });
    changed |= rows.row(ui, "Indent with spaces", "Insert spaces when you press Tab or indent, instead of tab characters.", |ui| {
        ui.checkbox(&mut st.insert_spaces, "Use spaces").changed()
    });
    changed |= rows.row(ui, "Word wrap", "Wrap long lines at the edge of the editor (Alt+Z).", |ui| ui.checkbox(&mut st.word_wrap, "Wrap lines").changed());
    changed |= rows.row(ui, "Minimap", "Show a small overview of the whole file at the right edge.", |ui| ui.checkbox(&mut st.minimap, "Show minimap").changed());
    changed |= rows.row(ui, "Line numbers", "Numbers in the left margin. Relative numbers count from the cursor line, handy for vim motions like 5j.", |ui| {
        let a = ui.checkbox(&mut st.line_numbers, "Show line numbers").changed();
        let b = ui.checkbox(&mut st.relative_line_numbers, "Relative to the cursor").changed();
        a || b
    });
    changed |= rows.row(ui, "Indent guides", "Thin vertical lines at each indentation level.", |ui| ui.checkbox(&mut st.indent_guides, "Show indent guides").changed());
    changed |= rows.row(ui, "Brackets", "Highlight matching brackets, and close brackets and quotes as you type them.", |ui| {
        let a = ui.checkbox(&mut st.bracket_matching, "Highlight the matching bracket").changed();
        let b = ui.checkbox(&mut st.auto_close_brackets, "Auto-close brackets and quotes").changed();
        a || b
    });
    changed |= rows.row(ui, "Highlight current line", "Outline the line the cursor is on.", |ui| {
        ui.checkbox(&mut st.highlight_current_line, "Highlight").changed()
    });
    changed |= rows.row(ui, "Show whitespace", "Draw dots for spaces and arrows for tabs.", |ui| {
        let options: Vec<(&str, &str)> = WHITESPACE_MODES
            .iter()
            .map(|m| (*m, match *m {
                "none" => "Never",
                "boundary" => "At the start and end of lines",
                _ => "Everywhere",
            }))
            .collect();
        combo(ui, "whitespace", &mut st.render_whitespace, &options)
    });
    changed |= rows.row(
        ui,
        "Syntax checking",
        "Underline syntax errors as you type, for Rust, Python, JavaScript, TypeScript, C, C++, Go, Java, JSON, Bash, CSS, HTML, TOML and YAML. Jump between errors with ]d and [d.",
        |ui| ui.checkbox(&mut st.syntax_check, "Check syntax").changed(),
    );
    changed |= rows.row(ui, "Breadcrumbs", "Show the file's folder path above the editor.", |ui| ui.checkbox(&mut st.breadcrumbs, "Show breadcrumbs").changed());
    changed |= rows.row(ui, "Scroll margin", "Lines kept visible above and below the cursor when scrolling.", |ui| {
        ui.add(egui::DragValue::new(&mut st.scroll_off).range(0..=20).suffix(" lines")).changed()
    });
    changed
}

fn files(ui: &mut Ui, rows: &mut Rows, st: &mut Settings) -> bool {
    let mut changed = false;
    changed |= rows.row(ui, "Auto save", "Save files automatically shortly after you stop typing.", |ui| {
        let mut c = ui.checkbox(&mut st.auto_save, "Save automatically").changed();
        ui.add_enabled_ui(st.auto_save, |ui| {
            ui.horizontal(|ui| {
                ui.label("Delay");
                c |= ui.add(egui::DragValue::new(&mut st.auto_save_delay_ms).range(100..=60_000).speed(50).suffix(" ms")).changed();
            });
        });
        c
    });
    changed |= rows.row(ui, "Trim trailing whitespace", "Remove spaces and tabs at the ends of lines when saving.", |ui| {
        ui.checkbox(&mut st.trim_trailing_whitespace, "Trim on save").changed()
    });
    changed
}

fn terminal(ui: &mut Ui, rows: &mut Rows, st: &mut Settings) -> bool {
    let mut changed = false;
    changed |= rows.row(ui, "Terminal font size", "Size of text in the terminal panel.", |ui| {
        let mut same = st.terminal_font_size == 0.0;
        let mut c = false;
        if ui.checkbox(&mut same, "Same as the editor").changed() {
            st.terminal_font_size = if same { 0.0 } else { st.font_size };
            c = true;
        }
        if !same {
            c |= ui.add(egui::Slider::new(&mut st.terminal_font_size, 8.0..=32.0).step_by(1.0).suffix(" pt")).changed();
        }
        c
    });
    changed |= rows.row(ui, "Shell", "Program the terminal runs, with arguments (e.g. /bin/zsh or fish). Leave empty for your default shell. Applies to new terminals.", |ui| {
        ui.add(egui::TextEdit::singleline(&mut st.terminal_shell).hint_text("default shell").desired_width(300.0)).changed()
    });
    changed
}

fn keyboard(ui: &mut Ui, rows: &mut Rows, st: &mut Settings, out: &mut Outcome) -> bool {
    let changed = rows.row(
        ui,
        "Vim keys by position",
        "For Colemak, Dvorak, AZERTY and other layouts: vim commands use where a key sits on a US QWERTY keyboard, so h j k l stay together under your fingers. Typing text, : commands, searches and the character after f, t or r still use your layout.",
        |ui| ui.checkbox(&mut st.vim_key_positions, "Use key positions for vim commands").changed(),
    );
    rows.row(ui, "Keyboard shortcuts", "Change the keys for app commands, and add Vim mappings without writing a script.", |ui| {
        if ui.button("Open Keyboard Shortcuts").clicked() {
            out.open_keys = true;
        }
        false
    });
    rows.row(ui, "Startup script (init.vex)", "Vim-style commands run at startup, like a .vimrc: mappings, leader key, :set options.", |ui| {
        if ui.button("Open init.vex").clicked() {
            out.open_init_script = true;
            out.close = true;
        }
        false
    });
    changed
}

/// A clickable miniature of an editor drawn in `pal`'s colors.
fn theme_card(ui: &mut Ui, pal: &Palette, selected: bool) -> egui::Response {
    let size = vec2(176.0, 128.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect.expand(2.0));
    let preview = Rect::from_min_size(rect.min, vec2(size.x, 100.0));
    let r = CornerRadius::same(6);
    painter.rect_filled(preview, r, pal.editor_bg);
    // Activity bar and sidebar.
    let side = Rect::from_min_size(preview.min, vec2(34.0, preview.height()));
    painter.rect_filled(side, CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, pal.sidebar_bg);
    painter.rect_filled(Rect::from_min_size(side.min + vec2(0.0, 10.0), vec2(2.0, 10.0)), 0.0, pal.accent);
    for i in 0..4 {
        let y = preview.top() + 30.0 + i as f32 * 9.0;
        painter.rect_filled(Rect::from_min_size(pos2(side.left() + 8.0, y), vec2(18.0 - (i % 2) as f32 * 5.0, 3.0)), 1.0, pal.text_muted);
    }
    // Tab strip with the active tab.
    let tabs = Rect::from_min_max(pos2(side.right(), preview.top()), pos2(preview.right(), preview.top() + 16.0));
    painter.rect_filled(tabs, CornerRadius { ne: 6, nw: 0, sw: 0, se: 0 }, pal.tabs_bg);
    let tab = Rect::from_min_size(tabs.min, vec2(44.0, 16.0));
    painter.rect_filled(tab, 0.0, pal.editor_bg);
    painter.rect_filled(Rect::from_min_size(tab.min, vec2(tab.width(), 2.0)), 0.0, pal.accent);
    // A few lines of "code".
    let code = [
        [(pal.keyword, 14.0), (pal.function, 26.0), (pal.text, 10.0)],
        [(pal.control, 12.0), (pal.type_, 20.0), (pal.string, 30.0)],
        [(pal.comment, 56.0), (pal.comment, 0.0), (pal.comment, 0.0)],
        [(pal.keyword, 10.0), (pal.constant, 18.0), (pal.number, 12.0)],
        [(pal.function, 22.0), (pal.string, 34.0), (pal.text, 8.0)],
    ];
    for (i, line) in code.iter().enumerate() {
        let y = tabs.bottom() + 9.0 + i as f32 * 14.0;
        let mut x = side.right() + 10.0 + if i == 1 || i == 4 { 10.0 } else { 0.0 };
        if i == 1 {
            painter.rect_filled(Rect::from_min_size(pos2(side.right(), y - 4.0), vec2(preview.right() - side.right(), 12.0)), 0.0, pal.current_line);
        }
        for &(color, w) in line {
            if w > 0.0 {
                painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(w, 4.0)), 2.0, color);
                x += w + 5.0;
            }
        }
    }
    let ui_p = theme::p();
    let border = if selected {
        Stroke::new(2.0, ui_p.accent)
    } else if resp.hovered() {
        Stroke::new(1.0, ui_p.focus_border)
    } else {
        Stroke::new(1.0, ui_p.separator.gamma_multiply(2.0))
    };
    painter.rect_stroke(preview, r, border, egui::StrokeKind::Outside);
    painter.text(
        pos2(rect.left() + 2.0, preview.bottom() + 14.0),
        Align2::LEFT_CENTER,
        pal.title.as_ref(),
        FontId::proportional(13.0),
        if selected { ui_p.text_bright } else { ui_p.text_ui },
    );
    if selected {
        painter.text(pos2(rect.right() - 2.0, preview.bottom() + 14.0), Align2::RIGHT_CENTER, "✔", FontId::proportional(13.0), ui_p.accent);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
