<p align="center"><img src="assets/icon.png" width="120" alt="pm"></p>

# pm

a diff-oriented code editor built with [gpui](https://github.com/zed-industries/zed).

pm opens a git repository and shows your changes like an editor: a resizable
sidebar with a "changes" list and a file tree, and a center pane with a
side-by-side line diff of head vs the working tree. every panel is a custom
gpui element that paints only its visible rows.

## install (windows)

```powershell
irm https://raw.githubusercontent.com/GecEnterprises/pm/trunk/install.ps1 | iex
```

downloads the latest `pm.exe` into `%LOCALAPPDATA%\Programs\pm`, adds it to your
user PATH — no admin — then runs `pm --setup`, which registers a Start Menu
entry and an uninstaller (so pm shows up in Windows "Installed apps" /
BCUninstaller) and offers to wire pm's MCP server into Claude Code. windows may
show a SmartScreen "unknown publisher" warning (the binary isn't code-signed
yet); choose *More info → Run anyway*. `pm --update` pulls the next release in
place; `pm --uninstall` reverses everything.

## run

```
cargo run                 # opens with no project (pick one from the window)
cargo run -- .            # opens the current directory
cargo run -- path/to/repo
pm                        # once installed
```

`pm` is a single binary; everything past the GUI is a `--flag`:

| command | does |
| ------- | ---- |
| `pm` | open pm with nothing loaded — pick a project from the window |
| `pm .` / `pm <path>` | open the diff GUI on that folder |
| `pm --mcp [--project <p>]` | run the [MCP](crates/pm-mcp/README.md) server on stdio |
| `pm --setup [--yes]` | register pm (Start Menu, uninstaller, Claude Code MCP) |
| `pm --uninstall [--yes]` | undo `--setup` and remove pm |
| `pm --update` | update to the latest release (windows) |
| `pm --version` | print version + build commit |

- click a file in **changes** or the **explorer** tree to diff it
- drag the sidebar edge, the changes/explorer split, or the diff's centre
  divider to resize
- mouse wheel scrolls; shift+wheel scrolls a diff column sideways
- the working tree is watched — edits, stages, commits and checkouts update
  the changes list, tree and open diff live; `⟳` forces a rescan

## layout

| file               | role |
| ------------------ | ---- |
| `src/git.rs`       | `git2` wrapper: changed files + status + line counts, head/working blobs, tree walk |
| `src/diff.rs`      | `similar` line diff aligned into side-by-side rows |
| `src/highlight.rs` | `syntect` syntax highlighting |
| `src/scroll.rs`    | scroll-offset + scrollbar geometry |
| `src/diff_view.rs` | the diff body element — gutters, columns, scrollbars, draggable split |
| `src/list_view.rs` | the "changes" list element |
| `src/tree_view.rs` | the "explorer" file-tree element |
| `src/icons.rs`     | file-type icons (zed's `file_icons` set + the ported mapping tables) |
| `src/watch.rs`     | the sentinel — a `notify` filesystem watcher over the working tree |
| `src/main.rs`      | window, panel layout, resize handles |

## status

early. working: repo discovery, changes list with status + ±loc badges, file
tree with icons, side-by-side syntax-highlighted diff, custom scrollbars,
resizable panels, live filesystem watching. not yet: editing, staging /
unstaging hunks, word-level intra-line diff, keyboard navigation.

## license

bsd 3-clause. see [license](LICENSE).
