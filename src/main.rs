mod app;
mod buffer;
mod explorer;
mod settings;
mod snapshot;
mod syntax;
mod theme;
mod vim;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    // `vex [path]`: a folder becomes the workspace; a file opens inside its folder.
    let arg = std::env::args_os().nth(1).map(PathBuf::from);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (root, file) = match arg {
        Some(p) if p.is_dir() => (p, None),
        Some(p) => {
            let p = if p.is_absolute() { p } else { cwd.join(p) };
            let root = p.parent().map_or(cwd.clone(), |d| d.to_path_buf());
            (root, Some(p))
        }
        None => (cwd, None),
    };
    let root = root.canonicalize().unwrap_or(root);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Vex")
            .with_app_id("vex")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Vex",
        options,
        Box::new(move |cc| Ok(Box::new(app::VexApp::new(cc, root, file)))),
    )
}
