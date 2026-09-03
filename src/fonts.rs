//! Fonts vendored into the binary.
//!
//! System font discovery is unreliable across platforms (notably bare Linux,
//! where neither "Segoe UI" nor "Consolas" exist), so we ship the two families
//! the UI names — Roboto for chrome, JetBrains Mono for diff text — and register
//! them with gpui's text system at startup. Both are SIL OFL 1.1; the license
//! texts live next to the `.ttf` files in `assets/fonts`.

use std::borrow::Cow;

use gpui::App;

/// Every face we embed. `include_bytes!` keeps them in the executable's rodata.
const FACES: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Roboto-Regular.ttf"),
    include_bytes!("../assets/fonts/Roboto-Medium.ttf"),
    include_bytes!("../assets/fonts/Roboto-Bold.ttf"),
    include_bytes!("../assets/fonts/Roboto-Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
];

/// Register the vendored faces. Call once, inside `App::run`.
pub fn load(cx: &App) {
    if let Err(err) = cx
        .text_system()
        .add_fonts(FACES.iter().map(|b| Cow::Borrowed(*b)).collect())
    {
        eprintln!("pm: could not register vendored fonts ({err})");
    }
}
