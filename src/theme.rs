//! Colors and fonts, modeled on VS Code's "Dark+" theme.

use eframe::egui::{self, Color32, FontData, FontFamily, Stroke};
use eframe::epaint::text::{FontInsert, FontPriority, InsertFontFamily};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const EDITOR_BG: Color32 = hex(0x1e1e1e);
pub const SIDEBAR_BG: Color32 = hex(0x252526);
pub const ACTIVITY_BG: Color32 = hex(0x333333);
pub const TABS_BG: Color32 = hex(0x252526);
pub const TAB_INACTIVE: Color32 = hex(0x2d2d2d);
pub const STATUS_BG: Color32 = hex(0x007acc);
pub const WIDGET_BG: Color32 = hex(0x252526);
pub const INPUT_BG: Color32 = hex(0x3c3c3c);
pub const BORDER: Color32 = hex(0x454545);
pub const FOCUS_BORDER: Color32 = hex(0x007fd4);

pub const TEXT: Color32 = hex(0xd4d4d4);
pub const TEXT_UI: Color32 = hex(0xcccccc);
pub const TEXT_MUTED: Color32 = hex(0x969696);
pub const TEXT_BRIGHT: Color32 = hex(0xffffff);
pub const ICON_INACTIVE: Color32 = hex(0x858585);

pub const LINE_NUMBER: Color32 = hex(0x858585);
pub const LINE_NUMBER_ACTIVE: Color32 = hex(0xc6c6c6);
pub const CURRENT_LINE: Color32 = hex(0x282828);
pub const SELECTION: Color32 = hex(0x264f78);
pub const SEARCH_MATCH: Color32 = Color32::from_rgba_premultiplied(0x50, 0x2a, 0x00, 0x55);
pub const CURSOR: Color32 = hex(0xaeafad);
pub const INDENT_GUIDE: Color32 = hex(0x404040);
pub const LIST_HOVER: Color32 = hex(0x2a2d2e);
pub const LIST_ACTIVE: Color32 = hex(0x37373d);
pub const LIST_SELECTED: Color32 = hex(0x04395e);

// Syntax colors.
pub const KEYWORD: Color32 = hex(0x569cd6);
pub const CONTROL: Color32 = hex(0xc586c0);
pub const TYPE: Color32 = hex(0x4ec9b0);
pub const FUNCTION: Color32 = hex(0xdcdcaa);
pub const STRING: Color32 = hex(0xce9178);
pub const NUMBER: Color32 = hex(0xb5cea8);
pub const COMMENT: Color32 = hex(0x6a9955);
pub const CONSTANT: Color32 = hex(0x4fc1ff);

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

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = EDITOR_BG;
    v.window_fill = WIDGET_BG;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.extreme_bg_color = INPUT_BG;
    v.override_text_color = Some(TEXT_UI);
    v.selection.bg_fill = SELECTION;
    v.selection.stroke = Stroke::new(1.0, TEXT_BRIGHT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, hex(0x2b2b2b));
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, FOCUS_BORDER);
    v.window_corner_radius = 6.into();
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(0.0, 0.0);
        s.interaction.selectable_labels = false;
    });

    // Use a system UI font if one is installed; egui's bundled fonts are the fallback.
    let ui_fonts = [
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
