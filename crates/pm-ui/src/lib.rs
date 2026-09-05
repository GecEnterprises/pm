pub mod app;
pub mod config;
pub mod diff_view;
pub mod history_view;
pub mod icons;
pub mod image_view;
pub mod list_view;
pub mod menu;
pub mod status_bar;
pub mod theme;

pub use fremantle::text_input;
pub mod tickets_view;
pub mod title_bar;
pub mod tree_view;
pub mod update;

pub use app::{set_app_label, Pm};
pub use config::ConfigStore;
pub use update::UpdateStatus;
pub use text_input::{TextInput, TextInputEvent};
pub use menu::{
    app_menus, About, Copy, FindTickets, NextView, OpenFolder, PrevView, Quit, Refresh, SelectAll,
    ToggleChanges, ToggleExplorer, ToggleHistory, ToggleWatchJump, ViewFiles, ViewSummary,
    ViewTickets, ZoomIn, ZoomOut, ZoomReset,
};
