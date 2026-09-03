# pm — Plus Minus

A diff-oriented code viewer built with [gpui](https://github.com/zed-industries/zed).

pm opens a folder, finds the enclosing git repository, lists every changed file,
and shows a **side-by-side line diff** of HEAD vs the working tree.

## Run

```sh
cargo run                # opens the current directory
cargo run -- path/to/repo
```

Click a file in the sidebar to diff it. `⟳` rescans for changes.

## Layout

| file          | role |
| ------------- | ---- |
| `src/git.rs`  | `git2` wrapper: changed files, HEAD blob, working-tree file |
| `src/diff.rs` | `similar` line diff aligned into side-by-side rows |
| `src/main.rs` | gpui window, sidebar, diff pane |

## Status

Bootstrap. Working: repo discovery, change list, side-by-side line diff, file
switching, rescan. Not yet: editing, staging/unstaging hunks, syntax
highlighting, virtualized scrolling, word-level intra-line diff.

<!-- try me: edit this line and rescan in pm -->
