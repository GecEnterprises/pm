//! Application actions and the menu-bar model.
//!
//! `app_menus()` feeds `cx.set_menus` (a real menu bar on macOS); the same
//! structure is mirrored in-window by the custom title bar on Windows/Linux via
//! [`menu_groups`], which can reflect live `Pm` state (e.g. panel checkmarks).

use gpui::{actions, Action, Menu, MenuItem};

use crate::app::Pm;

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
        /// Show or hide the Explorer panel.
        ToggleExplorer,
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
            MenuItem::action("Changes Panel", ToggleChanges),
            MenuItem::action("Explorer Panel", ToggleExplorer),
        ]),
        Menu::new("Help").items([MenuItem::action("About pm", About)]),
    ]
}

/// One row in a dropdown.
pub enum Entry {
    Separator,
    Item {
        label: &'static str,
        shortcut: Option<&'static str>,
        action: Box<dyn Action>,
        checked: bool,
    },
}

fn item(label: &'static str, shortcut: Option<&'static str>, action: impl Action) -> Entry {
    Entry::Item {
        label,
        shortcut,
        action: Box::new(action),
        checked: false,
    }
}

fn checkable(label: &'static str, action: impl Action, checked: bool) -> Entry {
    Entry::Item {
        label,
        shortcut: None,
        action: Box::new(action),
        checked,
    }
}

pub struct Group {
    pub name: &'static str,
    pub entries: Vec<Entry>,
}

/// The in-window menu bar, with checkmarks resolved against `pm`.
pub fn menu_groups(pm: &Pm) -> Vec<Group> {
    vec![
        Group {
            name: "File",
            entries: vec![
                item("Open Folder…", Some("Ctrl+O"), OpenFolder),
                Entry::Separator,
                item("Refresh", Some("Ctrl+R"), Refresh),
                Entry::Separator,
                item("Quit", Some("Ctrl+Q"), Quit),
            ],
        },
        Group {
            name: "Edit",
            entries: vec![item("Copy", None, Copy), item("Select All", None, SelectAll)],
        },
        Group {
            name: "View",
            entries: vec![
                checkable("Changes Panel", ToggleChanges, !pm.changes_collapsed),
                checkable("Explorer Panel", ToggleExplorer, !pm.explorer_collapsed),
            ],
        },
        Group {
            name: "Help",
            entries: vec![item("About pm", None, About)],
        },
    ]
}
