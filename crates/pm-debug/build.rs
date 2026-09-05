//! Embeds the blue `pm-debug` icon into `pm-debug.exe`. Separate from the root
//! package's build.rs because winresource compiles one icon resource per crate,
//! so the two variants must be different crates to carry different `.exe` icons
//! (PM-88).

fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=../../assets/icon-debug.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon-debug.ico");
        if let Err(err) = res.compile() {
            println!("cargo:warning=pm-debug icon not embedded: {err}");
        }
    }
}
