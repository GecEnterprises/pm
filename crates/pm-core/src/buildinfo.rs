//! Compile-time build metadata (see `build.rs`). `pm --version` and the About
//! box read this; the update check compares [`VERSION`] against GitHub.

include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

/// `"pm 0.1.0 (abc1234, 2026-09-04)"`.
pub fn long_version() -> String {
    format!("pm {VERSION} ({COMMIT}, {DATE})")
}
