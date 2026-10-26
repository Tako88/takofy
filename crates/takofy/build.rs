//! The executable's own icon.
//!
//! The window icon is an `eframe` setting and is already compiled in; the file
//! Explorer and the taskbar draw for the executable is a Windows resource, and
//! only a resource compiler can write it. This is that step, and it does
//! nothing anywhere else — `CARGO_CFG_TARGET_OS` is the target's, not the
//! build machine's, so a cross-compile still gets the icon.

fn main() {
    let icon = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packaging/icons/takofy.ico"
    );
    println!("cargo:rerun-if-changed={icon}");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    winresource::WindowsResource::new()
        .set_icon(icon)
        .compile()
        .expect("compiling the application icon into the executable");
}
