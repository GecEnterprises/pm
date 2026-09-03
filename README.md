<p align="center"><img src="assets/icon.png" width="120" alt="pm"></p>

# pm

a diff-oriented code editor built with [gpui](https://github.com/zed-industries/zed).

pm opens a git repository and shows your changes like an editor: a resizable
sidebar with a "changes" list and a file tree, and a center pane with a
side-by-side line diff of head vs the working tree. every panel is a custom
gpui element that paints only its visible rows.

## run

```
cargo run                 # opens the current directory
cargo run -- path/to/repo
```

- click a file in **changes** or the **explorer** tree to diff it
- drag the sidebar edge, the changes/explorer split, or the diff's centre
  divider to resize
- mouse wheel scrolls; shift+wheel scrolls a diff column sideways
- `⟳` rescans

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
| `src/main.rs`      | window, panel layout, resize handles |

## status

early. working: repo discovery, changes list with status + ±loc badges, file
tree with icons, side-by-side syntax-highlighted diff, custom scrollbars,
resizable panels. not yet: editing, staging / unstaging hunks, word-level
intra-line diff, keyboard navigation.

## license

bsd 3-clause. see [license](LICENSE).
