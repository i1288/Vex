//! Colors and fonts. Built-in themes follow VS Code's "Dark Modern" and
//! "Light Modern" (the defaults), the classic "Dark+" / "Light+", and popular
//! community palettes. Users can add theme files and override single colors.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Stroke};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

#[derive(Clone)]
pub struct Palette {
    /// Name in settings.toml and `:colorscheme`.
    pub name: Cow<'static, str>,
    /// Name shown in the settings page.
    pub title: Cow<'static, str>,
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
    name: Cow::Borrowed("dark"),
    title: Cow::Borrowed("Dark Modern"),
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
    name: Cow::Borrowed("light"),
    title: Cow::Borrowed("Light Modern"),
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
    name: Cow::Borrowed("dark-plus"),
    title: Cow::Borrowed("Dark+ (classic)"),
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
    name: Cow::Borrowed("light-plus"),
    title: Cow::Borrowed("Light+ (classic)"),
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

/// Settings-page labels for every color a theme sets, keyed by field name
/// (the names used in theme files and `[colors]`).
macro_rules! color_fields {
    ($($field:ident: $label:literal),* $(,)?) => {
        pub const COLOR_FIELDS: &[(&str, &str)] = &[$((stringify!($field), $label)),*];

        impl Palette {
            pub fn color(&self, key: &str) -> Option<Color32> {
                match key {
                    $(stringify!($field) => Some(self.$field),)*
                    _ => None,
                }
            }

            pub fn color_mut(&mut self, key: &str) -> Option<&mut Color32> {
                match key {
                    $(stringify!($field) => Some(&mut self.$field),)*
                    _ => None,
                }
            }
        }
    };
}

color_fields! {
    editor_bg: "Editor background",
    text: "Editor text",
    selection: "Selection",
    current_line: "Current line",
    cursor: "Cursor",
    line_number: "Line numbers",
    line_number_active: "Current line number",
    indent_guide: "Indent guides",
    search_match: "Search matches",
    search_border: "Search match border",
    keyword: "Keywords",
    control: "Control flow (if, return…)",
    type_: "Types",
    function: "Functions",
    string: "Strings",
    number: "Numbers",
    comment: "Comments",
    constant: "Constants",
    sidebar_bg: "Sidebar background",
    activity_bg: "Activity bar background",
    activity_fg: "Activity bar icons",
    icon_inactive: "Inactive icons",
    tabs_bg: "Tab bar background",
    tab_inactive: "Inactive tab",
    status_bg: "Status bar background",
    status_fg: "Status bar text",
    panel_bg: "Terminal panel background",
    accent: "Accent",
    separator: "Separators",
    border: "Popup and input borders",
    focus_border: "Focus border",
    widget_bg: "Popup background",
    input_bg: "Input background",
    text_ui: "UI text",
    text_muted: "Muted text",
    text_bright: "Bright text",
    list_hover: "List hover",
    list_active: "Active list item",
    list_selected: "Selected list item",
    tree_guide: "Tree guides",
    scrollbar: "Scrollbar",
}

/// The few colors a built-in community theme is defined by; everything else
/// is derived from them.
struct Spec {
    name: &'static str,
    title: &'static str,
    dark: bool,
    bg: u32,
    surface: u32,
    fg: u32,
    muted: u32,
    accent: u32,
    selection: u32,
    line: u32,
    border: u32,
    cursor: u32,
    /// keyword, control, type, function, string, number, comment, constant
    syntax: [u32; 8],
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn derive(s: &Spec) -> Palette {
    let (bg, fg, surface) = (hex(s.bg), hex(s.fg), hex(s.surface));
    let extreme = if s.dark { Color32::WHITE } else { Color32::BLACK };
    let base = if s.dark { &DARK_MODERN } else { &LIGHT_MODERN };
    let [keyword, control, type_, function, string, number, comment, constant] = s.syntax.map(hex);
    Palette {
        name: Cow::Borrowed(s.name),
        title: Cow::Borrowed(s.title),
        dark: s.dark,
        editor_bg: bg,
        sidebar_bg: surface,
        activity_bg: surface,
        tabs_bg: surface,
        tab_inactive: surface,
        status_bg: surface,
        panel_bg: surface,
        widget_bg: mix(surface, fg, 0.04),
        input_bg: mix(bg, fg, 0.08),
        border: mix(bg, fg, 0.2),
        focus_border: hex(s.accent),
        separator: hex(s.border),
        accent: hex(s.accent),
        text: fg,
        text_ui: fg,
        text_muted: hex(s.muted),
        text_bright: mix(fg, extreme, 0.5),
        status_fg: fg,
        activity_fg: mix(fg, extreme, 0.3),
        icon_inactive: hex(s.muted),
        line_number: mix(hex(s.muted), bg, 0.15),
        line_number_active: fg,
        current_line: hex(s.line),
        selection: hex(s.selection),
        search_match: Color32::from_rgba_unmultiplied(string.r(), string.g(), string.b(), 0x50),
        search_border: string,
        cursor: hex(s.cursor),
        indent_guide: mix(bg, fg, 0.12),
        list_hover: mix(surface, fg, 0.06),
        list_active: mix(surface, fg, 0.12),
        list_selected: hex(s.selection),
        tree_guide: mix(surface, fg, 0.15),
        scrollbar: hex(s.muted),
        keyword,
        control,
        type_,
        function,
        string,
        number,
        comment,
        constant,
        ansi: base.ansi,
    }
}

const COMMUNITY: &[Spec] = &[
    Spec { name: "monokai", title: "Monokai", dark: true, bg: 0x272822, surface: 0x1e1f1c, fg: 0xf8f8f2, muted: 0x75715e,
        accent: 0x66d9ef, selection: 0x49483e, line: 0x3e3d32, border: 0x171814, cursor: 0xf8f8f0,
        syntax: [0xf92672, 0xf92672, 0x66d9ef, 0xa6e22e, 0xe6db74, 0xae81ff, 0x75715e, 0xae81ff] },
    Spec { name: "dracula", title: "Dracula", dark: true, bg: 0x282a36, surface: 0x21222c, fg: 0xf8f8f2, muted: 0x6272a4,
        accent: 0xbd93f9, selection: 0x44475a, line: 0x343746, border: 0x191a21, cursor: 0xf8f8f2,
        syntax: [0xff79c6, 0xff79c6, 0x8be9fd, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0x6272a4, 0xbd93f9] },
    Spec { name: "one-dark", title: "One Dark", dark: true, bg: 0x282c34, surface: 0x21252b, fg: 0xabb2bf, muted: 0x5c6370,
        accent: 0x528bff, selection: 0x3e4451, line: 0x2c313c, border: 0x181a1f, cursor: 0x528bff,
        syntax: [0xc678dd, 0xc678dd, 0xe5c07b, 0x61afef, 0x98c379, 0xd19a66, 0x5c6370, 0xd19a66] },
    Spec { name: "nord", title: "Nord", dark: true, bg: 0x2e3440, surface: 0x2b313c, fg: 0xd8dee9, muted: 0x616e88,
        accent: 0x88c0d0, selection: 0x434c5e, line: 0x3b4252, border: 0x3b4252, cursor: 0xd8dee9,
        syntax: [0x81a1c1, 0x81a1c1, 0x8fbcbb, 0x88c0d0, 0xa3be8c, 0xb48ead, 0x616e88, 0x81a1c1] },
    Spec { name: "gruvbox-dark", title: "Gruvbox Dark", dark: true, bg: 0x282828, surface: 0x1d2021, fg: 0xebdbb2, muted: 0x928374,
        accent: 0x83a598, selection: 0x504945, line: 0x3c3836, border: 0x3c3836, cursor: 0xebdbb2,
        syntax: [0xfb4934, 0xfb4934, 0xfabd2f, 0x8ec07c, 0xb8bb26, 0xd3869b, 0x928374, 0xd3869b] },
    Spec { name: "tokyo-night", title: "Tokyo Night", dark: true, bg: 0x1a1b26, surface: 0x16161e, fg: 0xa9b1d6, muted: 0x565f89,
        accent: 0x7aa2f7, selection: 0x283457, line: 0x1f2335, border: 0x101014, cursor: 0xc0caf5,
        syntax: [0xbb9af7, 0xbb9af7, 0x2ac3de, 0x7aa2f7, 0x9ece6a, 0xff9e64, 0x565f89, 0xff9e64] },
    Spec { name: "catppuccin-mocha", title: "Catppuccin Mocha", dark: true, bg: 0x1e1e2e, surface: 0x181825, fg: 0xcdd6f4, muted: 0x7f849c,
        accent: 0xcba6f7, selection: 0x45475a, line: 0x313244, border: 0x11111b, cursor: 0xf5e0dc,
        syntax: [0xcba6f7, 0xcba6f7, 0xf9e2af, 0x89b4fa, 0xa6e3a1, 0xfab387, 0x7f849c, 0xfab387] },
    Spec { name: "solarized-dark", title: "Solarized Dark", dark: true, bg: 0x002b36, surface: 0x00212b, fg: 0x93a1a1, muted: 0x586e75,
        accent: 0x268bd2, selection: 0x274642, line: 0x073642, border: 0x003847, cursor: 0x93a1a1,
        syntax: [0x859900, 0x859900, 0xb58900, 0x268bd2, 0x2aa198, 0xd33682, 0x586e75, 0xcb4b16] },
    Spec { name: "solarized-light", title: "Solarized Light", dark: false, bg: 0xfdf6e3, surface: 0xeee8d5, fg: 0x586e75, muted: 0x93a1a1,
        accent: 0x268bd2, selection: 0xddd6c1, line: 0xeee8d5, border: 0xddd6c1, cursor: 0x586e75,
        syntax: [0x859900, 0x859900, 0xb58900, 0x268bd2, 0x2aa198, 0xd33682, 0x93a1a1, 0xcb4b16] },
    Spec { name: "github-light", title: "GitHub Light", dark: false, bg: 0xffffff, surface: 0xf6f8fa, fg: 0x24292f, muted: 0x6e7781,
        accent: 0x0969da, selection: 0xb6e3ff, line: 0xf6f8fa, border: 0xd0d7de, cursor: 0x0969da,
        syntax: [0xcf222e, 0xcf222e, 0x953800, 0x8250df, 0x0a3069, 0x0550ae, 0x6e7781, 0x0550ae] },
];

/// All themes: built-in ones first, then the user's theme files.
static REGISTRY: LazyLock<RwLock<Vec<Arc<Palette>>>> = LazyLock::new(|| RwLock::new(builtins()));
static CURRENT: LazyLock<RwLock<Arc<Palette>>> = LazyLock::new(|| RwLock::new(Arc::new(DARK_MODERN.clone())));

fn builtins() -> Vec<Arc<Palette>> {
    let mut v: Vec<Arc<Palette>> =
        [&DARK_MODERN, &LIGHT_MODERN, &DARK_PLUS, &LIGHT_PLUS].into_iter().map(|p| Arc::new(p.clone())).collect();
    v.extend(COMMUNITY.iter().map(|s| Arc::new(derive(s))));
    v
}

/// The active palette.
pub fn p() -> Arc<Palette> {
    CURRENT.read().unwrap().clone()
}

pub fn all() -> Vec<Arc<Palette>> {
    REGISTRY.read().unwrap().clone()
}

/// Find a theme by name (case-insensitive, with a few aliases).
pub fn find(name: &str) -> Option<Arc<Palette>> {
    let name = name.trim().to_lowercase();
    let name = match name.as_str() {
        "dark-modern" => "dark",
        "light-modern" => "light",
        "dark+" => "dark-plus",
        "light+" => "light-plus",
        other => other,
    };
    REGISTRY.read().unwrap().iter().find(|p| p.name == name).cloned()
}

/// `#rrggbb` or `#rrggbbaa`.
pub fn parse_color(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

pub fn to_hex(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// Apply `overrides` (field name → color) to `p`, returning problems found.
fn apply_overrides(p: &mut Palette, overrides: &BTreeMap<String, String>, source: &str) -> Vec<String> {
    let mut errors = Vec::new();
    for (key, value) in overrides {
        match (p.color_mut(key), parse_color(value)) {
            (Some(slot), Some(c)) => *slot = c,
            (None, _) => errors.push(format!("{source}: unknown color \"{key}\"")),
            (_, None) => errors.push(format!("{source}: \"{value}\" is not a #rrggbb color")),
        }
    }
    errors
}

/// Make `name` (plus color overrides) the active theme. An unknown name
/// falls back to the default theme, with a warning.
pub fn activate(name: &str, overrides: &BTreeMap<String, String>) -> Vec<String> {
    let (palette, warnings) = build(name, overrides);
    *CURRENT.write().unwrap() = palette;
    warnings
}

/// The palette `activate` would install.
fn build(name: &str, overrides: &BTreeMap<String, String>) -> (Arc<Palette>, Vec<String>) {
    let mut warnings = Vec::new();
    let base = find(name).unwrap_or_else(|| {
        warnings.push(format!("unknown theme \"{name}\", using Dark Modern"));
        Arc::new(DARK_MODERN.clone())
    });
    let palette = if overrides.is_empty() {
        base
    } else {
        let mut p = (*base).clone();
        warnings.extend(apply_overrides(&mut p, overrides, "[colors]"));
        Arc::new(p)
    };
    (palette, warnings)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: Option<String>,
    title: Option<String>,
    base: Option<String>,
    #[serde(default)]
    colors: BTreeMap<String, String>,
}

/// Load `*.toml` theme files from `dir` into the registry (replacing ones
/// loaded before). Each file starts from a `base` theme and overrides colors:
///
/// ```toml
/// title = "My Theme"
/// base = "dracula"
/// [colors]
/// editor_bg = "#101018"
/// keyword = "#ff6ac1"
/// ```
pub fn load_theme_files(dir: &Path) -> Vec<String> {
    let mut themes = Vec::new();
    let mut errors = Vec::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect())
        .unwrap_or_default();
    paths.sort();
    for path in paths {
        let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| toml::from_str::<ThemeFile>(&t).map_err(|e| e.message().to_string()));
        let file = match parsed {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("themes/{file_name}: {e}"));
                continue;
            }
        };
        let base_name = file.base.as_deref().unwrap_or("dark");
        let Some(base) = builtins().into_iter().find(|p| p.name == base_name).or_else(|| find(base_name)) else {
            errors.push(format!("themes/{file_name}: unknown base theme \"{base_name}\""));
            continue;
        };
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let mut p = (*base).clone();
        p.name = Cow::Owned(file.name.map(|n| n.to_lowercase()).unwrap_or(stem));
        p.title = Cow::Owned(file.title.unwrap_or_else(|| p.name.to_string()));
        errors.extend(apply_overrides(&mut p, &file.colors, &format!("themes/{file_name}")));
        themes.push(Arc::new(p));
    }
    let mut reg = builtins();
    reg.retain(|b| !themes.iter().any(|t| t.name == b.name));
    reg.extend(themes);
    *REGISTRY.write().unwrap() = reg;
    errors
}

/// Write the active theme as a theme file the user can edit.
pub fn export_current(dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let p = p();
    let mut out = format!("# Vex theme. Edit any color, save, and pick it in Settings.\ntitle = \"{name}\"\nbase = \"{}\"\n\n[colors]\n", if p.dark { "dark" } else { "light" });
    for (key, _) in COLOR_FIELDS {
        if let Some(c) = p.color(key) {
            out.push_str(&format!("{key} = \"{}\"\n", to_hex(c)));
        }
    }
    std::fs::create_dir_all(dir)?;
    let slug: String = name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let path = dir.join(format!("{slug}.toml"));
    std::fs::write(&path, out)?;
    Ok(path)
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
    let _ = install_fonts(ctx, None);
}

const FONT_DIRS: &[&str] = &[
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "~/.local/share/fonts",
    "~/.fonts",
    "/System/Library/Fonts",
    "/Library/Fonts",
    "~/Library/Fonts",
    "C:\\Windows\\Fonts",
];

/// Set up fonts: a system UI font if one is installed (egui's bundled fonts
/// are the fallback), and `editor_font` (a .ttf/.otf path) for code.
pub fn install_fonts(ctx: &egui::Context, editor_font: Option<&Path>) -> Result<(), String> {
    let mut defs = FontDefinitions::default();
    let ui_fonts = [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    if let Some(data) = ui_fonts.iter().find_map(|p| std::fs::read(p).ok()) {
        defs.font_data.insert("ui".into(), Arc::new(FontData::from_owned(data)));
        defs.families.entry(FontFamily::Proportional).or_default().insert(0, "ui".into());
    }
    let mut result = Ok(());
    if let Some(path) = editor_font {
        match std::fs::read(path) {
            Ok(data) => {
                defs.font_data.insert("editor".into(), Arc::new(FontData::from_owned(data)));
                defs.families.entry(FontFamily::Monospace).or_default().insert(0, "editor".into());
            }
            Err(e) => result = Err(format!("cannot load font {}: {e}", path.display())),
        }
    }
    ctx.set_fonts(defs);
    result
}

fn expand_home(p: &str) -> Option<PathBuf> {
    match p.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rest)),
        None => Some(PathBuf::from(p)),
    }
}

/// Monospace fonts installed on the system (regular weights), by name.
pub fn find_mono_fonts() -> Vec<(String, PathBuf)> {
    const MONO_HINTS: &[&str] = &["mono", "code", "consol", "courier", "menlo", "hack", "iosevka", "inconsolata", "cascadia", "fira"];
    const STYLES: &[&str] = &["bold", "italic", "oblique", "light", "thin", "medium", "semi", "extra", "black", "heavy", "condensed", "narrow", "retina"];
    let mut found = BTreeMap::new();
    let mut stack: Vec<(PathBuf, u8)> = FONT_DIRS.iter().filter_map(|d| expand_home(d)).map(|d| (d, 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if depth < 5 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext != "ttf" && ext != "otf" {
                continue;
            }
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let lower = stem.to_lowercase();
            if !MONO_HINTS.iter().any(|h| lower.contains(h)) || STYLES.iter().any(|s| lower.contains(s)) {
                continue;
            }
            let name = stem.trim_end_matches("-Regular").trim_end_matches("-R").replace(['-', '_'], " ");
            found.entry(name).or_insert(path);
        }
    }
    found.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse_and_print() {
        assert_eq!(parse_color("#ff8000"), Some(Color32::from_rgb(255, 128, 0)));
        assert_eq!(to_hex(parse_color("#12abef").unwrap()), "#12abef");
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn every_builtin_has_a_unique_name_and_full_palette() {
        let all = builtins();
        assert!(all.len() >= 14);
        for (i, p) in all.iter().enumerate() {
            assert!(all.iter().skip(i + 1).all(|q| q.name != p.name), "duplicate {}", p.name);
            for (key, _) in COLOR_FIELDS {
                assert!(p.color(key).is_some(), "{} lacks {key}", p.name);
            }
        }
        assert!(find("Dracula").is_some_and(|p| p.dark));
        assert!(find("solarized-light").is_some_and(|p| !p.dark));
        assert!(find("dark+").is_some());
    }

    #[test]
    fn theme_files_and_overrides() {
        let dir = std::env::temp_dir().join(format!("vex-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mine.toml"), "title = \"Mine\"\nbase = \"nord\"\n[colors]\nkeyword = \"#ff0000\"\n").unwrap();
        std::fs::write(dir.join("bad.toml"), "base = \"nope\"\n").unwrap();
        let errors = load_theme_files(&dir);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let mine = find("mine").unwrap();
        assert_eq!(mine.title, "Mine");
        assert_eq!(mine.keyword, Color32::RED);
        assert_eq!(mine.editor_bg, find("nord").unwrap().editor_bg);

        let mut overrides = BTreeMap::new();
        overrides.insert("editor_bg".to_string(), "#000000".to_string());
        overrides.insert("nope".to_string(), "#000000".to_string());
        // `build` rather than `activate`: other tests may switch the global theme.
        let (built, warnings) = build("mine", &overrides);
        assert_eq!(warnings.len(), 1);
        assert_eq!(built.editor_bg, Color32::BLACK);
        assert_eq!(built.keyword, Color32::RED);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
