//! pm (Plus Minus) — a diff-oriented code viewer.
//!
//! Opens a folder (arg 1, or the current directory), finds the enclosing git
//! repository, and shows changed files with a side-by-side line diff. All state
//! lives in `pm_core`; all rendering in `pm_ui`. This file is just wiring.

// Release builds are a GUI app with no console window; debug builds keep the
// console so `eprintln!` / panics are visible during development.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod fonts;

use std::path::PathBuf;

use gpui::{prelude::*, px, size, App, Bounds, WindowBounds, WindowOptions};
use gpui_platform::application;
use pm_core::Repo;
use pm_ui::Pm;

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .expect("could not determine a folder to open");

    let repo = match Repo::discover(&path) {
        Ok(repo) => repo,
        Err(err) => {
            eprintln!("pm: {} is not inside a git repository ({err})", path.display());
            std::process::exit(1);
        }
    };
    eprintln!("pm: opened {}", repo.root().display());

    let icon = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()
        .map(|img| std::sync::Arc::new(img.into_rgba8()));

    application().run(move |cx: &mut App| {
        fonts::load(cx);
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                icon: icon.clone(),
                ..Default::default()
            },
            |_window, cx| {
                cx.new(|cx| {
                    let mut pm = Pm::new(repo, cx);
                    pm.start_watch(cx);
                    pm
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
