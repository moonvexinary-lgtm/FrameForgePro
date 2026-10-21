//! Embeds FrameForge Pro's icon and version info into the Windows executable
//! resource (Explorer's icon for `frameforge-pro.exe`). Pure Rust, no external
//! resource compiler needed.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/frameforgepro-icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set("FileDescription", "FrameForge Pro");
        resource.set("ProductName", "FrameForge Pro");
        resource.set_icon("../../assets/frameforgepro-icon.ico");
        if let Err(e) = resource.compile() {
            println!("cargo::warning=could not embed the Windows icon: {e}");
        }
    }
}
