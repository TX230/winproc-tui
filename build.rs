fn main() -> std::io::Result<()> {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".to_string());
        let mut resource = winresource::WindowsResource::new();
        resource
            .set("FileDescription", "winproc-tui")
            .set("FileVersion", &version)
            .set("ProductName", "winproc-tui")
            .set("ProductVersion", &version)
            .set("LegalCopyright", "© 2026 TX230")
            .set("OriginalFilename", "winproc-tui.exe")
            .set_language(0x0409);
        resource.compile()?;
    }

    Ok(())
}
