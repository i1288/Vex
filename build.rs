// Embed the app icon in the Windows executable. Set VEX_SKIP_WINRES=1 to
// skip it, e.g. for `cargo check --target ...-windows-...` from another OS.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-env-changed=VEX_SKIP_WINRES");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if windows && std::env::var_os("VEX_SKIP_WINRES").is_none() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Vex");
        res.set("FileDescription", "Vex code editor");
        res.compile().expect("failed to embed Windows resources");
    }
}
