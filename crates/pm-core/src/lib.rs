pub mod config;
pub mod content;
pub mod diff;
pub mod git;
pub mod highlight;
pub mod identity;
pub mod pm;
pub mod state;
pub mod text;
pub mod watch;

pub use config::Config;
pub use identity::resolve_author;
pub use git::{CommitInfo, DiffTarget, Repo};
pub use git2::Oid;
pub use pm::{Anchor, Comment, PmData, Priority, Status, Ticket};
pub use state::{AppState, MAX_ROWS};
