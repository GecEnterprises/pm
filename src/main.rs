//! The `pm` binary — the installed release preview you keep open. All wiring
//! lives in the `pm` library crate; this is just the entry point (PM-88).

// Release builds are a GUI app with no console window; debug builds keep the
// console so `eprintln!` / panics are visible during development.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    pm::run(pm::RELEASE);
}
