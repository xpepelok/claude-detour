fn main() {
    #[cfg(windows)]
    {
        let icon = "assets/icon.ico";
        println!("cargo:rerun-if-changed={icon}");
        if std::path::Path::new(icon).exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon(icon);
            if let Err(e) = res.compile() {
                println!("cargo:warning=embedding icon failed: {e}");
            }
        }
    }
}
