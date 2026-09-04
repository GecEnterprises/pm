//! Embeds version / commit / build-date / release-notes into the binary so the
//! About box (and `pm --version`) can show exactly what's running (PM-27).

use std::path::Path;
use std::process::Command;

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());

    // Commit: CI passes $GITHUB_SHA; locally fall back to git; else "dev".
    let commit = std::env::var("GITHUB_SHA")
        .ok()
        .map(|s| s.chars().take(7).collect::<String>())
        .or_else(|| git(&["rev-parse", "--short", "HEAD"]))
        .unwrap_or_else(|| "dev".into());

    // Commit date (YYYY-MM-DD) — reproducible, unlike "now".
    let date = git(&["log", "-1", "--format=%cs"]).unwrap_or_else(|| "unknown".into());

    // Release notes for this exact version, if the file exists.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let notes_path = root.join("release-notes").join(format!("{version}.md"));
    let notes = std::fs::read_to_string(&notes_path)
        .unwrap_or_else(|_| format!("No release notes for {version}."));

    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("build_info.rs");
    std::fs::write(
        &out,
        format!(
            "pub const VERSION: &str = {version:?};\n\
             pub const COMMIT: &str = {commit:?};\n\
             pub const DATE: &str = {date:?};\n\
             pub const RELEASE_NOTES: &str = {notes:?};\n"
        ),
    )
    .unwrap();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", notes_path.display());
    println!("cargo:rerun-if-changed={}", root.join(".git/HEAD").display());
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}
