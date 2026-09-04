//! Tiny hand-rolled arg parser for the single `pm` binary (PM-14).
//!
//! `pm` is primarily a GUI, so the default with no recognised subcommand is to
//! open a folder. The subcommands are the CLI surface: `mcp`, `update`,
//! `--version`, `--help`.

use std::path::PathBuf;

pub enum Command {
    /// Open the diff GUI on this folder (or cwd).
    Gui { path: Option<PathBuf> },
    /// Run the MCP server on stdio (`pm mcp [--project <path>]`).
    Mcp { project: Option<PathBuf> },
    /// Download and install the latest release over this binary.
    Update,
    /// Print `pm <version> (<commit>, <date>)` and exit.
    Version,
    /// Print usage and exit.
    Help,
}

const USAGE: &str = "\
pm — Plus Minus, a diff-oriented code viewer

USAGE:
    pm [<path>]              open the diff GUI on <path> (default: current dir)
    pm mcp [--project <p>]   run the Model Context Protocol server on stdio
    pm update               update pm to the latest release (Windows)
    pm --version            print version and build info
    pm --help               show this message
";

pub fn parse() -> Command {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else {
        return Command::Gui { path: None };
    };

    match first.as_str() {
        "--version" | "-V" => Command::Version,
        "--help" | "-h" => Command::Help,
        "mcp" => {
            let mut project = None;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--project" | "-p" => project = args.next().map(PathBuf::from),
                    other if !other.starts_with('-') => project = Some(PathBuf::from(other)),
                    _ => {}
                }
            }
            Command::Mcp { project }
        }
        "update" => Command::Update,
        // Anything else is treated as a path to open.
        path => Command::Gui { path: Some(PathBuf::from(path)) },
    }
}

pub fn usage() -> &'static str {
    USAGE
}
