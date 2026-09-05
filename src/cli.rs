//! Tiny hand-rolled arg parser for the `pm` / `pm-debug` binaries (PM-14, PM-5).
//!
//! `pm` is primarily a GUI: `pm` opens it with no project, `pm .` / `pm <path>`
//! open a folder. Everything else is a `--flag`: `--mcp`, `--setup`,
//! `--uninstall`, `--update`, `--version`, `--help`.

use std::path::PathBuf;

pub enum Command {
    /// Open the diff GUI on this folder, or with no project when `None`.
    Gui { path: Option<PathBuf> },
    /// Run the MCP server on stdio (`pm --mcp [--project <path>]`).
    Mcp { project: Option<PathBuf> },
    /// First-run setup: registry, Start Menu, MCP client wiring.
    Setup { assume_yes: bool },
    /// Undo what `--setup` did and remove the binary.
    Uninstall { assume_yes: bool },
    /// Download and install the latest release over this binary.
    Update,
    /// Print `pm <version> (<commit>, <date>)` and exit.
    Version,
    /// Print usage and exit.
    Help,
}


pub fn parse() -> Command {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else {
        return Command::Gui { path: None };
    };
    let rest = &args[1..];
    let has_yes = rest.iter().any(|a| a == "--yes" || a == "-y");

    match first.as_str() {
        "--version" | "-V" => Command::Version,
        "--help" | "-h" => Command::Help,
        "--mcp" | "mcp" => {
            let mut project = None;
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--project" | "-p" => project = it.next().map(PathBuf::from),
                    other if !other.starts_with('-') => project = Some(PathBuf::from(other)),
                    _ => {}
                }
            }
            Command::Mcp { project }
        }
        "--setup" => Command::Setup { assume_yes: has_yes },
        "--uninstall" => Command::Uninstall { assume_yes: has_yes },
        "--update" | "update" => Command::Update,
        // Anything else is treated as a path to open.
        _ => Command::Gui { path: Some(PathBuf::from(first)) },
    }
}

pub fn usage(prog: &str) -> String {
    format!(
        "\
{prog} — Plus Minus, a diff-oriented code viewer

USAGE:
    {prog}                      open {prog} with no project
    {prog} .                    open the diff GUI on the current directory
    {prog} <path>               open the diff GUI on <path>
    {prog} --mcp [--project <p>] run the Model Context Protocol server on stdio
    {prog} --setup [--yes]      register {prog} (Start Menu, uninstaller, Claude Code MCP)
    {prog} --uninstall [--yes]  undo --setup and remove {prog}
    {prog} --update             update {prog} to the latest release (Windows)
    {prog} --version            print version and build info
    {prog} --help               show this message
"
    )
}
