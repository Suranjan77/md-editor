fn main() {
    #[cfg(windows)]
    embed_windows_resources();
}

/// Embed the application icon and version metadata into the `.exe` so Explorer,
/// the taskbar and pinned shortcuts show the MD Editor icon instead of the
/// generic executable one. Iced only sets the icon of the *running* window;
/// the file icon has to come from a Win32 resource.
///
/// Resource compilation needs `rc.exe` from the Windows SDK. When that is
/// missing the build still succeeds — the executable just keeps the default
/// icon — so a bare toolchain can build the project.
#[cfg(windows)]
fn embed_windows_resources() {
    let icon = concat!(env!("CARGO_MANIFEST_DIR"), "/../md-editor.ico");
    println!("cargo:rerun-if-changed={icon}");
    println!("cargo:rerun-if-changed=build.rs");

    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon);
    res.set("ProductName", "MD Editor");
    res.set("FileDescription", "MD Editor");
    res.set("LegalCopyright", "MIT licensed");

    if let Err(e) = res.compile() {
        println!("cargo:warning=Could not embed Windows resources ({e}); building without an icon");
    }
}
