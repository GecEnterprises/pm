//! pm (Plus Minus) — a diff-oriented code viewer.
//!
//! Opens a folder (arg 1; `pm` with no arg opens with no project), finds the
//! enclosing git repository, and shows changed files with a side-by-side line
//! diff. All state lives in `pm_core`; all rendering in `pm_ui`.
//!
//! The two binaries (`pm`, `pm-debug`) are thin `main`s that call [`run`] with a
//! different [`Variant`] — see PM-88. Everything else lives here.

pub mod cli;
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
use pm_ui::{
    FindTickets, NextView, OpenFolder, Pm, PrevView, Quit, ViewFiles, ViewSummary, ViewTickets,
    ZoomIn, ZoomOut, ZoomReset,
};

/// Which build this is. The two binaries differ only in these few values;
/// everything they thread through — window title, help/version text, the OS app
/// identity, the taskbar icon — comes from here (PM-88).
#[derive(Clone, Copy)]
pub struct Variant {
    /// Program name in `--help` / `--version` and the window-title suffix.
    pub label: &'static str,
    /// Linux window class (`_NET_WM_CLASS`) and Windows AppUserModelID, so the
    /// two variants group separately in the taskbar / dock.
    pub app_id: &'static str,
    /// PNG for the taskbar / window icon — a distinct hue per variant so the
    /// debug build is obvious at a glance (`assets/icon*.png`).
    pub icon_png: &'static [u8],
}

/// The installed build you keep open. Bare `cargo run` opens this one.
pub const RELEASE: Variant = Variant {
    label: "pm",
    app_id: "com.gecenterprises.pm",
    icon_png: include_bytes!("../assets/icon.png"),
};

/// The throwaway build for development, runnable beside [`RELEASE`].
pub const DEBUG: Variant = Variant {
    label: "pm-debug",
    app_id: "com.gecenterprises.pm-debug",
    icon_png: include_bytes!("../assets/icon-debug.png"),
};

/// Loaded once at startup so every window (including ones opened via
/// File → Open Folder) gets the same taskbar icon.
static ICON: OnceLock<Option<Arc<RgbaImage>>> = OnceLock::new();

/// Entry point shared by both binaries.
pub fn run(variant: Variant) {
    pm_core::update::cleanup_stale();

    // `None` → open the welcome window; `Some(path)` → open a repo window.
    let start: Option<PathBuf> = match cli::parse() {
        cli::Command::Gui { path } => path,
        cli::Command::Mcp { project } => {
            attach_console();
            let root = project
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            return exit_on_err("--mcp", pm_mcp::serve_stdio(root));
        }
        cli::Command::Setup { assume_yes } => {
            attach_console();
            return exit_on_err("--setup", pm_core::setup::run(assume_yes));
        }
        cli::Command::Uninstall { assume_yes } => {
            attach_console();
            return exit_on_err("--uninstall", pm_core::setup::uninstall(assume_yes));
        }
        cli::Command::Update => {
            attach_console();
            return exit_on_err("--update", pm_core::update::run_self_update());
        }
        cli::Command::Version => {
            attach_console();
            println!("{}", pm_core::buildinfo::long_version_named(variant.label));
            return;
        }
        cli::Command::Help => {
            attach_console();
            print!("{}", cli::usage(variant.label));
            return;
        }
    };

    let icon = image::load_from_memory(variant.icon_png)
        .ok()
        .map(|img| Arc::new(img.into_rgba8()));

    application().run(move |cx: &mut App| {
        cx.set_app_identity(variant.app_id, variant.label);
        let _ = ICON.set(icon.clone());
        fonts::load(cx);
        pm_ui::ConfigStore::init(cx);
        pm_ui::set_app_label(variant.label);

        cx.set_menus(pm_ui::app_menus());
        cx.bind_keys([
            KeyBinding::new("ctrl-o", OpenFolder, None),
            KeyBinding::new("ctrl-r", pm_ui::Refresh, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-1", ViewSummary, None),
            KeyBinding::new("ctrl-2", ViewFiles, None),
            KeyBinding::new("ctrl-3", ViewTickets, None),
            KeyBinding::new("ctrl-f", FindTickets, None),
            KeyBinding::new("ctrl-tab", NextView, None),
            KeyBinding::new("ctrl-shift-tab", PrevView, None),
            KeyBinding::new("ctrl-=", ZoomIn, None),
            KeyBinding::new("ctrl-+", ZoomIn, None),
            KeyBinding::new("ctrl--", ZoomOut, None),
            KeyBinding::new("ctrl-0", ZoomReset, None),
        ]);
        bind_text_input_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(move |_: &OpenFolder, cx| {
            let paths = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Open".into()),
            });
            cx.spawn(async move |cx| {
                if let Ok(Ok(Some(mut dirs))) = paths.await {
                    if let Some(dir) = dirs.pop() {
                        cx.update(|cx| open_pm_window(cx, Some(&dir), variant));
                    }
                }
            })
            .detach();
        });

        pm_ui::UpdateStatus::init(cx);
        open_pm_window(cx, start.as_deref(), variant);
        cx.activate(true);
    });
}

/// Run `r`, printing `<what>: <err>` and exiting non-zero on failure.
fn exit_on_err(what: &str, r: anyhow::Result<()>) {
    if let Err(e) = r {
        eprintln!("{what}: {e}");
        std::process::exit(1);
    }
}

/// On Windows the release build has no console (`windows_subsystem = "windows"`),
/// so CLI subcommands attach to the parent shell's console for their output.
#[cfg(windows)]
fn attach_console() {
    // SAFETY: FFI with no pointers; failure (no parent console) is ignored.
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}
#[cfg(not(windows))]
fn attach_console() {}

/// Keybindings for the editable text fields (Tickets pane). Scoped to the
/// `TextInput` key contexts so they never reach the diff view or menus.
fn bind_text_input_keys(cx: &mut gpui::App) {
    use pm_ui::text_input::*;
    let ti = Some("TextInput");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, ti),
        KeyBinding::new("delete", Delete, ti),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, ti),
        KeyBinding::new("left", Left, ti),
        KeyBinding::new("right", Right, ti),
        KeyBinding::new("up", Up, ti),
        KeyBinding::new("down", Down, ti),
        KeyBinding::new("ctrl-left", WordLeft, ti),
        KeyBinding::new("ctrl-right", WordRight, ti),
        KeyBinding::new("home", Home, ti),
        KeyBinding::new("end", End, ti),
        KeyBinding::new("shift-left", SelectLeft, ti),
        KeyBinding::new("shift-right", SelectRight, ti),
        KeyBinding::new("shift-up", SelectUp, ti),
        KeyBinding::new("shift-down", SelectDown, ti),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, ti),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, ti),
        KeyBinding::new("shift-home", SelectHome, ti),
        KeyBinding::new("shift-end", SelectEnd, ti),
        KeyBinding::new("ctrl-a", SelectAll, ti),
        KeyBinding::new("ctrl-c", Copy, ti),
        KeyBinding::new("ctrl-x", Cut, ti),
        KeyBinding::new("ctrl-v", Paste, ti),
        KeyBinding::new("ctrl-z", Undo, ti),
        KeyBinding::new("ctrl-shift-z", Redo, ti),
        KeyBinding::new("ctrl-y", Redo, ti),
        KeyBinding::new("enter", Confirm, Some("TextInputSingleLine")),
        KeyBinding::new("enter", Newline, Some("TextInputMultiLine")),
    ]);
}

/// Open a pm window. `Some(path)` opens it as a git repo (or a plain folder if
/// it isn't one); `None` opens the "Nothing opened" placeholder (`pm` with no
/// argument — PM-5), from which the user picks a folder in place.
fn open_pm_window(cx: &mut App, path: Option<&Path>, variant: Variant) {
    let repo = path.map(Repo::open);
    let tag = variant.label;
    match &repo {
        Some(r) => {
            eprintln!(
                "{tag}: opened {}{}",
                r.root().display(),
                if r.is_git() { "" } else { " (not a git repository)" }
            );
            let root = r.root().to_path_buf();
            pm_ui::ConfigStore::update(cx, move |c| {
                c.push_recent(&root);
            });
        }
        None => eprintln!("{tag}: opened with no project"),
    }

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
            app_id: Some(variant.app_id.to_string()),
            icon,
            ..Default::default()
        },
        |window, cx| {
            let view = cx.new(|cx| match repo {
                Some(repo) => {
                    let mut pm = Pm::new(repo, cx);
                    pm.start_watch(cx);
                    pm
                }
                None => Pm::new_empty(cx),
            });
            let focus = view.read(cx).root_focus.clone();
            window.focus(&focus, cx);
            view
        },
    )
    .unwrap();
}
