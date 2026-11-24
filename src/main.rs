// Release builds on Windows are GUI apps: no console window behind the editor.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod buffer;
mod commands;
mod diagnostics;
mod explorer;
mod git;
mod icons;
mod keymap;
mod keys_panel;
mod lsp;
mod palette;
mod search;
mod session;
mod settings;
mod settings_page;
mod snapshot;
mod syntax;
mod terminal;
mod theme;
mod view;
mod vim;
mod workspace;

use std::path::PathBuf;

use eframe::egui;

/// The app icon, also drawn on the welcome screen.
pub const ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");

fn main() -> eframe::Result {
    // `vex [path]`: a folder becomes the workspace; a file opens inside its folder.
    let arg = std::env::args_os().nth(1).map(PathBuf::from);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (root, file) = match arg {
        Some(p) if p.is_dir() => (p, None),
        Some(p) => {
            let p = if p.is_absolute() { p } else { cwd.join(p) };
            let root = project_root(&p).unwrap_or_else(|| p.parent().map_or(cwd.clone(), |d| d.to_path_buf()));
            (root, Some(p))
        }
        None if launched_from_gui(&cwd) => {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
            (home.map_or(cwd, PathBuf::from), None)
        }
        None => (cwd, None),
    };
    let root = root.canonicalize().unwrap_or(root);

    let icon = eframe::icon_data::from_png_bytes(ICON_PNG).expect("bundled icon is a valid PNG");
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Vex")
        .with_app_id("vex")
        .with_icon(std::sync::Arc::new(icon))
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([480.0, 320.0]);
    // Vex's own title bar: on macOS it sits under the native window buttons;
    // elsewhere it replaces the system's title bar.
    if settings::load().map_or(true, |s| s.custom_title_bar) {
        viewport = if cfg!(target_os = "macos") {
            viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false)
        } else {
            viewport.with_decorations(false)
        };
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Vex",
        options,
        Box::new(move |cc| Ok(Box::new(app::VexApp::new(cc, root, file)))),
    )
}

/// Started from Finder, Explorer or a desktop menu rather than a shell: the
/// working folder is then `/` or the program's own folder.
fn launched_from_gui(cwd: &std::path::Path) -> bool {
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf()));
    cwd.parent().is_none() || exe_dir.as_deref() == Some(cwd)
}

/// The nearest folder above `file` that looks like a project root, so a file
/// opened on its own still gets its project's explorer and language server.
fn project_root(file: &std::path::Path) -> Option<PathBuf> {
    const MARKERS: &[&str] = &["Cargo.toml", "package.json", "pyproject.toml", "go.mod", ".git"];
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dir = file.parent();
    while let Some(d) = dir {
        if Some(d) == home.as_deref() {
            return None; // never treat the home folder as a project
        }
        if MARKERS.iter().any(|m| d.join(m).exists()) {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}
