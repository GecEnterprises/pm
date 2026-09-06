//! Tiny hand-rolled arg parser for the `pm` / `pm-debug` binaries (PM-14, PM-5).
//!
//! `pm` is primarily a GUI: `pm` opens it with no project, `pm .` / `pm <path>`
//! open a folder. Everything else is a `--flag`: `--mcp`, `--setup`,
//! `--uninstall`, `--update`, `--version`, `--help`.

use std::path::PathBuf;

pub enum Command {
    /// Open the diff GUI on this folder, or with no project when `None`.
    Gui { path: Option<PathBuf>, jump: Jump },
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

/// Where to land after opening the GUI (PM-99) — lets manual and scripted
/// verification jump straight to the view under test instead of hand-editing
/// `Pm::new`'s defaults, rebuilding, and reverting each time.
#[derive(Default)]
pub struct Jump {
    pub view: Option<JumpView>,
    /// `--ticket <id>` — selects a ticket. Implies the Tickets view if `view`
    /// wasn't also given.
    pub ticket: Option<u64>,
    /// `--path <file>` — opens a file in the Files view (relative to the repo
    /// root, same as the Explorer tree). Implies the Files view if `view`
    /// wasn't also given.
    pub file: Option<PathBuf>,
    /// `--screenshot <out.png>` (PM-100) — after the window settles, capture
    /// it to this path and exit.
    pub screenshot: Option<PathBuf>,
}

impl Jump {
    pub(crate) fn is_empty(&self) -> bool {
        self.view.is_none() && self.ticket.is_none() && self.file.is_none() && self.screenshot.is_none()
    }
}

#[derive(Clone, Copy)]
pub enum JumpView {
    Summary,
    Files,
    Tickets,
}

pub fn parse() -> Command {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else {
        return Command::Gui { path: None, jump: Jump::default() };
    };
    let rest = &args[1..];
    let has_yes = rest.iter().any(|a| a == "--yes" || a == "-y");

    match first.as_str() {
        "--version" | "-V" => return Command::Version,
        "--help" | "-h" => return Command::Help,
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
            return Command::Mcp { project };
        }
        "--setup" => return Command::Setup { assume_yes: has_yes },
        "--uninstall" => return Command::Uninstall { assume_yes: has_yes },
        "--update" | "update" => return Command::Update,
        _ => {}
    }

    let (path, jump) = parse_gui_args(&args);
    Command::Gui { path, jump }
}

/// Everything that isn't a recognized subcommand opens the GUI. The first
/// token that isn't one of the flags below (or a value belonging to one) is
/// the path; the flags themselves are recognized anywhere, not just after the
/// path.
fn parse_gui_args(args: &[String]) -> (Option<PathBuf>, Jump) {
    let mut path = None;
    let mut jump = Jump::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--view" => {
                jump.view = it.next().and_then(|v| match v.as_str() {
                    "summary" => Some(JumpView::Summary),
                    "files" => Some(JumpView::Files),
                    "tickets" => Some(JumpView::Tickets),
                    _ => None,
                });
            }
            "--ticket" => jump.ticket = it.next().and_then(|v| v.parse().ok()),
            "--path" => jump.file = it.next().map(PathBuf::from),
            "--screenshot" => jump.screenshot = it.next().map(PathBuf::from),
            other if path.is_none() && !other.starts_with('-') => path = Some(PathBuf::from(other)),
            _ => {}
        }
    }
    (path, jump)
}

pub fn usage(prog: &str) -> String {
    format!(
        "\
{prog} — Plus Minus, a diff-oriented code viewer

USAGE:
    {prog}                      open {prog} with no project
    {prog} .                    open the diff GUI on the current directory
    {prog} <path>               open the diff GUI on <path>
    {prog} <path> --view <summary|files|tickets>
                                open <path> directly on a view (PM-99)
    {prog} <path> --view tickets --ticket <id>
                                open <path> on a ticket's detail pane
    {prog} <path> --view files --path <file>
                                open <path> on a specific file (relative to <path>)
    {prog} <path> --screenshot <out.png>
                                after the window settles, save a screenshot and exit (PM-100)
    {prog} --mcp [--project <p>] run the Model Context Protocol server on stdio
    {prog} --setup [--yes]      register {prog} (Start Menu, uninstaller, Claude Code MCP)
    {prog} --uninstall [--yes]  undo --setup and remove {prog}
    {prog} --update             update {prog} to the latest release (Windows)
    {prog} --version            print version and build info
    {prog} --help               show this message
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_gui(args: &[&str]) -> (Option<PathBuf>, Jump) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_gui_args(&args)
    }

    #[test]
    fn bare_path_unaffected() {
        let (path, jump) = parse_gui(&["."]);
        assert_eq!(path, Some(PathBuf::from(".")));
        assert!(jump.is_empty());
    }

    #[test]
    fn view_and_ticket() {
        let (path, jump) = parse_gui(&[".", "--view", "tickets", "--ticket", "96"]);
        assert_eq!(path, Some(PathBuf::from(".")));
        assert!(matches!(jump.view, Some(JumpView::Tickets)));
        assert_eq!(jump.ticket, Some(96));
    }

    #[test]
    fn flags_before_path() {
        let (path, jump) = parse_gui(&["--view", "summary", "."]);
        assert_eq!(path, Some(PathBuf::from(".")));
        assert!(matches!(jump.view, Some(JumpView::Summary)));
    }

    #[test]
    fn unknown_view_value_ignored() {
        let (_, jump) = parse_gui(&[".", "--view", "bogus"]);
        assert!(jump.view.is_none());
    }

    #[test]
    fn screenshot_path_and_file() {
        let (_, jump) = parse_gui(&[
            ".", "--view", "files", "--path", "src/lib.rs", "--screenshot", "out.png",
        ]);
        assert_eq!(jump.file, Some(PathBuf::from("src/lib.rs")));
        assert_eq!(jump.screenshot, Some(PathBuf::from("out.png")));
    }

    #[test]
    fn empty_args_is_empty_jump() {
        let (path, jump) = parse_gui(&[]);
        assert!(path.is_none());
        assert!(jump.is_empty());
    }
}
