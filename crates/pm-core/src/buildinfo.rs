//! Compile-time build metadata (see `build.rs`). `pm --version` and the About
//! box read this; the update check compares [`VERSION`] against GitHub.

include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

/// `"pm 0.1.0 (abc1234, 2026-09-04)"`.
pub fn long_version() -> String {
    long_version_named("pm")
}

/// Like [`long_version`] but with the program name spelled out, so the
/// `pm-debug` build reports itself honestly (PM-88).
pub fn long_version_named(prog: &str) -> String {
    format!("{prog} {VERSION} ({COMMIT}, {DATE})")
}
