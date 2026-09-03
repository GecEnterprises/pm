pub mod app;
pub mod decorations;
pub mod diff_view;
pub mod history_view;
pub mod icons;
pub mod image_view;
pub mod list_view;
pub mod menu;
pub mod scroll;
pub mod status_bar;
pub mod text_input;
pub mod theme;
pub mod tickets_view;
pub mod title_bar;
pub mod tree_view;

pub use app::Pm;
pub use menu::{
    app_menus, About, Copy, OpenFolder, Quit, Refresh, SelectAll, ToggleChanges, ToggleExplorer,
    ToggleHistory, ViewFiles, ViewSummary, ViewTickets,
};
