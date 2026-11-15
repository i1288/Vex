//! Colors and fonts, modeled on VS Code's themes: "Dark Modern" and "Light
//! Modern" (the defaults), plus the classic "Dark+" and "Light+".

use std::sync::atomic::{AtomicU8, Ordering};

use eframe::egui::{self, Color32, FontData, FontFamily, Stroke};
use eframe::epaint::text::{FontInsert, FontPriority, InsertFontFamily};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub struct Palette {
    /// Name in settings.toml and `:colorscheme`.
    pub name: &'static str,
    pub dark: bool,
    pub editor_bg: Color32,
    pub sidebar_bg: Color32,
    pub activity_bg: Color32,
    pub tabs_bg: Color32,
    pub tab_inactive: Color32,
    pub status_bg: Color32,
    pub panel_bg: Color32,
    pub widget_bg: Color32,
    pub input_bg: Color32,
    /// Outline of inputs and popups.
    pub border: Color32,
    pub focus_border: Color32,
    /// 1px lines between the sidebar, tabs, editor, panel and status bar.
    pub separator: Color32,
    /// Active tab line, activity bar indicator.
    pub accent: Color32,

    pub text: Color32,
    pub text_ui: Color32,
    pub text_muted: Color32,
    pub text_bright: Color32,
    pub status_fg: Color32,
    pub activity_fg: Color32,
    pub icon_inactive: Color32,

    pub line_number: Color32,
    pub line_number_active: Color32,
    pub current_line: Color32,
    pub selection: Color32,
    pub search_match: Color32,
    pub search_border: Color32,
    pub cursor: Color32,
    pub indent_guide: Color32,
    pub list_hover: Color32,
    pub list_active: Color32,
    pub list_selected: Color32,
    pub tree_guide: Color32,
    pub scrollbar: Color32,

    pub keyword: Color32,
    pub control: Color32,
    pub type_: Color32,
    pub function: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub constant: Color32,

    /// The 16 ANSI terminal colors.
    pub ansi: [Color32; 16],
}

pub const DARK_MODERN: Palette = Palette {
    name: "dark",
    dark: true,
    editor_bg: hex(0x1f1f1f),
    sidebar_bg: hex(0x181818),
    activity_bg: hex(0x181818),
    tabs_bg: hex(0x181818),
    tab_inactive: hex(0x181818),
    status_bg: hex(0x181818),
    panel_bg: hex(0x181818),
    widget_bg: hex(0x222222),
    input_bg: hex(0x313131),
    border: hex(0x3c3c3c),
    focus_border: hex(0x0078d4),
    separator: hex(0x2b2b2b),
    accent: hex(0x0078d4),
    text: hex(0xcccccc),
    text_ui: hex(0xcccccc),
    text_muted: hex(0x9d9d9d),
    text_bright: hex(0xffffff),
    status_fg: hex(0xcccccc),
    activity_fg: hex(0xd7d7d7),
    icon_inactive: hex(0x868686),
    line_number: hex(0x6e7681),
    line_number_active: hex(0xcccccc),
    current_line: hex(0x282828),
    selection: hex(0x264f78),
    search_match: Color32::from_rgba_premultiplied(0x50, 0x2a, 0x00, 0x55),
    search_border: hex(0x8a4b14),
    cursor: hex(0xaeafad),
    indent_guide: hex(0x404040),
    list_hover: hex(0x2a2d2e),
    list_active: hex(0x37373d),
    list_selected: hex(0x04395e),
    tree_guide: hex(0x3c3c3c),
    scrollbar: hex(0x797979),
    keyword: hex(0x569cd6),
    control: hex(0xc586c0),
    type_: hex(0x4ec9b0),
    function: hex(0xdcdcaa),
    string: hex(0xce9178),
    number: hex(0xb5cea8),
    comment: hex(0x6a9955),
    constant: hex(0x4fc1ff),
    ansi: [
        hex(0x000000), hex(0xcd3131), hex(0x0dbc79), hex(0xe5e510),
        hex(0x2472c8), hex(0xbc3fbc), hex(0x11a8cd), hex(0xe5e5e5),
        hex(0x666666), hex(0xf14c4c), hex(0x23d18b), hex(0xf5f543),
        hex(0x3b8eea), hex(0xd670d6), hex(0x29b8db), hex(0xe5e5e5),
    ],
};

pub const LIGHT_MODERN: Palette = Palette {
    name: "light",
    dark: false,
    editor_bg: hex(0xffffff),
    sidebar_bg: hex(0xf8f8f8),
    activity_bg: hex(0xf8f8f8),
    tabs_bg: hex(0xf8f8f8),
    tab_inactive: hex(0xf8f8f8),
    status_bg: hex(0xf8f8f8),
    panel_bg: hex(0xf8f8f8),
    widget_bg: hex(0xf8f8f8),
    input_bg: hex(0xffffff),
    border: hex(0xcecece),
    focus_border: hex(0x005fb8),
    separator: hex(0xe5e5e5),
    accent: hex(0x005fb8),
    text: hex(0x3b3b3b),
    text_ui: hex(0x3b3b3b),
    text_muted: hex(0x6f6f6f),
    text_bright: hex(0x1f1f1f),
    status_fg: hex(0x3b3b3b),
    activity_fg: hex(0x1f1f1f),
    icon_inactive: hex(0x616161),
    line_number: hex(0x6e7681),
    line_number_active: hex(0x171184),
    current_line: hex(0xeeeeee),
    selection: hex(0xadd6ff),
    search_match: Color32::from_rgba_premultiplied(0x9e, 0x3e, 0x00, 0x55),
    search_border: hex(0xd18616),
    cursor: hex(0x000000),
    indent_guide: hex(0xd3d3d3),
    list_hover: hex(0xf2f2f2),
    list_active: hex(0xe4e6f1),
    list_selected: hex(0xcce5ff),
    tree_guide: hex(0xd3d3d3),
    scrollbar: hex(0x646464),
    keyword: hex(0x0000ff),
    control: hex(0xaf00db),
    type_: hex(0x267f99),
    function: hex(0x795e26),
    string: hex(0xa31515),
    number: hex(0x098658),
    comment: hex(0x008000),
    constant: hex(0x0070c1),
    ansi: [
        hex(0x000000), hex(0xcd3131), hex(0x00bc00), hex(0x949800),
        hex(0x0451a5), hex(0xbc05bc), hex(0x0598bc), hex(0x555555),
        hex(0x666666), hex(0xcd3131), hex(0x14ce14), hex(0xb5ba00),
        hex(0x0451a5), hex(0xbc05bc), hex(0x0598bc), hex(0xa5a5a5),
    ],
};

pub const DARK_PLUS: Palette = Palette {
    name: "dark-plus",
    dark: true,
    editor_bg: hex(0x1e1e1e),
    sidebar_bg: hex(0x252526),
    activity_bg: hex(0x333333),
    tabs_bg: hex(0x252526),
    tab_inactive: hex(0x2d2d2d),
    status_bg: hex(0x007acc),
    panel_bg: hex(0x1e1e1e),
    widget_bg: hex(0x252526),
    input_bg: hex(0x3c3c3c),
    border: hex(0x454545),
    focus_border: hex(0x007fd4),
    text: hex(0xd4d4d4),
    text_ui: hex(0xcccccc),
    text_muted: hex(0x969696),
    text_bright: hex(0xffffff),
    status_fg: hex(0xffffff),
    activity_fg: hex(0xffffff),
    accent: hex(0x007acc),
    // The classic themes separate regions by color alone.
    separator: Color32::TRANSPARENT,
    icon_inactive: hex(0x858585),
    line_number: hex(0x858585),
    line_number_active: hex(0xc6c6c6),
    current_line: hex(0x282828),
    selection: hex(0x264f78),
    search_match: Color32::from_rgba_premultiplied(0x50, 0x2a, 0x00, 0x55),
    search_border: hex(0x8a4b14),
    cursor: hex(0xaeafad),
    indent_guide: hex(0x404040),
    list_hover: hex(0x2a2d2e),
    list_active: hex(0x37373d),
    list_selected: hex(0x04395e),
    tree_guide: hex(0x454545),
    scrollbar: hex(0x797979),
    keyword: hex(0x569cd6),
    control: hex(0xc586c0),
    type_: hex(0x4ec9b0),
    function: hex(0xdcdcaa),
    string: hex(0xce9178),
    number: hex(0xb5cea8),
    comment: hex(0x6a9955),
    constant: hex(0x4fc1ff),
    ansi: [
        hex(0x000000), hex(0xcd3131), hex(0x0dbc79), hex(0xe5e510),
        hex(0x2472c8), hex(0xbc3fbc), hex(0x11a8cd), hex(0xe5e5e5),
        hex(0x666666), hex(0xf14c4c), hex(0x23d18b), hex(0xf5f543),
        hex(0x3b8eea), hex(0xd670d6), hex(0x29b8db), hex(0xe5e5e5),
    ],
};

pub const LIGHT_PLUS: Palette = Palette {
    name: "light-plus",
    dark: false,
    editor_bg: hex(0xffffff),
    sidebar_bg: hex(0xf3f3f3),
    activity_bg: hex(0x2c2c2c),
    tabs_bg: hex(0xf3f3f3),
    tab_inactive: hex(0xececec),
    status_bg: hex(0x007acc),
    panel_bg: hex(0xffffff),
    widget_bg: hex(0xf3f3f3),
    input_bg: hex(0xffffff),
    border: hex(0xc8c8c8),
    focus_border: hex(0x0090f1),
    text: hex(0x000000),
    text_ui: hex(0x616161),
    text_muted: hex(0x6f6f6f),
    text_bright: hex(0x000000),
    status_fg: hex(0xffffff),
    activity_fg: hex(0xffffff),
    accent: hex(0x007acc),
    // The classic themes separate regions by color alone.
    separator: Color32::TRANSPARENT,
    icon_inactive: hex(0x8a8a8a),
    line_number: hex(0x237893),
    line_number_active: hex(0x0b216f),
    current_line: hex(0xeeeeee),
    selection: hex(0xadd6ff),
    search_match: Color32::from_rgba_premultiplied(0x9e, 0x3e, 0x00, 0x55),
    search_border: hex(0xd18616),
    cursor: hex(0x000000),
    indent_guide: hex(0xd3d3d3),
    list_hover: hex(0xe8e8e8),
    list_active: hex(0xe4e6f1),
    list_selected: hex(0xcce5ff),
    tree_guide: hex(0xc8c8c8),
    scrollbar: hex(0x646464),
    keyword: hex(0x0000ff),
    control: hex(0xaf00db),
    type_: hex(0x267f99),
    function: hex(0x795e26),
    string: hex(0xa31515),
    number: hex(0x098658),
    comment: hex(0x008000),
    constant: hex(0x0070c1),
    ansi: [
        hex(0x000000), hex(0xcd3131), hex(0x00bc00), hex(0x949800),
        hex(0x0451a5), hex(0xbc05bc), hex(0x0598bc), hex(0x555555),
        hex(0x666666), hex(0xcd3131), hex(0x14ce14), hex(0xb5ba00),
        hex(0x0451a5), hex(0xbc05bc), hex(0x0598bc), hex(0xa5a5a5),
    ],
};

pub const PALETTES: [&Palette; 4] = [&DARK_MODERN, &LIGHT_MODERN, &DARK_PLUS, &LIGHT_PLUS];

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The active palette.
pub fn p() -> &'static Palette {
    PALETTES[CURRENT.load(Ordering::Relaxed) as usize]
}

/// Theme names accepted in settings, with aliases.
pub fn lookup(name: &str) -> Option<usize> {
    let name = match name {
        "dark-modern" => "dark",
        "light-modern" => "light",
        "dark+" => "dark-plus",
        "light+" => "light-plus",
        other => other,
    };
    PALETTES.iter().position(|p| p.name == name)
}

/// Select a palette by name; returns false for an unknown name.
pub fn set(name: &str) -> bool {
    match lookup(name) {
        Some(i) => {
            CURRENT.store(i as u8, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Status-bar badge color for each mode.
pub fn mode_color(mode: crate::vim::Mode) -> Color32 {
    use crate::vim::Mode::*;
    match mode {
        Normal => hex(0x16508a),
        Insert => hex(0x16825d),
        Visual | VisualLine => hex(0x8b4fbf),
        Command | Search => hex(0xb5651d),
    }
}

/// Icon color for a file, by extension (loosely after the Seti icon theme).
pub fn file_color(name: &str) -> Color32 {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "rs" => hex(0xdea584),
        "py" => hex(0x519aba),
        "js" | "mjs" | "cjs" | "json" => hex(0xcbcb41),
        "ts" | "tsx" | "jsx" | "c" | "h" | "go" | "md" | "css" => hex(0x519aba),
        "cpp" | "cc" | "hpp" | "cxx" => hex(0xa074c4),
        "html" | "xml" | "java" => hex(0xe37933),
        "toml" | "yaml" | "yml" | "ini" | "lock" | "cfg" => hex(0x6d8086),
        "sh" | "bash" | "zsh" => hex(0x8dc149),
        _ => hex(0x8a8a8a),
    }
}

/// Apply the active palette to egui's widget styling.
pub fn apply_visuals(ctx: &egui::Context) {
    let p = p();
    let (theme, mut v) = if p.dark {
        (egui::Theme::Dark, egui::Visuals::dark())
    } else {
        (egui::Theme::Light, egui::Visuals::light())
    };
    v.panel_fill = p.editor_bg;
    v.window_fill = p.widget_bg;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.input_bg;
    v.override_text_color = Some(p.text_ui);
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0, p.text_bright);
    // Panel edges: egui draws these between the sidebar, editor, terminal and status bar.
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.focus_border);
    v.window_corner_radius = 6.into();
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, v);
}

pub fn install(ctx: &egui::Context) {
    apply_visuals(ctx);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(0.0, 0.0);
        s.interaction.selectable_labels = false;
    });

    // Use a system UI font if one is installed; egui's bundled fonts are the fallback.
    let ui_fonts = [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    if let Some(data) = ui_fonts.iter().find_map(|p| std::fs::read(p).ok()) {
        ctx.add_font(FontInsert::new(
            "ui",
            FontData::from_owned(data),
            vec![InsertFontFamily { family: FontFamily::Proportional, priority: FontPriority::Highest }],
        ));
    }
}
