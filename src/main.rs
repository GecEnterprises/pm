//! pm (Plus Minus) — a diff-oriented code viewer.
//!
//! Opens a folder (arg 1, or the current directory), finds the enclosing git
//! repository, and shows changed files with a side-by-side line diff. All state
//! lives in `pm_core`; all rendering in `pm_ui`. This file is just wiring.

// Release builds are a GUI app with no console window; debug builds keep the
// console so `eprintln!` / panics are visible during development.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod fonts;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use gpui::{
    point, prelude::*, px, size, App, Bounds, KeyBinding, PathPromptOptions, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions,
};
use gpui_platform::application;
use image::RgbaImage;
use pm_core::Repo;
use pm_ui::{OpenFolder, Pm, Quit, ViewFiles, ViewSummary, ViewTickets};

/// Loaded once at startup so every window (including ones opened via
/// File → Open Folder) gets the same taskbar icon.
static ICON: OnceLock<Option<Arc<RgbaImage>>> = OnceLock::new();

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .expect("could not determine a folder to open");

    let icon = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()
        .map(|img| Arc::new(img.into_rgba8()));

    application().run(move |cx: &mut App| {
        let _ = ICON.set(icon.clone());
        fonts::load(cx);

        cx.set_menus(pm_ui::app_menus());
        cx.bind_keys([
            KeyBinding::new("ctrl-o", OpenFolder, None),
            KeyBinding::new("ctrl-r", pm_ui::Refresh, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-1", ViewSummary, None),
            KeyBinding::new("ctrl-2", ViewFiles, None),
            KeyBinding::new("ctrl-3", ViewTickets, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &OpenFolder, cx| {
            let paths = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Open".into()),
            });
            cx.spawn(async move |cx| {
                if let Ok(Ok(Some(mut dirs))) = paths.await {
                    if let Some(dir) = dirs.pop() {
                        cx.update(|cx| open_pm_window(cx, &dir));
                    }
                }
            })
            .detach();
        });

        open_pm_window(cx, &path);
        cx.activate(true);
    });
}

/// Open a pm window for `path` — as a git repo if it's inside one, otherwise as
/// a plain folder browser.
fn open_pm_window(cx: &mut App, path: &Path) {
    let repo = Repo::open(path);
    eprintln!(
        "pm: opened {}{}",
        repo.root().display(),
        if repo.is_git() { "" } else { " (not a git repository)" }
    );

    let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
    let icon = ICON.get().cloned().flatten();
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(point(px(9.0), px(9.0))),
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: WindowBackgroundAppearance::Opaque,
            window_min_size: Some(size(px(480.0), px(320.0))),
            icon,
            ..Default::default()
        },
        |window, cx| {
            let view = cx.new(|cx| {
                let mut pm = Pm::new(repo, cx);
                pm.start_watch(cx);
                pm
            });
            let focus = view.read(cx).root_focus.clone();
            window.focus(&focus, cx);
            view
        },
    )
    .unwrap();
}
