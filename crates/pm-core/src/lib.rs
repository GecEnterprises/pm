pub mod diff;
pub mod git;
pub mod highlight;
pub mod state;
pub mod text;
pub mod watch;

pub use git::Repo;
pub use state::{AppState, MAX_ROWS};
