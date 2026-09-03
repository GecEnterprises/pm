fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(err) = res.compile() {
            // Missing rc.exe / llvm-rc shouldn't fail the build; the app just
            // falls back to the default executable icon.
            println!("cargo:warning=window icon not embedded: {err}");
        }
    }
}
