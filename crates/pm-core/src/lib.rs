pub mod content;
pub mod diff;
pub mod git;
pub mod highlight;
pub mod state;
pub mod text;
pub mod watch;

pub use git::{CommitInfo, DiffTarget, Repo};
pub use git2::Oid;
pub use state::{AppState, MAX_ROWS};
