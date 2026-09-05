//! Application actions and the menu-bar model.
//!
//! `app_menus()` feeds `cx.set_menus` (a real menu bar on macOS); the same
//! structure is mirrored in-window by the custom title bar on Windows/Linux via
//! [`menu_groups`], which can reflect live `Pm` state (e.g. panel checkmarks).

use std::rc::Rc;

use gpui::{actions, Action, App, Context, Menu, MenuItem, SharedString, Window};

use crate::app::Pm;
use crate::config::ConfigStore;

actions!(
    pm,
    [
        /// Pick a folder and open it in a new window.
        OpenFolder,
        /// Re-read git state and reload the open file.
        Refresh,
        /// Quit pm.
        Quit,
        /// Show the About dialog.
        About,
        /// Show or hide the Changes panel.
        ToggleChanges,
        /// Show or hide the Commit History panel.
        ToggleHistory,
        /// Show or hide the Explorer panel.
        ToggleExplorer,
        /// Toggle watch-jump mode (follow file changes into the diff — PM-30).
        ToggleWatchJump,
        /// Switch to the Summary view.
        ViewSummary,
        /// Switch to the File-to-File view.
        ViewFiles,
        /// Switch to the Tickets view.
        ViewTickets,
        /// Cycle to the next top-level view (Ctrl+Tab).
        NextView,
        /// Cycle to the previous top-level view (Ctrl+Shift+Tab).
        PrevView,
        /// Jump to the ticket search box (Ctrl+F in the Tickets view — PM-80).
        FindTickets,
        /// Increase the window scale.
        ZoomIn,
        /// Decrease the window scale.
        ZoomOut,
        /// Reset the window scale to 100%.
        ZoomReset,
        /// Copy the diff selection.
        Copy,
        /// Select the whole open side of the diff.
        SelectAll,
    ]
);

/// Native menu bar (macOS). Windows/Linux store this but render their own bar.
pub fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new("File").items([
            MenuItem::action("Open Folder…", OpenFolder),
            MenuItem::separator(),
            MenuItem::action("Refresh", Refresh),
            MenuItem::separator(),
            MenuItem::action("Quit", Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Copy", Copy),
            MenuItem::action("Select All", SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Summary", ViewSummary),
            MenuItem::action("File-to-File", ViewFiles),
            MenuItem::action("Tickets", ViewTickets),
            MenuItem::separator(),
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
            MenuItem::action("Reset Zoom", ZoomReset),
            MenuItem::separator(),
            MenuItem::action("Changes Panel", ToggleChanges),
            MenuItem::action("Commit History", ToggleHistory),
            MenuItem::action("Explorer Panel", ToggleExplorer),
            MenuItem::separator(),
            MenuItem::action("Watchjump", ToggleWatchJump),
        ]),
        Menu::new("Help").items([MenuItem::action("About pm", About)]),
    ]
}

/// A closure a menu row runs directly (for rows that can't be a static action —
/// e.g. "open this specific recent project").
pub type RunFn = Rc<dyn Fn(&mut Pm, &mut Window, &mut Context<Pm>)>;

/// One row in a dropdown.
pub enum Entry {
    Separator,
    /// A non-interactive caption (section label / "None").
    Header(SharedString),
    Item {
        label: SharedString,
        shortcut: Option<SharedString>,
        action: Box<dyn Action>,
        checked: bool,
    },
    /// A row that runs `run` on click instead of dispatching an action.
    Run { label: SharedString, run: RunFn },
}

fn item(label: impl Into<SharedString>, shortcut: Option<&'static str>, action: impl Action) -> Entry {
    Entry::Item {
        label: label.into(),
        shortcut: shortcut.map(SharedString::from),
        action: Box::new(action),
        checked: false,
    }
}

fn checkable(label: impl Into<SharedString>, action: impl Action, checked: bool) -> Entry {
    Entry::Item {
        label: label.into(),
        shortcut: None,
        action: Box::new(action),
        checked,
    }
}

pub struct Group {
    pub name: &'static str,
    pub entries: Vec<Entry>,
}

/// Rows for the "Open Recent Projects" section of the File menu (PM-48).
fn recent_entries(cx: &App) -> Vec<Entry> {
    let recent = ConfigStore::get(cx).recent;
    let mut out = vec![Entry::Header("Open Recent Projects".into())];
    if recent.is_empty() {
        out.push(Entry::Header("  (none)".into()));
        return out;
    }
    for path in &recent {
        let label: SharedString = format!("  {}", path.display()).into();
        let p = path.clone();
        out.push(Entry::Run {
            label,
            run: Rc::new(move |pm, _window, cx| pm.load_repo(p.clone(), cx)),
        });
    }
    out.push(Entry::Run {
        label: "  Clear Recent".into(),
        run: Rc::new(|_pm, _window, cx| {
            ConfigStore::update(cx, |c| c.recent.clear());
        }),
    });
    out
}

/// The in-window menu bar, with checkmarks resolved against `pm`.
pub fn menu_groups(pm: &Pm, cx: &App) -> Vec<Group> {
    let mut file = vec![
        item("Open Folder…", Some("Ctrl+O"), OpenFolder),
        Entry::Separator,
    ];
    file.extend(recent_entries(cx));
    file.extend([
        Entry::Separator,
        item("Refresh", Some("Ctrl+R"), Refresh),
        Entry::Separator,
        item("Quit", Some("Ctrl+Q"), Quit),
    ]);

    vec![
        Group { name: "File", entries: file },
        Group {
            name: "Edit",
            entries: vec![item("Copy", None, Copy), item("Select All", None, SelectAll)],
        },
        Group {
            name: "View",
            entries: vec![
                checkable("Summary", ViewSummary, pm.view == crate::app::View::Summary),
                checkable("File-to-File", ViewFiles, pm.view == crate::app::View::Files),
                checkable("Tickets", ViewTickets, pm.view == crate::app::View::Tickets),
                Entry::Separator,
                item("Zoom In", Some("Ctrl+="), ZoomIn),
                item("Zoom Out", Some("Ctrl+-"), ZoomOut),
                item("Reset Zoom", Some("Ctrl+0"), ZoomReset),
                Entry::Separator,
                checkable("Changes Panel", ToggleChanges, !pm.changes_collapsed),
                checkable("Commit History", ToggleHistory, !pm.history_collapsed),
                checkable("Explorer Panel", ToggleExplorer, !pm.explorer_collapsed),
                Entry::Separator,
                checkable("Watchjump", ToggleWatchJump, ConfigStore::get(cx).watchjump),
            ],
        },
        Group {
            name: "Help",
            entries: vec![item("About pm", None, About)],
        },
    ]
}
